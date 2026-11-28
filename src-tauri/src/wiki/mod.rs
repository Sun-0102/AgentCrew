use crate::storage::Store;
use rusqlite::{params, params_from_iter, types::Value as SqlValue, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static LOCAL_HTTP_ENDPOINT: OnceLock<Mutex<Option<(String, String)>>> = OnceLock::new();
static PENDING_GROUP_REFS: OnceLock<Mutex<HashMap<String, Vec<ExplicitWikiRef>>>> = OnceLock::new();
static SESSION_INJECTED_HASHES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

pub const BINDING_NONE_SENTINEL: &str = "__none__";

pub fn parse_binding_source_ids(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == BINDING_NONE_SENTINEL {
        return Vec::new();
    }
    let items: Vec<String> = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_else(|_| vec![trimmed.to_owned()])
    } else if trimmed.contains(',') {
        trimmed.split(',').map(|s| s.trim().to_owned()).collect()
    } else {
        vec![trimmed.to_owned()]
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let s = item.trim();
        if !s.is_empty() && s != BINDING_NONE_SENTINEL && seen.insert(s.to_owned()) {
            out.push(s.to_owned());
        }
    }
    out
}

pub fn encode_binding_source_ids(ids: &[String]) -> Option<String> {
    let mut seen = HashSet::new();
    let mut cleaned = Vec::new();
    for item in ids {
        let s = item.trim();
        if !s.is_empty() && s != BINDING_NONE_SENTINEL && seen.insert(s.to_owned()) {
            cleaned.push(s.to_owned());
        }
    }
    match cleaned.len() {
        0 => None,
        1 => Some(cleaned.remove(0)),
        _ => serde_json::to_string(&cleaned).ok(),
    }
}

pub fn set_local_http_endpoint(base_url: String, token: String) {
    let slot = LOCAL_HTTP_ENDPOINT.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some((base_url, token));
    }
}

pub fn set_pending_group_refs(conversation_id: &str, refs: Vec<ExplicitWikiRef>) {
    if refs.is_empty() {
        return;
    }
    let map = PENDING_GROUP_REFS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = map.lock() {
        guard.insert(conversation_id.to_owned(), refs);
    }
}

pub fn take_pending_group_refs(conversation_id: &str) -> Vec<ExplicitWikiRef> {
    let map = PENDING_GROUP_REFS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = map.lock() {
        guard.remove(conversation_id).unwrap_or_default()
    } else {
        Vec::new()
    }
}

