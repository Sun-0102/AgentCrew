use super::*;

pub(super) fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

/// Inserts spaces around CJK characters so SQLite FTS5 unicode61 can match
/// Chinese substrings accurately while keeping ASCII identifiers (VIP3, S2, 600000) intact.
pub fn cjk_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 2);
    let mut prev_was_cjk = false;
    for ch in input.chars() {
        if is_cjk(ch) {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push(ch);
            out.push(' ');
            prev_was_cjk = true;
        } else {
            if prev_was_cjk && ch.is_alphanumeric() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push(ch);
            prev_was_cjk = false;
        }
    }
    out
}

/// Converts a user search string into a safe FTS5 MATCH expression.
fn build_fts_query(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut clauses: Vec<String> = Vec::new();
    for piece in trimmed.split_whitespace() {
        let cleaned: String = piece
            .chars()
            .filter(|c| c.is_alphanumeric() || is_cjk(*c) || *c == '_' || *c == '-')
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        // Split contiguous runs into ASCII tokens and CJK runs
        let mut ascii_buf = String::new();
        let mut cjk_chars: Vec<char> = Vec::new();
        let flush_ascii = |buf: &mut String, out: &mut Vec<String>| {
            if !buf.is_empty() {
                let escaped = buf.replace('"', "");
                if !escaped.is_empty() {
                    out.push(format!("\"{escaped}\""));
                }
                buf.clear();
            }
        };
        let flush_cjk = |chars: &mut Vec<char>, out: &mut Vec<String>| {
            if !chars.is_empty() {
                let phrase: String = chars
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push(format!("\"{phrase}\""));
                chars.clear();
            }
        };
        for ch in cleaned.chars() {
            if is_cjk(ch) {
                flush_ascii(&mut ascii_buf, &mut clauses);
                cjk_chars.push(ch);
            } else if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                flush_cjk(&mut cjk_chars, &mut clauses);
                ascii_buf.push(ch);
            }
        }
        flush_ascii(&mut ascii_buf, &mut clauses);
        flush_cjk(&mut cjk_chars, &mut clauses);
    }
    if clauses.is_empty() {
        return None;
    }
    // Use OR so multi-term natural language queries rank best matches via BM25
    // without failing when one descriptive word isn't verbatim in the doc.
    Some(clauses.join(" OR "))
}

#[derive(Debug, Default)]
pub(super) struct ParsedFrontmatter {
    pub(super) title: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) is_deprecated: bool,
    pub(super) body_start_offset: usize,
}

