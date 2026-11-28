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

mod commands;
mod context;
mod indexer;
mod remote;

pub use commands::*;
pub use context::*;
pub use indexer::*;
pub use remote::*;

#[cfg(test)]
mod tests;
