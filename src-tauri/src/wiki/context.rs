use super::*;

fn build_habits_block(store: &Store, budget_bytes: usize) -> Result<String, String> {
    let pages = list_habit_pages(store)?;
    let mut parts = Vec::new();
    let mut used = 0usize;
    for page in pages.into_iter().filter(|p| p.enabled) {
        if used >= budget_bytes {
            break;
        }
        let remaining = budget_bytes.saturating_sub(used);
        if remaining < 80 {
            break;
        }
        if let Ok(doc) = read_document(store, Some(&page.source_id), &page.rel_path, None, Some(remaining)) {
            let fm = parse_frontmatter(&doc.content);
            let body = doc.content[fm.body_start_offset..].trim();
            let block = format!("### {}\n{}", doc.title, body);
            let (clipped, _) = truncate_utf8_bytes(&block, remaining);
            used += clipped.len();
            parts.push(clipped);
        }
    }
    Ok(parts.join("\n\n"))
}

fn build_l0_overview(
    store: &Store,
    binding: &WikiBinding,
    budget_bytes: usize,
) -> Result<String, String> {
    let conn = store.connect()?;
    let source_ids: Vec<String> = if !binding.source_ids.is_empty() {
        binding.source_ids.clone()
    } else {
        parse_binding_source_ids(&binding.source_id)
    };
    if source_ids.is_empty() {
        return Ok(String::new());
    }
    let mut lines = Vec::new();
    if source_ids.len() > 1 {
        lines.push(format!(
            "当前绑定 {} 个业务知识库（按需检索，切勿猜测未读条款）：",
            source_ids.len()
        ));
    }
    for sid in &source_ids {
        let source_name: String = conn
            .query_row(
                "SELECT name FROM wiki_sources WHERE id = ?1",
                [sid],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| sid.clone());
        let domains = source_domain_counts(&conn, sid)?;
        let total: usize = domains.iter().map(|d| d.count).sum();
        lines.push(format!(
            "当前绑定业务知识库：{source_name}（source_id=`{}`，有效文档 {total} 篇{}）",
            sid,
            binding
                .domain_filter
                .as_deref()
                .map(|d| format!("，限定目录 `{d}`"))
                .unwrap_or_default()
        ));
        if source_ids.len() == 1 {
            lines.push("L0 一级领域概览（仅列目录与数量，按需检索，切勿猜测未读条款）：".into());
        }
        for d in domains {
            if let Some(filter) = binding.domain_filter.as_deref() {
                if !filter.is_empty() && d.domain != filter {
                    continue;
                }
            }
            lines.push(format!("- {} ({} 篇)", d.domain, d.count));
        }
    }
    let joined = lines.join("\n");
    let (clipped, _) = truncate_utf8_bytes(&joined, budget_bytes);
    Ok(clipped)
}