pub(super) fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
    let mut result = ParsedFrontmatter::default();
    let trimmed_bom = content.strip_prefix('\u{feff}').unwrap_or(content);
    let bom_len = content.len() - trimmed_bom.len();
    if !trimmed_bom.starts_with("---\n") && !trimmed_bom.starts_with("---\r\n") {
        result.body_start_offset = bom_len;
        if content.contains("#已废弃") || content.contains("#归档") {
            result.is_deprecated = true;
        }
        return result;
    }
    let after_open = if trimmed_bom.starts_with("---\r\n") { 5 } else { 4 };
    let rest = &trimmed_bom[after_open..];
    let mut close_offset = None;
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim();
        if bare == "---" || bare == "..." {
            close_offset = Some(after_open + offset + line.len());
            break;
        }
        offset += line.len();
    }
    let Some(end_idx) = close_offset else {
        result.body_start_offset = bom_len;
        return result;
    };
    let yaml_block = &trimmed_bom[after_open..after_open + offset];
    for line in yaml_block.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().trim_matches('"').trim_matches('\'').trim();
            match key.as_str() {
                "title" if !val.is_empty() => result.title = Some(val.to_owned()),
                "status" => {
                    let lower = val.to_ascii_lowercase();
                    if lower == "deprecated" || lower == "archived" || lower == "obsolete" || val.contains("废弃") || val.contains("归档") {
                        result.is_deprecated = true;
                    }
                }
                "tags" => {
                    let inner = val.trim_start_matches('[').trim_end_matches(']');
                    for tag in inner.split(',') {
                        let t = tag.trim().trim_matches('"').trim_matches('\'').trim();
                        if !t.is_empty() {
                            if t.eq_ignore_ascii_case("deprecated")
                                || t.contains("已废弃")
                                || t.contains("废弃")
                                || t == "归档"
                                || t == "#已废弃"
                                || t == "#归档"
                            {
                                result.is_deprecated = true;
                            }
                            result.tags.push(t.to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    result.body_start_offset = bom_len + end_idx;
    let body_tail = &content[result.body_start_offset..];
    if body_tail.contains("#已废弃") || body_tail.contains("#归档") {
        result.is_deprecated = true;
    }
    result
}

fn extract_title(rel_path: &str, fm_title: Option<String>, body: &str) -> String {
    if let Some(t) = fm_title {
        if !t.trim().is_empty() {
            return t;
        }
    }
    for line in body.lines().take(20) {
        let trimmed = line.trim();
        if let Some(h1) = trimmed.strip_prefix("# ") {
            let clean = h1.trim();
            if !clean.is_empty() {
                return clean.to_owned();
            }
        }
    }
    Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
        .to_owned()
}

fn extract_wikilinks(content: &str) -> Vec<String> {
    let mut links = BTreeSet::new();
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end_rel) = content[i + 2..].find("]]") {
                let raw = &content[i + 2..i + 2 + end_rel];
                let target = raw
                    .split('|')
                    .next()
                    .unwrap_or("")
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !target.is_empty() && !target.contains('\n') && target.len() <= 160 {
                    links.insert(target.to_owned());
                }
                i = i + 2 + end_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    links.into_iter().collect()
}

fn split_sections(title: &str, body: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_heading = title.to_owned();
    let mut current_buf = String::new();

    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(h2) = trimmed.strip_prefix("## ") {
            if !current_buf.trim().is_empty() {
                sections.push((current_heading.clone(), current_buf.trim().to_owned()));
            }
            current_heading = h2.trim().to_owned();
            current_buf.clear();
        } else {
            current_buf.push_str(line);
            current_buf.push('\n');
        }
    }
    if !current_buf.trim().is_empty() || sections.is_empty() {
        sections.push((current_heading, current_buf.trim().to_owned()));
    }
    sections
}

fn collect_markdown_files(base: &Path, current: &Path, out: &mut Vec<(String, PathBuf, i64, i64)>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            // Skip raw/ draft folder at root level
            if current == base && name_str == "raw" {
                continue;
            }
            collect_markdown_files(base, &path, out);
        } else if meta.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext != "md" && ext != "txt" {
                continue;
            }
            let Ok(rel) = path.strip_prefix(base) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let size_bytes = meta.len() as i64;
            out.push((rel_str, path, mtime_ms, size_bytes));
        }
    }
}

