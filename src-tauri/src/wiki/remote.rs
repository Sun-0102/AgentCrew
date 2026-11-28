use super::*;

/// Minimal zero-dependency HTTP/1.1 client for local LLM Wiki (127.0.0.1 / localhost).
/// Strictly read-only endpoints + optional /sources/rescan; never invokes /chat.
pub(super) fn local_http_request(
    method: &str,
    base_url: &str,
    path: &str,
    api_token: &str,
    body: Option<&str>,
    timeout_ms: u64,
) -> Result<(u16, String), String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| "仅支持本机 http:// 地址（如 http://127.0.0.1:19828）".to_owned())?;
    let (host_port, base_path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], rest[idx..].trim_end_matches('/')),
        None => (rest, ""),
    };
    let host_only = host_port.split(':').next().unwrap_or("");
    if host_only != "127.0.0.1" && host_only != "localhost" && host_only != "::1" {
        return Err("为保障零外泄与零云端费用，LLM Wiki 仅允许连接本机回环地址（127.0.0.1 / localhost）。".into());
    }
    let full_path = format!("{base_path}{path}");
    let addr = host_port
        .to_socket_addrs()
        .map_err(|e| format!("无法解析地址 {host_port}：{e}"))?
        .next()
        .ok_or_else(|| format!("无法解析地址 {host_port}"))?;
    let timeout = Duration::from_millis(timeout_ms);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("无法连接到 LLM Wiki ({host_port})：{e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let payload = body.unwrap_or("");
    let mut req = format!(
        "{method} {full_path} HTTP/1.1\r\nHost: {host_port}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if !api_token.trim().is_empty() {
        req.push_str(&format!("Authorization: Bearer {}\r\n", api_token.trim()));
        req.push_str(&format!("X-LLM-Wiki-Token: {}\r\n", api_token.trim()));
    }
    if !payload.is_empty() || method == "POST" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");
    req.push_str(payload);

    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("发送请求失败：{e}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("读取响应失败：{e}"))?;

    let split_pos = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "LLM Wiki 返回了无效的 HTTP 响应。".to_owned())?;
    let header_str = String::from_utf8_lossy(&raw[..split_pos]);
    let status_code = header_str
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let is_chunked = header_str
        .lines()
        .any(|line| line.to_ascii_lowercase().contains("transfer-encoding:") && line.to_ascii_lowercase().contains("chunked"));
    let body_bytes = &raw[split_pos + 4..];
    let decoded = if is_chunked {
        decode_chunked_body(body_bytes)
    } else {
        String::from_utf8_lossy(body_bytes).into_owned()
    };
    Ok((status_code, decoded))
}

