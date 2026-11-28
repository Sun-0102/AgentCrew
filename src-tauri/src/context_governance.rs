use crate::{agents, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

const AUTO_COMPACT_THRESHOLD_PERCENT: f64 = 80.0;
const COOLDOWN_MIN_TURNS: i64 = 3;

static COMPACTING_CONVERSATIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
static PENDING_ROLLOVER_CONTEXT: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();

fn compacting_map() -> &'static Mutex<HashMap<String, String>> {
    COMPACTING_CONVERSATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rollover_map() -> &'static Mutex<HashMap<(String, String), String>> {
    PENDING_ROLLOVER_CONTEXT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn current_compacting_agent(conversation_id: &str) -> Option<String> {
    compacting_map()
        .lock()
        .ok()
        .and_then(|g| g.get(conversation_id).cloned())
}

pub fn take_pending_rollover_context(conversation_id: &str, agent_id: &str) -> Option<String> {
    rollover_map()
        .lock()
        .ok()
        .and_then(|mut g| g.remove(&(conversation_id.to_owned(), agent_id.to_owned())))
}

pub fn get_auto_compact(store: &Store) -> Result<bool, String> {
    let val: Option<i64> = store
        .connect()?
        .query_row(
            "SELECT auto_compact_context FROM app_controls WHERE id=1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(val.unwrap_or(1) != 0)
}

pub fn set_auto_compact(store: &Store, enabled: bool) -> Result<(), String> {
    store
        .connect()?
        .execute(
            "UPDATE app_controls SET auto_compact_context=?1 WHERE id=1",
            [if enabled { 1 } else { 0 }],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}k", (n as f64 / 1_000.0).round() as i64)
    } else {
        n.to_string()
    }
}

fn clip_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let taken: String = s.chars().take(max_chars).collect();
    format!("{taken}…")
}

/// Build a structured rolling checkpoint summary from conversation messages up to `up_to_sequence`.
pub fn build_structured_summary(
    store: &Store,
    conversation_id: &str,
    up_to_sequence: Option<i64>,
) -> Result<(i64, String), String> {
    let connection = store.connect()?;
    let max_seq: i64 = match up_to_sequence {
        Some(seq) => seq,
        None => connection
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM codex_messages WHERE conversation_id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?,
    };
    let mut stmt = connection
        .prepare(
            "SELECT sequence, role, agent, text
             FROM codex_messages
             WHERE conversation_id=?1 AND sequence<=?2 AND role IN ('user','assistant')
             ORDER BY sequence ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id, max_seq], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok((max_seq, "（暂无可摘要的历史对话）".to_owned()));
    }

    let first_goal = rows
        .iter()
        .find(|(_, role, _, _)| role == "user")
        .map(|(_, _, _, text)| clip_str(text.trim(), 400))
        .unwrap_or_else(|| "未记录".to_owned());

    let mut conclusions = Vec::new();
    let mut open_questions = Vec::new();
    let mut file_mentions = HashSet::new();

    for (_, role, agent, text) in rows.iter().rev().take(24) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Extract plausible file paths (containing / and an extension)
        for token in trimmed.split(|c: char| c.is_whitespace() || matches!(c, '`' | '"' | '\'' | ',' | '，' | '。' | '(' | ')' | '[' | ']')) {
            if token.contains('/') && token.contains('.') && token.len() >= 4 && token.len() <= 120 && !token.starts_with("http") {
                file_mentions.insert(token.to_owned());
            }
        }
        if role == "assistant" {
            let who = agent.as_deref().unwrap_or("助手");
            let first_line = trimmed.lines().find(|l| !l.trim().is_empty()).unwrap_or(trimmed);
            if trimmed.contains('?') || trimmed.contains('？') || trimmed.contains("分歧") || trimmed.contains("待确认") {
                if open_questions.len() < 4 {
                    open_questions.push(format!("- [{who}] {}", clip_str(first_line, 180)));
                }
            }
            if conclusions.len() < 6 {
                conclusions.push(format!("- [{who}] {}", clip_str(first_line, 200)));
            }
        }
    }
    conclusions.reverse();
    open_questions.reverse();

    let mut files_vec: Vec<String> = file_mentions.into_iter().collect();
    files_vec.sort();
    files_vec.truncate(16);

    let mut sections = vec![
        format!("### 阶段检查点摘要（截至消息序号 {max_seq}，共 {} 条核心对话）", rows.len()),
        format!("**核心目标**：{first_goal}"),
    ];
    if !conclusions.is_empty() {
        sections.push(format!("**阶段结论与共识**：\n{}", conclusions.join("\n")));
    }
    if !open_questions.is_empty() {
        sections.push(format!("**待对齐/分歧项**：\n{}", open_questions.join("\n")));
    }
    if !files_vec.is_empty() {
        sections.push(format!("**涉及文件路径**：{}", files_vec.join("、")));
    }
    Ok((max_seq, sections.join("\n\n")))
}