/// Incremental FTS5 indexer: compares (mtime_ms, size_bytes) and only re-parses changed files.
pub fn index_source(store: &Store, source_id: &str, force: bool) -> Result<IndexStats, String> {
    let (root_path, content_subdir): (String, String) = store
        .connect()?
        .query_row(
            "SELECT root_path, content_subdir FROM wiki_sources WHERE id = ?1",
            [source_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("未找到知识源 {source_id}：{e}"))?;

    let scan_root = source_content_root(&root_path, &content_subdir);
    if !scan_root.is_dir() {
        let _ = store.connect()?.execute(
            "UPDATE wiki_sources SET index_state = 'error' WHERE id = ?1",
            [source_id],
        );
        return Err(format!("知识库目录不可访问：{}", scan_root.display()));
    }

    let _ = store.connect()?.execute(
        "UPDATE wiki_sources SET index_state = 'indexing' WHERE id = ?1",
        [source_id],
    );

    let mut files = Vec::new();
    collect_markdown_files(&scan_root, &scan_root, &mut files);

    let mut conn = store.connect()?;
    let existing: HashMap<String, (i64, i64)> = {
        let mut stmt = conn
            .prepare("SELECT rel_path, mtime_ms, size_bytes FROM wiki_documents WHERE source_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([source_id], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<HashMap<_, _>, _>>()
            .map_err(|e| e.to_string())?
    };

    let seen: HashSet<String> = files.iter().map(|(rel, _, _, _)| rel.clone()).collect();
    let to_remove: Vec<String> = existing
        .keys()
        .filter(|rel| !seen.contains(*rel))
        .cloned()
        .collect();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for rel in &to_remove {
        tx.execute(
            "DELETE FROM wiki_sections_fts WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, rel],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM wiki_links WHERE source_id = ?1 AND from_path = ?2",
            params![source_id, rel],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM wiki_documents WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, rel],
        )
        .map_err(|e| e.to_string())?;
    }

    let mut indexed = 0usize;
    for (rel_path, abs_path, mtime_ms, size_bytes) in &files {
        if !force {
            if let Some(&(old_mtime, old_size)) = existing.get(rel_path) {
                if old_mtime == *mtime_ms && old_size == *size_bytes {
                    continue;
                }
            }
        }
        let Ok(content) = fs::read_to_string(abs_path) else {
            continue;
        };
        let fm = parse_frontmatter(&content);
        let body = &content[fm.body_start_offset..];
        let title = extract_title(rel_path, fm.title, body);
        let top_domain = rel_path
            .split_once('/')
            .map(|(dir, _)| dir.to_owned())
            .unwrap_or_default();
        let tags_joined = fm.tags.join(",");
        let is_deprecated = if fm.is_deprecated { 1 } else { 0 };

        tx.execute(
            "INSERT INTO wiki_documents (source_id, rel_path, top_domain, title, tags, is_deprecated, mtime_ms, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(source_id, rel_path) DO UPDATE SET
               top_domain = excluded.top_domain,
               title = excluded.title,
               tags = excluded.tags,
               is_deprecated = excluded.is_deprecated,
               mtime_ms = excluded.mtime_ms,
               size_bytes = excluded.size_bytes",
            params![
                source_id,
                rel_path,
                top_domain,
                title,
                tags_joined,
                is_deprecated,
                mtime_ms,
                size_bytes
            ],
        )
        .map_err(|e| e.to_string())?;

        tx.execute(
            "DELETE FROM wiki_sections_fts WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, rel_path],
        )
        .map_err(|e| e.to_string())?;

        for (heading, sec_body) in split_sections(&title, body) {
            let heading_idx = cjk_segment(&format!("{title} {heading} {tags_joined}"));
            let body_idx = cjk_segment(&sec_body);
            tx.execute(
                "INSERT INTO wiki_sections_fts (source_id, rel_path, section_heading, body)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source_id, rel_path, heading_idx, body_idx],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.execute(
            "DELETE FROM wiki_links WHERE source_id = ?1 AND from_path = ?2",
            params![source_id, rel_path],
        )
        .map_err(|e| e.to_string())?;
        for target in extract_wikilinks(&content) {
            tx.execute(
                "INSERT OR IGNORE INTO wiki_links (source_id, from_path, to_target) VALUES (?1, ?2, ?3)",
                params![source_id, rel_path, target],
            )
            .map_err(|e| e.to_string())?;
        }
        indexed += 1;
    }

    let active_docs: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM wiki_documents WHERE source_id = ?1 AND is_deprecated = 0",
            [source_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    tx.execute(
        "UPDATE wiki_sources
         SET doc_count = ?1, index_state = 'ready',
             last_indexed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?2",
        params![active_docs, source_id],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(IndexStats {
        source_id: source_id.to_owned(),
        scanned: files.len(),
        indexed,
        removed: to_remove.len(),
        active_docs: active_docs.max(0) as usize,
    })
}

fn clean_snippet_from_file(abs_path: &Path, query: &str, max_chars: usize) -> String {
    let Ok(content) = fs::read_to_string(abs_path) else {
        return String::new();
    };
    let fm = parse_frontmatter(&content);
    let body = content[fm.body_start_offset..].trim();
    let compact: String = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() {
        return String::new();
    }
    let terms: Vec<&str> = query
        .split_whitespace()
        .filter(|t| t.chars().count() >= 2)
        .collect();
    let lower_compact = compact.to_lowercase();
    let match_byte = terms
        .iter()
        .filter_map(|t| lower_compact.find(&t.to_lowercase()))
        .min();

    let chars: Vec<char> = compact.chars().collect();
    if let Some(byte_pos) = match_byte {
        let char_pos = compact[..byte_pos.min(compact.len())].chars().count();
        let start = char_pos.saturating_sub(24);
        let end = (start + max_chars).min(chars.len());
        let mut snippet: String = chars[start..end].iter().collect();
        if start > 0 {
            snippet = format!("…{snippet}");
        }
        if end < chars.len() {
            snippet.push('…');
        }
        snippet
    } else {
        let end = max_chars.min(chars.len());
        let mut snippet: String = chars[..end].iter().collect();
        if end < chars.len() {
            snippet.push('…');
        }
        snippet
    }
}

pub fn search_fts(
    store: &Store,
    source_id: Option<&str>,
    query: &str,
    domain_filter: Option<&str>,
    include_archives: bool,
    include_deprecated: bool,
    limit: usize,
    offset: usize,
) -> Result<Vec<WikiSearchHit>, String> {
    let conn = store.connect()?;
    let limit = limit.clamp(1, MAX_SEARCH_LIMIT) as i64;
    let offset = offset as i64;
    let domain = domain_filter
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "all")
        .map(str::to_owned);

    let fts_expr = build_fts_query(query);
    let source_ids: Vec<String> = source_id
        .map(parse_binding_source_ids)
        .unwrap_or_default();
    let has_source_filter = !source_ids.is_empty();
    let mut hits = Vec::new();

    if let Some(expr) = fts_expr {
        let mut sql = String::from(
            "SELECT d.source_id, s.name, s.root_path, s.content_subdir,
                    d.rel_path, d.top_domain, d.title, d.tags, d.is_deprecated,
                    f.section_heading,
                    bm25(wiki_sections_fts, 4.0, 1.0) AS rank
             FROM wiki_sections_fts f
             JOIN wiki_documents d ON d.source_id = f.source_id AND d.rel_path = f.rel_path
             JOIN wiki_sources s ON s.id = d.source_id
             WHERE wiki_sections_fts MATCH ?",
        );
        let mut bind_values: Vec<SqlValue> = vec![SqlValue::from(expr)];
        if has_source_filter {
            let placeholders = vec!["?"; source_ids.len()].join(", ");
            sql.push_str(&format!(" AND d.source_id IN ({placeholders})"));
            for sid in &source_ids {
                bind_values.push(SqlValue::from(sid.clone()));
            }
        } else if !include_archives {
            sql.push_str(" AND s.tier = 'core_wiki'");
        }
        if let Some(ref dom) = domain {
            sql.push_str(" AND d.top_domain = ?");
            bind_values.push(SqlValue::from(dom.clone()));
        }
        if !include_deprecated {
            sql.push_str(" AND d.is_deprecated = 0");
        }
        sql.push_str(" ORDER BY rank ASC LIMIT ? OFFSET ?");
        bind_values.push(SqlValue::from(limit * 2));
        bind_values.push(SqlValue::from(offset));

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params_from_iter(bind_values.iter()),
                |row| {
                    let sid: String = row.get(0)?;
                    let sname: String = row.get(1)?;
                    let root_path: String = row.get(2)?;
                    let sub: String = row.get(3)?;
                    let rel_path: String = row.get(4)?;
                    let top_domain: String = row.get(5)?;
                    let title: String = row.get(6)?;
                    let tags: String = row.get(7)?;
                    let is_dep: bool = row.get::<_, i64>(8)? != 0;
                    let raw_heading: String = row.get(9)?;
                    Ok((sid, sname, root_path, sub, rel_path, top_domain, title, tags, is_dep, raw_heading))
                },
            )
            .map_err(|e| e.to_string())?;

        let mut seen_docs = HashSet::new();
        for row in rows {
            let (sid, sname, root_path, sub, rel_path, top_domain, title, tags, is_dep, _raw_heading) =
                row.map_err(|e| e.to_string())?;
            if !seen_docs.insert((sid.clone(), rel_path.clone())) {
                continue;
            }
            let abs = source_content_root(&root_path, &sub).join(&rel_path);
            let snippet = clean_snippet_from_file(&abs, query, DEFAULT_SNIPPET_CHARS);
            hits.push(WikiSearchHit {
                source_id: sid,
                source_name: sname,
                rel_path,
                abs_path: abs.to_string_lossy().to_string(),
                top_domain,
                title,
                section_heading: String::new(),
                snippet,
                tags,
                is_deprecated: is_dep,
            });
            if hits.len() >= limit as usize {
                break;
            }
        }
    } else {
        // Empty query: paginated browse sorted by mtime DESC
        let mut sql = String::from(
            "SELECT d.source_id, s.name, s.root_path, s.content_subdir,
                    d.rel_path, d.top_domain, d.title, d.tags, d.is_deprecated
             FROM wiki_documents d
             JOIN wiki_sources s ON s.id = d.source_id
             WHERE 1 = 1",
        );
        let mut bind_values: Vec<SqlValue> = Vec::new();
        if has_source_filter {
            let placeholders = vec!["?"; source_ids.len()].join(", ");
            sql.push_str(&format!(" AND d.source_id IN ({placeholders})"));
            for sid in &source_ids {
                bind_values.push(SqlValue::from(sid.clone()));
            }
        } else if !include_archives {
            sql.push_str(" AND s.tier = 'core_wiki'");
        }
        if let Some(ref dom) = domain {
            sql.push_str(" AND d.top_domain = ?");
            bind_values.push(SqlValue::from(dom.clone()));
        }
        if !include_deprecated {
            sql.push_str(" AND d.is_deprecated = 0");
        }
        sql.push_str(" ORDER BY d.mtime_ms DESC, d.title COLLATE NOCASE LIMIT ? OFFSET ?");
        bind_values.push(SqlValue::from(limit));
        bind_values.push(SqlValue::from(offset));

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params_from_iter(bind_values.iter()),
                |row| {
                    let sid: String = row.get(0)?;
                    let sname: String = row.get(1)?;
                    let root_path: String = row.get(2)?;
                    let sub: String = row.get(3)?;
                    let rel_path: String = row.get(4)?;
                    let top_domain: String = row.get(5)?;
                    let title: String = row.get(6)?;
                    let tags: String = row.get(7)?;
                    let is_dep: bool = row.get::<_, i64>(8)? != 0;
                    Ok((sid, sname, root_path, sub, rel_path, top_domain, title, tags, is_dep))
                },
            )
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (sid, sname, root_path, sub, rel_path, top_domain, title, tags, is_dep) =
                row.map_err(|e| e.to_string())?;
            let abs = source_content_root(&root_path, &sub).join(&rel_path);
            let snippet = clean_snippet_from_file(&abs, "", DEFAULT_SNIPPET_CHARS);
            hits.push(WikiSearchHit {
                source_id: sid,
                source_name: sname,
                rel_path,
                abs_path: abs.to_string_lossy().to_string(),
                top_domain,
                title,
                section_heading: String::new(),
                snippet,
                tags,
                is_deprecated: is_dep,
            });
        }
    }

    Ok(hits)
}