/// Builds the runtime-only `extra_context` for a turn without polluting `codex_messages`.
/// - For code projects without an explicit wiki binding and without explicit refs: returns `None` (0 pollution).
/// - For private chats with Session Resume: checks `last_injected_hash` so habits + L0 card are injected once per version.
/// - Explicit `@note` refs are always injected deterministically for the current turn.
pub fn build_runtime_context(
    store: &Store,
    conversation_id: &str,
    is_group: bool,
    is_project_session: bool,
    explicit_refs: &[ExplicitWikiRef],
) -> Result<Option<String>, String> {
    let settings = load_settings(store)?;
    if !settings.enabled && explicit_refs.is_empty() {
        return Ok(None);
    }

    let explicit_binding = get_binding(store, conversation_id)?;
    let (binding, is_explicit_db_binding): (Option<WikiBinding>, bool) = match explicit_binding {
        Some(ref b) if b.source_id == BINDING_NONE_SENTINEL => (None, true),
        Some(b) => (Some(b), true),
        None if !is_group && !is_project_session => {
            if let Some(ref default_sid) = settings.default_private_source_id {
                (
                    Some(WikiBinding {
                        conversation_id: conversation_id.to_owned(),
                        source_id: default_sid.clone(),
                        source_ids: parse_binding_source_ids(default_sid),
                        domain_filter: None,
                        include_archives: false,
                        last_injected_hash: None,
                    }),
                    false,
                )
            } else {
                (None, false)
            }
        }
        None => (None, false),
    };
    // Code project sessions never receive business knowledge unless explicitly bound or explicitly @referenced.
    if is_project_session && binding.is_none() && explicit_refs.is_empty() {
        return Ok(None);
    }

    let mut persistent_sections = Vec::new();
    if settings.enabled {
        // Only inject global habits if this conversation is bound to a wiki OR is a non-project private chat with habits configured
        let should_include_habits = binding.is_some() || (!is_project_session && !is_group);
        if should_include_habits {
            let habits = build_habits_block(store, settings.habits_budget_bytes)?;
            if !habits.is_empty() {
                persistent_sections.push(format!(
                    "[个人通用工作习惯（跨业务常驻，仅用于汇报风格与精度，纯代码/闲聊任务无需强行套用）]\n{habits}"
                ));
            }
        }

        if let Some(ref b) = binding {
            let l0 = build_l0_overview(store, b, settings.l0_map_budget_bytes)?;
            let endpoint_hint = local_wiki_endpoint_hint(b);
            persistent_sections.push(format!(
                "[业务知识库 L0 概览与按需检索协议]\n{l0}\n{endpoint_hint}\n\
                防污染原则：\n\
                1. 日常寒暄、排版微调、纯代码/通用技术任务、以及同话题已读条款的追问，保持 0 检索，直接回答。\n\
                2. 仅当问题涉及内部业务黑话、SOP 审批口径、定价/返点公式、实体档案或历史特例时，先调 L1 搜索 Top-10，再对真正相关的 1~2 篇调 L2 精读；若精读页返回了相关 `[[双链]]` 例外条款，须按需再读一跳关联页，严禁漏看特例或臆造条款。\n\
                3. 回答业务问题时附上依据文档路径。"
            ));
        }
    }

    let mut final_blocks = Vec::new();
    if !persistent_sections.is_empty() {
        let persistent_payload = persistent_sections.join("\n\n");
        if is_group {
            final_blocks.push(persistent_payload);
        } else {
            let hash = sha256_hex(&persistent_payload);
            let mem_hash = SESSION_INJECTED_HASHES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .ok()
                .and_then(|g| g.get(conversation_id).cloned());
            let already_injected = if is_explicit_db_binding {
                binding
                    .as_ref()
                    .and_then(|b| b.last_injected_hash.as_deref())
                    == Some(hash.as_str())
                    || mem_hash.as_deref() == Some(hash.as_str())
            } else {
                mem_hash.as_deref() == Some(hash.as_str())
            };
            if !already_injected {
                final_blocks.push(persistent_payload);
                if let Ok(mut guard) = SESSION_INJECTED_HASHES
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                {
                    guard.insert(conversation_id.to_owned(), hash.clone());
                }
                if is_explicit_db_binding {
                    if let Some(ref b) = binding {
                        let _ = store.connect()?.execute(
                            "UPDATE conversation_wiki_bindings SET last_injected_hash = ?1 WHERE conversation_id = ?2",
                            params![hash, b.conversation_id],
                        );
                    }
                }
            }
        }
    }

    if !explicit_refs.is_empty() {
        let mut ref_blocks = Vec::new();
        for item in explicit_refs {
            if let Ok(doc) = read_document(
                store,
                Some(&item.source_id),
                &item.rel_path,
                item.section.as_deref(),
                Some(settings.read_budget_bytes),
            ) {
                let mut block = format!("### 引用文档：{} ({})\n{}", doc.title, doc.rel_path, doc.content);
                if !doc.links.is_empty() {
                    let link_names: Vec<String> = doc
                        .links
                        .iter()
                        .map(|l| match &l.resolved_path {
                            Some(p) => format!("[[{}]] ({p})", l.target),
                            None => format!("[[{}]]", l.target),
                        })
                        .collect();
                    block.push_str(&format!("\n\n关联双链：{}", link_names.join(", ")));
                }
                ref_blocks.push(block);
            }
        }
        if !ref_blocks.is_empty() {
            final_blocks.push(format!(
                "[用户本轮显式点名的知识库文档]\n{}",
                ref_blocks.join("\n\n---\n\n")
            ));
        }
    }

    if final_blocks.is_empty() {
        Ok(None)
    } else {
        Ok(Some(final_blocks.join("\n\n")))
    }
}

fn local_wiki_endpoint_hint(binding: &WikiBinding) -> String {
    let slot = LOCAL_HTTP_ENDPOINT.get_or_init(|| Mutex::new(None));
    let pair = slot.lock().ok().and_then(|guard| guard.clone());
    let Some((base, token)) = pair else {
        return String::new();
    };
    let domain_q = binding
        .domain_filter
        .as_deref()
        .map(|d| format!("&domain={}", url_encode(d)))
        .unwrap_or_default();
    let source_ids: Vec<String> = if !binding.source_ids.is_empty() {
        binding.source_ids.clone()
    } else {
        parse_binding_source_ids(&binding.source_id)
    };
    let search_source_param = source_ids.join(",");
    if source_ids.len() <= 1 {
        let sid = source_ids.first().map(String::as_str).unwrap_or(&binding.source_id);
        format!(
            "本机只读检索端点（100% 只读，可直接用 curl 访问）：\n\
             - L1 搜索：`curl -s -H 'Authorization: Bearer {token}' '{base}/local-wiki/search?source_id={}{domain_q}&q=<关键词>'`\n\
             - L2 精读：`curl -s -H 'Authorization: Bearer {token}' '{base}/local-wiki/read?source_id={}&path=<rel_path或双链名>'`（可加 `&section=<章节名>`）",
            url_encode(sid),
            url_encode(sid),
        )
    } else {
        format!(
            "本机只读检索端点（100% 只读，可直接用 curl 访问）：\n\
             - L1 搜索：`curl -s -H 'Authorization: Bearer {token}' '{base}/local-wiki/search?source_id={}{domain_q}&q=<关键词>'`\n\
             - L2 精读：`curl -s -H 'Authorization: Bearer {token}' '{base}/local-wiki/read?source_id=<目标source_id>&path=<rel_path或双链名>'`（可加 `&section=<章节名>`）",
            url_encode(&search_source_param),
        )
    }
}

fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn archive_note(
    store: &Store,
    source_id: &str,
    title: &str,
    content: &str,
    tags: &[String],
    source_conversation: Option<&str>,
) -> Result<ArchiveNoteResult, String> {
    let (root_path, source_kind, inbox_subdir): (String, String, String) = store
        .connect()?
        .query_row(
            "SELECT root_path, source_kind, inbox_subdir FROM wiki_sources WHERE id = ?1",
            [source_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("未找到目标知识库：{e}"))?;

    let safe_inbox = validate_rel_path(if inbox_subdir.trim().is_empty() {
        "00-inbox"
    } else {
        inbox_subdir.trim()
    })?;
    let inbox_dir = PathBuf::from(&root_path).join(&safe_inbox);
    fs::create_dir_all(&inbox_dir)
        .map_err(|e| format!("创建归档目录失败（{}）：{e}", inbox_dir.display()))?;

    let clean_title = if title.trim().is_empty() {
        "AgentCrew 沉淀结论"
    } else {
        title.trim()
    };
    let slug: String = clean_title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || is_cjk(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect();
    let slug = if slug.is_empty() {
        format!("note-{}", &now_iso()[..8.min(now_iso().len())])
    } else {
        slug
    };
    let date = now_date_slug();
    let mut filename = format!("{date}-{slug}.md");
    let mut abs_path = inbox_dir.join(&filename);
    if abs_path.exists() {
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
        filename = format!("{date}-{slug}-{suffix}.md");
        abs_path = inbox_dir.join(&filename);
    }

    let mut tag_list = vec!["agentcrew-inbox".to_owned()];
    for t in tags {
        let trimmed = t.trim();
        if !trimmed.is_empty() && !tag_list.iter().any(|x| x == trimmed) {
            tag_list.push(trimmed.to_owned());
        }
    }
    let frontmatter = format!(
        "---\ntitle: \"{}\"\ntype: inbox\ntags: [{}]\ncreated: {date}\nsource: \"{}\"\n---\n\n# {clean_title}\n\n{}\n",
        clean_title.replace('"', "'"),
        tag_list.join(", "),
        source_conversation.unwrap_or("AgentCrew").replace('"', "'"),
        content.trim()
    );
    fs::write(&abs_path, frontmatter)
        .map_err(|e| format!("写入知识库文件失败：{e}"))?;

    // Trigger incremental index update (best effort)
    let _ = index_source(store, source_id, false);

    let mut rescan_triggered = false;
    if source_kind == "llm_wiki" {
        if let Ok(settings) = load_settings(store) {
            let path = format!("/api/v1/projects/{}/sources/rescan", url_encode(source_id));
            if let Ok((status, _)) = local_http_request(
                "POST",
                &settings.base_url,
                &path,
                &settings.api_token,
                Some("{}"),
                1500,
            ) {
                rescan_triggered = (200..300).contains(&status);
            }
        }
    }

    let rel_path = Path::new(&safe_inbox)
        .join(&filename)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(ArchiveNoteResult {
        source_id: source_id.to_owned(),
        rel_path,
        abs_path: abs_path.to_string_lossy().to_string(),
        rescan_triggered,
    })
}

/// Helper used by `pairing.rs` to serve GET /local-wiki/search
pub fn http_search_json(
    store: &Store,
    source_id: Option<&str>,
    query: &str,
    domain: Option<&str>,
    include_archives: bool,
    limit: usize,
) -> Result<Value, String> {
    let hits = search_fts(
        store,
        source_id,
        query,
        domain,
        include_archives,
        false,
        limit.clamp(1, 15),
        0,
    )?;
    Ok(json!({
        "query": query,
        "count": hits.len(),
        "hits": hits
    }))
}

/// Helper used by `pairing.rs` to serve GET /local-wiki/read
pub fn http_read_json(
    store: &Store,
    source_id: Option<&str>,
    path: &str,
    section: Option<&str>,
) -> Result<Value, String> {
    let doc = read_document(store, source_id, path, section, None)?;
    serde_json::to_value(doc).map_err(|e| e.to_string())
}