const MAX_SEARCH_LIMIT: usize = 30;
const DEFAULT_SNIPPET_CHARS: usize = 110;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSettings {
    pub enabled: bool,
    pub base_url: String,
    pub api_token: String,
    pub habits_budget_bytes: usize,
    pub l0_map_budget_bytes: usize,
    pub read_budget_bytes: usize,
    #[serde(default)]
    pub default_private_source_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiDomainCount {
    pub domain: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSource {
    pub id: String,
    pub name: String,
    pub source_kind: String,
    pub tier: String,
    pub root_path: String,
    pub content_subdir: String,
    pub inbox_subdir: String,
    pub doc_count: usize,
    pub index_state: String,
    pub last_indexed_at: Option<String>,
    pub domains: Vec<WikiDomainCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiHabitPage {
    pub source_id: String,
    pub source_name: String,
    pub rel_path: String,
    pub title: String,
    pub enabled: bool,
    pub sort_order: i64,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiBinding {
    pub conversation_id: String,
    pub source_id: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    pub domain_filter: Option<String>,
    pub include_archives: bool,
    pub last_injected_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSearchHit {
    pub source_id: String,
    pub source_name: String,
    pub rel_path: String,
    pub abs_path: String,
    pub top_domain: String,
    pub title: String,
    pub section_heading: String,
    pub snippet: String,
    pub tags: String,
    pub is_deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiLinkRef {
    pub target: String,
    pub resolved_path: Option<String>,
    pub resolved_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiReadResult {
    pub source_id: String,
    pub source_name: String,
    pub rel_path: String,
    pub abs_path: String,
    pub title: String,
    pub section: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub links: Vec<WikiLinkRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplicitWikiRef {
    pub source_id: String,
    pub rel_path: String,
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiConnectionTest {
    pub ok: bool,
    pub base_url: String,
    pub message: String,
    pub version: Option<String>,
    pub projects: Vec<RemoteWikiProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWikiProject {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub content_subdir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub source_id: String,
    pub scanned: usize,
    pub indexed: usize,
    pub removed: usize,
    pub active_docs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveNoteResult {
    pub source_id: String,
    pub rel_path: String,
    pub abs_path: String,
    pub rescan_triggered: bool,
}

fn now_iso() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ms}")
}

fn now_date_slug() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // Simple UTC YYYY-MM-DD calculation without extra crates
    let days = secs.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> (String, bool) {
    if input.len() <= max_bytes {
        return (input.to_owned(), false);
    }
    let mut end = max_bytes.min(input.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_owned(), true)
}

fn sha256_hex(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn is_cjk(ch: char) -> bool {
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

pub fn load_settings(store: &Store) -> Result<WikiSettings, String> {
    let conn = store.connect()?;
    conn.query_row(
        "SELECT enabled, base_url, api_token, habits_budget_bytes, l0_map_budget_bytes, read_budget_bytes, default_private_source_id, updated_at
         FROM wiki_settings WHERE id = 1",
        [],
        |row| {
            Ok(WikiSettings {
                enabled: row.get::<_, i64>(0)? != 0,
                base_url: row.get(1)?,
                api_token: row.get(2)?,
                habits_budget_bytes: row.get::<_, i64>(3)?.max(200) as usize,
                l0_map_budget_bytes: row.get::<_, i64>(4)?.max(200) as usize,
                read_budget_bytes: row.get::<_, i64>(5)?.max(1000) as usize,
                default_private_source_id: row.get::<_, Option<String>>(6)?.filter(|s| !s.trim().is_empty()),
                updated_at: row.get(7)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

pub fn save_settings(store: &Store, input: &WikiSettings) -> Result<WikiSettings, String> {
    let base_url = input.base_url.trim().trim_end_matches('/').to_owned();
    let base_url = if base_url.is_empty() {
        "http://127.0.0.1:19828".to_owned()
    } else {
        base_url
    };
    let habits_budget = input.habits_budget_bytes.clamp(200, 12_000) as i64;
    let l0_budget = input.l0_map_budget_bytes.clamp(200, 4_000) as i64;
    let read_budget = input.read_budget_bytes.clamp(1000, 24_000) as i64;
    let default_private = input
        .default_private_source_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    store.connect()?.execute(
        "UPDATE wiki_settings
         SET enabled = ?1, base_url = ?2, api_token = ?3,
             habits_budget_bytes = ?4, l0_map_budget_bytes = ?5, read_budget_bytes = ?6,
             default_private_source_id = ?7,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
        params![
            if input.enabled { 1 } else { 0 },
            base_url,
            input.api_token.trim(),
            habits_budget,
            l0_budget,
            read_budget,
            default_private
        ],
    )
    .map_err(|e| e.to_string())?;
    load_settings(store)
}

/// Minimal zero-dependency HTTP/1.1 client for local LLM Wiki (127.0.0.1 / localhost).
/// Strictly read-only endpoints + optional /sources/rescan; never invokes /chat.
fn local_http_request(
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

fn detect_content_subdir(root: &Path) -> String {
    if root.join("wiki").is_dir() {
        "wiki".to_owned()
    } else {
        String::new()
    }
}

fn detect_inbox_subdir(root: &Path) -> String {
    if root.join("00-inbox").is_dir() {
        "00-inbox".to_owned()
    } else if root.join("raw").join("sources").is_dir() {
        "raw/sources".to_owned()
    } else {
        "00-inbox".to_owned()
    }
}

fn source_content_root(root_path: &str, content_subdir: &str) -> PathBuf {
    let root = PathBuf::from(root_path);
    let sub = content_subdir.trim().trim_matches('/');
    if !sub.is_empty() && root.join(sub).is_dir() {
        root.join(sub)
    } else {
        root
    }
}

pub fn list_sources(store: &Store) -> Result<Vec<WikiSource>, String> {
    let conn = store.connect()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, source_kind, tier, root_path, content_subdir, inbox_subdir,
                    doc_count, index_state, last_indexed_at
             FROM wiki_sources
             ORDER BY CASE tier WHEN 'core_wiki' THEN 0 ELSE 1 END, name COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(WikiSource {
                id: row.get(0)?,
                name: row.get(1)?,
                source_kind: row.get(2)?,
                tier: row.get(3)?,
                root_path: row.get(4)?,
                content_subdir: row.get(5)?,
                inbox_subdir: row.get(6)?,
                doc_count: row.get::<_, i64>(7)?.max(0) as usize,
                index_state: row.get(8)?,
                last_indexed_at: row.get(9)?,
                domains: Vec::new(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut sources: Vec<WikiSource> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    for src in &mut sources {
        src.domains = source_domain_counts(&conn, &src.id)?;
    }
    Ok(sources)
}

fn source_domain_counts(conn: &rusqlite::Connection, source_id: &str) -> Result<Vec<WikiDomainCount>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT top_domain, COUNT(*)
             FROM wiki_documents
             WHERE source_id = ?1 AND is_deprecated = 0
             GROUP BY top_domain
             ORDER BY COUNT(*) DESC, top_domain ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([source_id], |row| {
            let domain: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok(WikiDomainCount {
                domain: if domain.is_empty() { "root".into() } else { domain },
                count: count.max(0) as usize,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn add_local_source(
    store: &Store,
    root_path: &str,
    name: Option<&str>,
    tier: Option<&str>,
    content_subdir: Option<&str>,
) -> Result<WikiSource, String> {
    let canonical = fs::canonicalize(root_path.trim())
        .map_err(|e| format!("目录不存在或无法访问（{root_path}）：{e}"))?;
    if !canonical.is_dir() {
        return Err("所选路径不是有效目录。".into());
    }
    let root_str = canonical.to_string_lossy().to_string();
    let display_name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            canonical
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("本地知识库")
                .to_owned()
        });
    let tier_val = match tier.unwrap_or("core_wiki") {
        "archive" => "archive",
        _ => "core_wiki",
    };
    let sub = content_subdir
        .map(|s| s.trim().trim_matches('/').to_owned())
        .unwrap_or_else(|| detect_content_subdir(&canonical));
    let inbox = detect_inbox_subdir(&canonical);
    let source_id = format!("local-{}", &sha256_hex(&root_str)[..12]);

    store.connect()?.execute(
        "INSERT INTO wiki_sources (id, name, source_kind, tier, root_path, content_subdir, inbox_subdir, index_state)
         VALUES (?1, ?2, 'local_folder', ?3, ?4, ?5, ?6, 'idle')
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           tier = excluded.tier,
           root_path = excluded.root_path,
           content_subdir = excluded.content_subdir,
           inbox_subdir = excluded.inbox_subdir",
        params![source_id, display_name, tier_val, root_str, sub, inbox],
    )
    .map_err(|e| e.to_string())?;

    let _ = index_source(store, &source_id, false)?;
    list_sources(store)?
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| "创建知识源后未能读取记录。".into())
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

pub fn remove_source(store: &Store, source_id: &str) -> Result<(), String> {
    let mut conn = store.connect()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM wiki_sections_fts WHERE source_id = ?1", [source_id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM wiki_links WHERE source_id = ?1", [source_id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM wiki_documents WHERE source_id = ?1", [source_id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM wiki_habit_pages WHERE source_id = ?1", [source_id])
        .map_err(|e| e.to_string())?;
    let existing_bindings: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare("SELECT conversation_id, source_id FROM conversation_wiki_bindings")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.filter_map(Result::ok).collect()
    };
    for (cid, raw_sid) in existing_bindings {
        if raw_sid == BINDING_NONE_SENTINEL {
            continue;
        }
        let ids = parse_binding_source_ids(&raw_sid);
        if !ids.iter().any(|id| id == source_id) {
            continue;
        }
        let remaining: Vec<String> = ids.into_iter().filter(|id| id != source_id).collect();
        if let Some(encoded) = encode_binding_source_ids(&remaining) {
            tx.execute(
                "UPDATE conversation_wiki_bindings SET source_id = ?1, last_injected_hash = NULL WHERE conversation_id = ?2",
                params![encoded, cid],
            )
            .map_err(|e| e.to_string())?;
        } else {
            tx.execute(
                "DELETE FROM conversation_wiki_bindings WHERE conversation_id = ?1",
                [&cid],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.execute(
        "UPDATE wiki_settings SET default_private_source_id = NULL WHERE default_private_source_id = ?1",
        [source_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM wiki_sources WHERE id = ?1", [source_id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[derive(Debug, Default)]
struct ParsedFrontmatter {
    title: Option<String>,
    tags: Vec<String>,
    is_deprecated: bool,
    body_start_offset: usize,
}

fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
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

fn validate_rel_path(rel_path: &str) -> Result<PathBuf, String> {
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

pub fn list_habit_pages(store: &Store) -> Result<Vec<WikiHabitPage>, String> {
    let conn = store.connect()?;
    let mut stmt = conn
        .prepare(
            "SELECT h.source_id, COALESCE(s.name, h.source_id), h.rel_path, h.title, h.enabled, h.sort_order,
                    COALESCE(d.size_bytes, 0)
             FROM wiki_habit_pages h
             LEFT JOIN wiki_sources s ON s.id = h.source_id
             LEFT JOIN wiki_documents d ON d.source_id = h.source_id AND d.rel_path = h.rel_path
             ORDER BY h.sort_order ASC, h.title COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WikiHabitPage {
                source_id: r.get(0)?,
                source_name: r.get(1)?,
                rel_path: r.get(2)?,
                title: r.get(3)?,
                enabled: r.get::<_, i64>(4)? != 0,
                sort_order: r.get(5)?,
                size_bytes: r.get::<_, i64>(6)?.max(0) as usize,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn set_habit_page(
    store: &Store,
    source_id: &str,
    rel_path: &str,
    title: &str,
    enabled: bool,
) -> Result<Vec<WikiHabitPage>, String> {
    let conn = store.connect()?;
    if enabled {
        conn.execute(
            "INSERT INTO wiki_habit_pages (source_id, rel_path, title, enabled)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(source_id, rel_path) DO UPDATE SET
               title = excluded.title,
               enabled = 1",
            params![source_id, rel_path, title],
        )
        .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "DELETE FROM wiki_habit_pages WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, rel_path],
        )
        .map_err(|e| e.to_string())?;
    }
    drop(conn);
    list_habit_pages(store)
}

pub fn get_binding(store: &Store, conversation_id: &str) -> Result<Option<WikiBinding>, String> {
    let conn = store.connect()?;
    let direct = conn
        .query_row(
            "SELECT conversation_id, source_id, domain_filter, include_archives, last_injected_hash
             FROM conversation_wiki_bindings WHERE conversation_id = ?1",
            [conversation_id],
            |r| {
                let raw_sid: String = r.get(1)?;
                let source_ids = parse_binding_source_ids(&raw_sid);
                Ok(WikiBinding {
                    conversation_id: r.get(0)?,
                    source_id: raw_sid,
                    source_ids,
                    domain_filter: r.get(2)?,
                    include_archives: r.get::<_, i64>(3)? != 0,
                    last_injected_hash: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if direct.is_some() {
        return Ok(direct);
    }
    // Fallback: if conversation_id is a group session, inherit the parent group's wiki binding
    let group_id: Option<String> = conn
        .query_row(
            "SELECT group_id FROM group_sessions WHERE conversation_id = ?1",
            [conversation_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(gid) = group_id else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT conversation_id, source_id, domain_filter, include_archives, last_injected_hash
         FROM conversation_wiki_bindings WHERE conversation_id = ?1",
        [&gid],
        |r| {
            let raw_sid: String = r.get(1)?;
            let source_ids = parse_binding_source_ids(&raw_sid);
            Ok(WikiBinding {
                conversation_id: r.get(0)?,
                source_id: raw_sid,
                source_ids,
                domain_filter: r.get(2)?,
                include_archives: r.get::<_, i64>(3)? != 0,
                last_injected_hash: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn set_binding(
    store: &Store,
    conversation_id: &str,
    source_id: Option<&str>,
    domain_filter: Option<&str>,
    include_archives: bool,
) -> Result<Option<WikiBinding>, String> {
    let conn = store.connect()?;
    if let Ok(mut guard) = SESSION_INJECTED_HASHES.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        guard.remove(conversation_id);
    }
    let sid = source_id.map(str::trim).filter(|s| !s.is_empty());
    let Some(sid) = sid else {
        conn.execute(
            "DELETE FROM conversation_wiki_bindings WHERE conversation_id = ?1",
            [conversation_id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(None);
    };
    let domain = domain_filter.map(str::trim).filter(|s| !s.is_empty());
    conn.execute(
        "INSERT INTO conversation_wiki_bindings (conversation_id, source_id, domain_filter, include_archives, last_injected_hash)
         VALUES (?1, ?2, ?3, ?4, NULL)
         ON CONFLICT(conversation_id) DO UPDATE SET
           source_id = excluded.source_id,
           domain_filter = excluded.domain_filter,
           include_archives = excluded.include_archives,
           last_injected_hash = NULL",
        params![conversation_id, sid, domain, if include_archives { 1 } else { 0 }],
    )
    .map_err(|e| e.to_string())?;
    drop(conn);
    get_binding(store, conversation_id)
}

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

mod commands;
pub use commands::*;

#[cfg(test)]
mod tests;