pub(super) fn validate_rel_path(rel_path: &str) -> Result<PathBuf, String> {
    let trimmed = rel_path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("文档路径不能为空。".into());
    }
    let path = PathBuf::from(trimmed);
    for comp in path.components() {
        if matches!(comp, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err("非法文档路径，禁止越界访问。".into());
        }
    }
    Ok(path)
}

pub fn read_document(
    store: &Store,
    source_id: Option<&str>,
    rel_path_or_slug: &str,
    section: Option<&str>,
    max_bytes_override: Option<usize>,
) -> Result<WikiReadResult, String> {
    let conn = store.connect()?;
    let settings = load_settings(store)?;
    let budget = max_bytes_override.unwrap_or(settings.read_budget_bytes);

    // Resolve either exact rel_path or wikilink slug (e.g. "s2-price-lock-exception")
    let cleaned = rel_path_or_slug.trim().trim_start_matches('/');
    let resolved: Option<(String, String, String, String, String, String)> = if let Some(sid) = source_id {
        conn.query_row(
            "SELECT d.source_id, s.name, s.root_path, s.content_subdir, d.rel_path, d.title
             FROM wiki_documents d
             JOIN wiki_sources s ON s.id = d.source_id
             WHERE d.source_id = ?1
               AND (d.rel_path = ?2 OR d.rel_path LIKE '%/' || ?2 || '.md' OR d.rel_path = ?2 || '.md' OR d.title = ?2)
             ORDER BY d.is_deprecated ASC, CASE WHEN d.rel_path = ?2 THEN 0 ELSE 1 END
             LIMIT 1",
            params![sid, cleaned],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
    } else {
        conn.query_row(
            "SELECT d.source_id, s.name, s.root_path, s.content_subdir, d.rel_path, d.title
             FROM wiki_documents d
             JOIN wiki_sources s ON s.id = d.source_id
             WHERE (d.rel_path = ?1 OR d.rel_path LIKE '%/' || ?1 || '.md' OR d.rel_path = ?1 || '.md' OR d.title = ?1)
             ORDER BY d.is_deprecated ASC, CASE WHEN d.rel_path = ?1 THEN 0 ELSE 1 END
             LIMIT 1",
            params![cleaned],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
    };

    let (sid, sname, root_path, content_subdir, rel_path, title) =
        resolved.ok_or_else(|| format!("知识库中未找到文档：{rel_path_or_slug}"))?;
    let safe_rel = validate_rel_path(&rel_path)?;
    let abs = source_content_root(&root_path, &content_subdir).join(&safe_rel);
    let full_text = fs::read_to_string(&abs)
        .map_err(|e| format!("读取文档失败（{}）：{e}", abs.display()))?;

    let target_text = if let Some(sec_name) = section.map(str::trim).filter(|s| !s.is_empty()) {
        let fm = parse_frontmatter(&full_text);
        let body = &full_text[fm.body_start_offset..];
        let sections = split_sections(&title, body);
        let needle = sec_name.to_lowercase();
        sections
            .into_iter()
            .find(|(h, _)| h.to_lowercase().contains(&needle))
            .map(|(h, b)| format!("## {h}\n\n{b}"))
            .unwrap_or(full_text)
    } else {
        full_text
    };

    let (content, truncated) = truncate_utf8_bytes(&target_text, budget);

    // Fetch 1-hop [[wikilinks]] and resolve their rel_path + title if present in same source
    let mut link_stmt = conn
        .prepare("SELECT to_target FROM wiki_links WHERE source_id = ?1 AND from_path = ?2 ORDER BY to_target")
        .map_err(|e| e.to_string())?;
    let targets: Vec<String> = link_stmt
        .query_map(params![sid, rel_path], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut links = Vec::new();
    for target in targets {
        let resolved_link: Option<(String, String)> = conn
            .query_row(
                "SELECT rel_path, title FROM wiki_documents
                 WHERE source_id = ?1 AND is_deprecated = 0
                   AND (rel_path = ?2 OR rel_path = ?2 || '.md' OR rel_path LIKE '%/' || ?2 || '.md' OR title = ?2)
                 LIMIT 1",
                params![sid, target],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        links.push(WikiLinkRef {
            target,
            resolved_path: resolved_link.as_ref().map(|(p, _)| p.clone()),
            resolved_title: resolved_link.map(|(_, t)| t),
        });
    }

    Ok(WikiReadResult {
        source_id: sid,
        source_name: sname,
        rel_path,
        abs_path: abs.to_string_lossy().to_string(),
        title,
        section: section.map(str::to_owned),
        content,
        truncated,
        links,
    })
}