pub fn ensure_checkpoint(store: &Store, conversation_id: &str) -> Result<Option<String>, String> {
    let connection = store.connect()?;
    let latest_cp_seq: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(up_to_sequence), 0) FROM context_checkpoints WHERE conversation_id=?1",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let new_turns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM codex_messages WHERE conversation_id=?1 AND sequence>?2 AND role IN ('user','assistant')",
            params![conversation_id, latest_cp_seq],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if new_turns >= 10 {
        let (max_seq, summary) = build_structured_summary(store, conversation_id, None)?;
        if max_seq > latest_cp_seq {
            connection
                .execute(
                    "INSERT INTO context_checkpoints(id, conversation_id, up_to_sequence, summary) VALUES(?1, ?2, ?3, ?4)",
                    params![format!("cp-{}", uuid::Uuid::new_v4()), conversation_id, max_seq, summary],
                )
                .map_err(|e| e.to_string())?;
        }
    }
    connection
        .query_row(
            "SELECT summary FROM context_checkpoints WHERE conversation_id=?1 ORDER BY up_to_sequence DESC LIMIT 1",
            [conversation_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactResult {
    pub compacted: bool,
    pub pre_tokens: i64,
    pub post_tokens: i64,
    pub mode: String,
    pub message: String,
}

fn clear_runtime_thread_id(
    store: &Store,
    conversation_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    let kind = agents::provider(store, agent_id)?;
    let table = agents::session_table(&kind)?;
    let runtime = agents::binding(store, agent_id, conversation_id)?;
    let connection = store.connect()?;
    if kind == "codex" {
        connection
            .execute(
                &format!("UPDATE {table} SET thread_id=NULL WHERE conversation_id=?1"),
                [&runtime],
            )
            .map_err(|e| e.to_string())?;
    } else {
        connection
            .execute(
                &format!("UPDATE {table} SET thread_id=NULL WHERE conversation_id=?1"),
                [&runtime],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn compact_or_rollover(
    app: Option<&tauri::AppHandle>,
    store: &Store,
    conversation_id: &str,
    agent_id: &str,
    force_manual: bool,
) -> Result<CompactResult, String> {
    let connection = store.connect()?;
    let current_seq: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM codex_messages WHERE conversation_id=?1",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if !force_manual {
        let last_seq: i64 = connection
            .query_row(
                "SELECT last_compacted_sequence FROM context_governance_state WHERE conversation_id=?1 AND agent_id=?2",
                params![conversation_id, agent_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        if last_seq > 0 {
            let assistant_turns_since: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM codex_messages WHERE conversation_id=?1 AND role='assistant' AND sequence>?2",
                    params![conversation_id, last_seq],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if assistant_turns_since < COOLDOWN_MIN_TURNS {
                return Err("上下文刚整理过但占用仍超过 80%，触发冷却保护。请手动精简要求或点击「从这里分叉」开启新会话。".into());
            }
        }
    }

    // Measure pre_tokens
    let mut sessions = serde_json::Map::new();
    if let Some(handle) = app {
        if let Ok(sess) = agents::session(handle, store, agent_id, conversation_id) {
            sessions.insert(agent_id.to_owned(), sess);
        }
    }
    let codex_rt = app.and_then(|h| h.try_state::<crate::codex::Runtime>());
    let ctx_usage = crate::usage::conversation_context(
        store,
        conversation_id,
        &[agent_id.to_owned()],
        &sessions,
        codex_rt.as_deref(),
    )?;
    let pre_tokens = ctx_usage
        .get(agent_id)
        .and_then(|v| v.get("usedTokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    if let Ok(mut guard) = compacting_map().lock() {
        guard.insert(conversation_id.to_owned(), agent_id.to_owned());
    }
    crate::events::emit_conversation_sync(conversation_id);

    let result = (|| -> Result<CompactResult, String> {
        let (max_seq, summary) = build_structured_summary(store, conversation_id, None)?;
        connection
            .execute(
                "INSERT INTO context_checkpoints(id, conversation_id, up_to_sequence, summary) VALUES(?1, ?2, ?3, ?4)",
                params![format!("cp-{}", uuid::Uuid::new_v4()), conversation_id, max_seq, summary],
            )
            .map_err(|e| e.to_string())?;

        // Build recent 4 turns verbatim for rollover handoff
        let mut stmt = connection
            .prepare(
                "SELECT role, agent, text FROM codex_messages
                 WHERE conversation_id=?1 AND role IN ('user','assistant')
                 ORDER BY sequence DESC LIMIT 4",
            )
            .map_err(|e| e.to_string())?;
        let mut recent = stmt
            .query_map([conversation_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        recent.reverse();
        let recent_text = recent
            .into_iter()
            .map(|(role, who, text)| {
                let label = if role == "user" {
                    "用户".to_owned()
                } else {
                    who.unwrap_or_else(|| "助手".to_owned())
                };
                format!("[{label}]\n{}", clip_str(&text, 1200))
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let handoff_context = format!(
            "【上下文无缝轮转背景（已自动整理历史，前端记录完整保留）】\n\n{summary}\n\n### 最近几轮原文\n{recent_text}"
        );
        let post_tokens = ((handoff_context.len() as f64) / 3.2).round() as i64;

        // Clear underlying thread_id so next turn starts a fresh runtime thread with handoff_context
        clear_runtime_thread_id(store, conversation_id, agent_id)?;
        if let Ok(mut guard) = rollover_map().lock() {
            guard.insert((conversation_id.to_owned(), agent_id.to_owned()), handoff_context);
        }

        // Record cooldown state
        connection
            .execute(
                "INSERT INTO context_governance_state(conversation_id, agent_id, last_compacted_sequence, last_compacted_at)
                 VALUES(?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
                 ON CONFLICT(conversation_id, agent_id) DO UPDATE SET
                   last_compacted_sequence=excluded.last_compacted_sequence,
                   last_compacted_at=excluded.last_compacted_at",
                params![conversation_id, agent_id, current_seq],
            )
            .map_err(|e| e.to_string())?;

        let notice = if pre_tokens > 0 {
            format!(
                "已整理上下文 · {} → {} tokens（历史记录已保留）",
                format_tokens(pre_tokens),
                format_tokens(post_tokens)
            )
        } else {
            format!(
                "已整理上下文 · 生成阶段摘要并轮转底层会话（约 {} tokens，历史记录已保留）",
                format_tokens(post_tokens)
            )
        };
        connection
            .execute(
                "INSERT INTO codex_messages(conversation_id, item_id, role, text) VALUES(?1, ?2, 'system', ?3)",
                params![conversation_id, format!("compact-{}", uuid::Uuid::new_v4()), notice],
            )
            .map_err(|e| e.to_string())?;

        Ok(CompactResult {
            compacted: true,
            pre_tokens,
            post_tokens,
            mode: "rollover".to_owned(),
            message: notice,
        })
    })();

    if let Ok(mut guard) = compacting_map().lock() {
        guard.remove(conversation_id);
    }
    crate::events::emit_conversation_sync(conversation_id);
    result
}

pub fn check_auto_compact_before_send(
    app: Option<&tauri::AppHandle>,
    store: &Store,
    conversation_id: &str,
    agent_id: &str,
) -> Result<Option<CompactResult>, String> {
    if !get_auto_compact(store).unwrap_or(true) {
        return Ok(None);
    }
    let mut sessions = serde_json::Map::new();
    if let Some(handle) = app {
        if let Ok(sess) = agents::session(handle, store, agent_id, conversation_id) {
            sessions.insert(agent_id.to_owned(), sess);
        }
    }
    let codex_rt = app.and_then(|h| h.try_state::<crate::codex::Runtime>());
    let ctx_usage = crate::usage::conversation_context(
        store,
        conversation_id,
        &[agent_id.to_owned()],
        &sessions,
        codex_rt.as_deref(),
    )?;
    let percent = ctx_usage
        .get(agent_id)
        .and_then(|v| v.get("percent"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if percent >= AUTO_COMPACT_THRESHOLD_PERCENT {
        let res = compact_or_rollover(app, store, conversation_id, agent_id, false)?;
        return Ok(Some(res));
    }
    Ok(None)
}

pub fn fork_conversation(
    store: &Store,
    agent: &str,
    project_id: Option<&str>,
    group_id: Option<&str>,
    session_id: Option<&str>,
    up_to_message_id: Option<&str>,
) -> Result<Value, String> {
    let source_conv = if let Some(g) = group_id {
        g.to_owned()
    } else if let (Some(_p), Some(s)) = (project_id, session_id) {
        s.to_owned()
    } else if let Some(s) = session_id {
        s.to_owned()
    } else {
        agents::private_conversation(store, agent, project_id)?
    };

    let connection = store.connect()?;
    let up_to_seq: Option<i64> = if let Some(msg_id) = up_to_message_id {
        connection
            .query_row(
                "SELECT sequence FROM codex_messages WHERE conversation_id=?1 AND item_id=?2",
                params![source_conv, msg_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
    } else {
        None
    };

    let (_, summary) = build_structured_summary(store, &source_conv, up_to_seq)?;
    let seed_msg = format!("【从历史会话分叉的上下文摘要】\n\n{summary}");

    if let Some(g) = group_id {
        let new_session = store.create_group_session(g, "分叉会话")?;
        let new_id = new_session.id.clone();
        connection
            .execute(
                "INSERT INTO codex_messages(conversation_id, item_id, role, text) VALUES(?1, ?2, 'system', ?3)",
                params![new_id, format!("fork-seed-{}", uuid::Uuid::new_v4()), seed_msg],
            )
            .map_err(|e| e.to_string())?;
        if let Ok(mut guard) = rollover_map().lock() {
            guard.insert((new_id.clone(), agent.to_owned()), seed_msg);
        }
        return Ok(json!({
            "kind": "group",
            "group_id": g,
            "session_id": new_id,
        }));
    }

    if let Some(p) = project_id {
        let new_session = store.create_project_session(p, "分叉会话")?;
        let new_id = new_session.id.clone();
        connection
            .execute(
                "INSERT INTO codex_messages(conversation_id, item_id, role, text) VALUES(?1, ?2, 'system', ?3)",
                params![new_id, format!("fork-seed-{}", uuid::Uuid::new_v4()), seed_msg],
            )
            .map_err(|e| e.to_string())?;
        if let Ok(mut guard) = rollover_map().lock() {
            guard.insert((new_id.clone(), agent.to_owned()), seed_msg);
        }
        return Ok(json!({
            "kind": "project",
            "project_id": p,
            "session_id": new_id,
            "agent": agent,
        }));
    }

    let new_session = store.create_private_session(agent, "分叉会话")?;
    let new_id = new_session.id.clone();
    connection
        .execute(
            "INSERT INTO codex_messages(conversation_id, item_id, role, text) VALUES(?1, ?2, 'system', ?3)",
            params![new_id, format!("fork-seed-{}", uuid::Uuid::new_v4()), seed_msg],
        )
        .map_err(|e| e.to_string())?;
    if let Ok(mut guard) = rollover_map().lock() {
        guard.insert((new_id.clone(), agent.to_owned()), seed_msg);
    }
    Ok(json!({
        "kind": "private",
        "session_id": new_id,
        "agent": agent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_compact_setting_defaults_true_and_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        assert!(get_auto_compact(&store).unwrap());
        set_auto_compact(&store, false).unwrap();
        assert!(!get_auto_compact(&store).unwrap());
    }

    #[test]
    fn checkpoint_and_cooldown_prevent_repeat_compact_loop() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let conn = store.connect().unwrap();
        let conv = "conv-test";
        conn.execute(
            "INSERT INTO conversations(id, workspace_path) VALUES(?1, ?2)",
            params![conv, dir.path().to_str().unwrap()],
        )
        .unwrap();
        for i in 1..=12 {
            let role = if i % 2 == 1 { "user" } else { "assistant" };
            conn.execute(
                "INSERT INTO codex_messages(conversation_id, item_id, role, text, agent) VALUES(?1, ?2, ?3, ?4, 'codex')",
                params![conv, format!("m-{i}"), role, format!("Message {i} touching src/lib.rs")],
            )
            .unwrap();
        }
        let cp = ensure_checkpoint(&store, conv).unwrap();
        assert!(cp.is_some());
        assert!(cp.unwrap().contains("src/lib.rs"));

        // First manual compact succeeds and records cooldown state
        let res = compact_or_rollover(None, &store, conv, "codex", true).unwrap();
        assert!(res.compacted);

        // Immediate auto compact without 3 new assistant turns hits cooldown
        let err = compact_or_rollover(None, &store, conv, "codex", false).unwrap_err();
        assert!(err.contains("冷却保护"));
    }
}