fn decode_chunked_body(input: &[u8]) -> String {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < input.len() {
        let Some(rel_end) = input[cursor..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_line = String::from_utf8_lossy(&input[cursor..cursor + rel_end]);
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let Ok(chunk_len) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        cursor += rel_end + 2;
        if chunk_len == 0 {
            break;
        }
        let end = (cursor + chunk_len).min(input.len());
        out.extend_from_slice(&input[cursor..end]);
        cursor = (end + 2).min(input.len());
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_remote_projects(value: &Value) -> Vec<RemoteWikiProject> {
    let arr = value
        .as_array()
        .or_else(|| value.get("projects").and_then(|v| v.as_array()))
        .or_else(|| value.get("data").and_then(|v| v.as_array()))
        .or_else(|| value.get("items").and_then(|v| v.as_array()));
    let Some(items) = arr else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("project_id"))
            .or_else(|| item.get("projectId"))
            .or_else(|| item.get("slug"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_owned();
        let root_path = item
            .get("root_path")
            .or_else(|| item.get("rootPath"))
            .or_else(|| item.get("path"))
            .or_else(|| item.get("workspace_path"))
            .or_else(|| item.get("workspacePath"))
            .or_else(|| item.get("dir"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_owned();
        if id.is_empty() && root_path.is_empty() {
            continue;
        }
        let name = item
            .get("name")
            .or_else(|| item.get("title"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                Path::new(&root_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&id)
                    .to_owned()
            });
        let final_id = if id.is_empty() {
            format!("llm-wiki-{}", &sha256_hex(&root_path)[..12])
        } else {
            id
        };
        let content_subdir = detect_content_subdir(Path::new(&root_path));
        result.push(RemoteWikiProject {
            id: final_id,
            name,
            root_path,
            content_subdir,
        });
    }
    result
}

pub fn test_connection(base_url: &str, api_token: &str) -> WikiConnectionTest {
    let base = base_url.trim().trim_end_matches('/').to_owned();
    let health = local_http_request("GET", &base, "/api/v1/health", api_token, None, 2500);
    match health {
        Err(err) => WikiConnectionTest {
            ok: false,
            base_url: base,
            message: format!("{err}（离线模式下仍可直接只读已接入的本地目录）"),
            version: None,
            projects: Vec::new(),
        },
        Ok((status, body)) => {
            if !(200..300).contains(&status) {
                return WikiConnectionTest {
                    ok: false,
                    base_url: base,
                    message: format!("健康检查返回 HTTP {status}，请检查服务状态或 API Token。"),
                    version: None,
                    projects: Vec::new(),
                };
            }
            let version = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(str::to_owned));
            let projects_resp = local_http_request(
                "GET",
                &base,
                "/api/v1/projects",
                api_token,
                None,
                3000,
            );
            match projects_resp {
                Ok((401 | 403, _)) => WikiConnectionTest {
                    ok: false,
                    base_url: base,
                    message: "本机 LLM Wiki 已开启鉴权，请先填写 API Token 后再同步。".into(),
                    version,
                    projects: Vec::new(),
                },
                Ok((p_status, p_body)) if (200..300).contains(&p_status) => {
                    let projects = serde_json::from_str::<Value>(&p_body)
                        .map(|v| parse_remote_projects(&v))
                        .unwrap_or_default();
                    WikiConnectionTest {
                        ok: true,
                        base_url: base,
                        message: format!("已连接本机 LLM Wiki，发现 {} 个项目。", projects.len()),
                        version,
                        projects,
                    }
                }
                Ok((p_status, _)) => WikiConnectionTest {
                    ok: false,
                    base_url: base,
                    message: format!("读取项目列表失败（HTTP {p_status}）。"),
                    version,
                    projects: Vec::new(),
                },
                Err(err) => WikiConnectionTest {
                    ok: false,
                    base_url: base,
                    message: format!("读取项目列表失败：{err}"),
                    version,
                    projects: Vec::new(),
                },
            }
        }
    }
}

pub fn sync_llm_wiki_projects(store: &Store) -> Result<Vec<WikiSource>, String> {
    let settings = load_settings(store)?;
    let test = test_connection(&settings.base_url, &settings.api_token);
    if !test.ok {
        return Err(test.message);
    }
    let conn = store.connect()?;
    for project in &test.projects {
        if project.root_path.is_empty() {
            continue;
        }
        let inbox = detect_inbox_subdir(Path::new(&project.root_path));
        conn.execute(
            "INSERT INTO wiki_sources (id, name, source_kind, tier, root_path, content_subdir, inbox_subdir, index_state)
             VALUES (?1, ?2, 'llm_wiki', 'core_wiki', ?3, ?4, ?5, 'idle')
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               source_kind = 'llm_wiki',
               root_path = excluded.root_path,
               content_subdir = excluded.content_subdir,
               inbox_subdir = excluded.inbox_subdir",
            params![
                project.id,
                project.name,
                project.root_path,
                project.content_subdir,
                inbox
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    drop(conn);
    for project in &test.projects {
        if !project.root_path.is_empty() && Path::new(&project.root_path).is_dir() {
            let _ = index_source(store, &project.id, false);
        }
    }
    list_sources(store)
}
