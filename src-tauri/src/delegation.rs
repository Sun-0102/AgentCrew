use crate::{agents, chat_queue, collaboration::dispatch::parse_dispatch_blocks, providers, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

const MAX_DELEGATION_HOPS: u32 = 20;

#[derive(Debug, Clone)]
struct WatchEntry {
    conversation_id: String,
    lead_agent: String,
    lead_model: String,
    last_checked_sequence: i64,
    hop_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedTask {
    pub id: String,
    pub parent_conversation_id: String,
    pub parent_message_id: Option<String>,
    pub lead_agent: String,
    pub lead_model: String,
    pub worker_agent: String,
    pub worker_model: String,
    pub worker_conversation_id: String,
    pub instruction: String,
    pub return_to_sender: bool,
    pub status: String,
    pub hop_count: u32,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub changed_files_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedTaskMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelegatedTaskDetail {
    pub task: DelegatedTask,
    pub messages: Vec<DelegatedTaskMessage>,
    pub turn_diffs: Value,
}

static WATCH_LIST: OnceLock<Mutex<HashMap<String, WatchEntry>>> = OnceLock::new();

fn watches() -> &'static Mutex<HashMap<String, WatchEntry>> {
    WATCH_LIST.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn recover(store: &Store) -> Result<(), String> {
    store
        .connect()?
        .execute(
            "UPDATE delegated_tasks
             SET status='interrupted_unknown',
                 error=COALESCE(error,'应用重启，委派子任务中断。'),
                 finished_at=COALESCE(finished_at,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             WHERE status IN ('queued','launching','running')",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn has_active(store: &Store, parent_conversation_id: &str) -> Result<bool, String> {
    store
        .connect()?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM delegated_tasks WHERE parent_conversation_id=?1 AND status IN ('queued','launching','running'))",
            [parent_conversation_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

fn worker_turn_diffs(connection: &rusqlite::Connection, worker_cid: &str) -> Vec<Value> {
    let Ok(mut stmt) = connection.prepare(
        "SELECT item_id, text FROM codex_messages WHERE conversation_id=?1 AND role='turn_diff' ORDER BY sequence ASC",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([worker_cid], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }) else {
        return Vec::new();
    };
    let mut diffs = Vec::new();
    for (id, text) in rows.flatten() {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&text) {
            parsed["turn_id"] = json!(id);
            diffs.push(parsed);
        }
    }
    diffs
}

pub fn list_for_conversation(
    store: &Store,
    parent_conversation_id: &str,
) -> Result<Vec<DelegatedTask>, String> {
    let connection = store.connect()?;
    let mut stmt = connection
        .prepare(
            "SELECT id, parent_conversation_id, parent_message_id, lead_agent, lead_model,
                    worker_agent, worker_model, worker_conversation_id, instruction,
                    return_to_sender, status, hop_count, summary, error, created_at, finished_at
             FROM delegated_tasks
             WHERE parent_conversation_id=?1
             ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([parent_conversation_id], |r| {
            let worker_cid: String = r.get(7)?;
            Ok(DelegatedTask {
                id: r.get(0)?,
                parent_conversation_id: r.get(1)?,
                parent_message_id: r.get(2)?,
                lead_agent: r.get(3)?,
                lead_model: r.get(4)?,
                worker_agent: r.get(5)?,
                worker_model: r.get(6)?,
                worker_conversation_id: worker_cid,
                instruction: r.get(8)?,
                return_to_sender: r.get::<_, i64>(9)? != 0,
                status: r.get(10)?,
                hop_count: r.get::<_, u32>(11)?,
                summary: r.get(12)?,
                error: r.get(13)?,
                created_at: r.get(14)?,
                finished_at: r.get(15)?,
                changed_files_count: 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut list = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    for item in &mut list {
        let diffs = worker_turn_diffs(&connection, &item.worker_conversation_id);
        item.changed_files_count = diffs
            .iter()
            .filter_map(|d| d.get("files").and_then(Value::as_array))
            .map(|arr| arr.len())
            .sum();
    }
    Ok(list)
}

pub fn task_detail(store: &Store, task_id: &str) -> Result<DelegatedTaskDetail, String> {
    let connection = store.connect()?;
    let mut task = connection
        .query_row(
            "SELECT id, parent_conversation_id, parent_message_id, lead_agent, lead_model,
                    worker_agent, worker_model, worker_conversation_id, instruction,
                    return_to_sender, status, hop_count, summary, error, created_at, finished_at
             FROM delegated_tasks
             WHERE id=?1",
            [task_id],
            |r| {
                Ok(DelegatedTask {
                    id: r.get(0)?,
                    parent_conversation_id: r.get(1)?,
                    parent_message_id: r.get(2)?,
                    lead_agent: r.get(3)?,
                    lead_model: r.get(4)?,
                    worker_agent: r.get(5)?,
                    worker_model: r.get(6)?,
                    worker_conversation_id: r.get(7)?,
                    instruction: r.get(8)?,
                    return_to_sender: r.get::<_, i64>(9)? != 0,
                    status: r.get(10)?,
                    hop_count: r.get::<_, u32>(11)?,
                    summary: r.get(12)?,
                    error: r.get(13)?,
                    created_at: r.get(14)?,
                    finished_at: r.get(15)?,
                    changed_files_count: 0,
                })
            },
        )
        .map_err(|_| "未找到指定的委派子任务。".to_string())?;

    let mut stmt = connection
        .prepare(
            "SELECT item_id, role, text, created_at
             FROM codex_messages
             WHERE conversation_id=?1 AND role IN ('assistant','tool','thinking')
             ORDER BY sequence ASC",
        )
        .map_err(|e| e.to_string())?;
    let messages = stmt
        .query_map([&task.worker_conversation_id], |r| {
            Ok(DelegatedTaskMessage {
                id: r.get(0)?,
                role: r.get(1)?,
                text: r.get(2)?,
                created_at: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let turns = worker_turn_diffs(&connection, &task.worker_conversation_id);
    task.changed_files_count = turns
        .iter()
        .filter_map(|d| d.get("files").and_then(Value::as_array))
        .map(|arr| arr.len())
        .sum();

    Ok(DelegatedTaskDetail {
        task,
        messages,
        turn_diffs: json!(turns),
    })
}

pub fn active_worker_approvals(
    app: &tauri::AppHandle,
    store: &Store,
    parent_conversation_id: &str,
) -> Result<Vec<Value>, String> {
    let connection = store.connect()?;
    let mut stmt = connection
        .prepare(
            "SELECT id, worker_agent, worker_conversation_id
             FROM delegated_tasks
             WHERE parent_conversation_id=?1 AND status IN ('launching','running')",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([parent_conversation_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut approvals = Vec::new();
    for (task_id, worker_agent, worker_cid) in rows {
        let Ok(kind) = agents::provider(store, &worker_agent) else {
            continue;
        };
        let Ok(adapter) = providers::resolve(&kind) else {
            continue;
        };
        if !adapter.supports_approval() {
            continue;
        }
        if let Ok(items) = adapter.approvals(app, store, &worker_cid) {
            for mut item in items {
                item["agent_id"] = json!(worker_agent);
                item["delegated_task_id"] = json!(task_id);
                item["is_delegated"] = json!(true);
                approvals.push(item);
            }
        }
    }
    Ok(approvals)
}

pub fn cancel_all(
    app: &tauri::AppHandle,
    store: &Store,
    parent_conversation_id: &str,
) -> Result<(), String> {
    {
        if let Ok(mut guard) = watches().lock() {
            guard.remove(parent_conversation_id);
        }
    }
    let connection = store.connect()?;
    let mut stmt = connection
        .prepare(
            "SELECT id, worker_agent, worker_conversation_id
             FROM delegated_tasks
             WHERE parent_conversation_id=?1 AND status IN ('queued','launching','running')",
        )
        .map_err(|e| e.to_string())?;
    let active = stmt
        .query_map([parent_conversation_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    for (task_id, worker_agent, worker_cid) in active {
        if let Ok(kind) = agents::provider(store, &worker_agent) {
            if let Ok(adapter) = providers::resolve(&kind) {
                let _ = adapter.cancel(app, store, &worker_cid);
            }
        }
        let _ = connection.execute(
            "UPDATE delegated_tasks
             SET status='cancelled',
                 error=COALESCE(error,'用户停止任务，已取消委派。'),
                 finished_at=COALESCE(finished_at,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             WHERE id=?1 AND status IN ('queued','launching','running')",
            [&task_id],
        );
    }
    crate::events::emit_conversation_sync(parent_conversation_id);
    Ok(())
}

pub fn prompt_context(
    store: &Store,
    _conversation_id: &str,
    lead_agent: &str,
) -> Result<Option<String>, String> {
    let available: Vec<agents::Agent> = agents::list(store)?
        .into_iter()
        .filter(|a| {
            a.id != lead_agent
                && a.enabled
                && a.registration_state == "active"
                && a.availability != "unavailable"
        })
        .collect();
    if available.is_empty() {
        return Ok(None);
    }
    let roster = available
        .iter()
        .map(|a| format!("{} (@{})", a.display_name, a.id))
        .collect::<Vec<_>>()
        .join("、");
    Ok(Some(format!(
        "【按需委派能力（仅在需要时使用）】\n\
         本机当前可协作的其他 Agent 成员：{roster}。\n\
         当用户明确要求你让其他 Agent 协助、实现子模块、排查问题或只读交叉复核，或者你判断某个独立子任务由其他成员处理更合适时，请在回复末尾输出：\n\
         ```agentcrew-dispatch\n\
         {{\"to\":\"成员id\",\"instruction\":\"精确、自包含的任务指令与相关文件路径\",\"return_to_sender\":true}}\n\
         ```\n\
         规则：\n\
         1. 子 Agent 运行在隔离上下文中，不会看到本会话完整历史，请把目标、约束与相关文件路径写清楚。\n\
         2. 对方完成后系统会自动将结果与工具记录回传给你，由你统一验收并向用户汇报。\n\
         3. 若你能独立完成且用户未要求委派，请直接回复用户，不要输出派发块。"
    )))
}

pub fn watch(store: &Store, conversation_id: &str, lead_agent: &str, lead_model: &str) {
    let seq: i64 = store
        .connect()
        .ok()
        .and_then(|c| {
            c.query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM codex_messages WHERE conversation_id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .ok()
        })
        .unwrap_or(0);
    if let Ok(mut guard) = watches().lock() {
        guard.insert(
            conversation_id.to_owned(),
            WatchEntry {
                conversation_id: conversation_id.to_owned(),
                lead_agent: lead_agent.to_owned(),
                lead_model: lead_model.to_owned(),
                last_checked_sequence: seq,
                hop_count: 0,
            },
        );
    }
}

fn watch_next_hop(
    conversation_id: &str,
    lead_agent: &str,
    lead_model: &str,
    last_checked_sequence: i64,
    hop_count: u32,
) {
    if let Ok(mut guard) = watches().lock() {
        guard.insert(
            conversation_id.to_owned(),
            WatchEntry {
                conversation_id: conversation_id.to_owned(),
                lead_agent: lead_agent.to_owned(),
                lead_model: lead_model.to_owned(),
                last_checked_sequence,
                hop_count,
            },
        );
    }
}

fn resolve_target_agent(store: &Store, lead_agent: &str, raw_to: &str) -> Option<String> {
    let trimmed = raw_to.trim().trim_start_matches('@');
    if trimmed.is_empty() {
        return None;
    }
    let list = agents::list(store).ok()?;
    list.into_iter()
        .find(|a| {
            a.id != lead_agent
                && a.enabled
                && a.registration_state == "active"
                && (a.id.eq_ignore_ascii_case(trimmed)
                    || a.display_name.eq_ignore_ascii_case(trimmed))
        })
        .map(|a| a.id)
}

fn resolve_worker_model(
    app: &tauri::AppHandle,
    store: &Store,
    worker_agent: &str,
    lead_model: &str,
) -> Result<String, String> {
    if let Ok(Some(def)) = crate::model_defaults::resolve_for(store, "private", worker_agent) {
        if !def.model.trim().is_empty() {
            return Ok(def.model);
        }
    }
    let kind = agents::provider(store, worker_agent)?;
    if let Ok(table) = agents::session_table(&kind) {
        if let Ok(Some(recent)) = store.connect()?.query_row(
            &format!("SELECT model FROM {table} WHERE model!='' ORDER BY updated_at DESC LIMIT 1"),
            [],
            |r| r.get::<_, String>(0),
        ).optional() {
            if !recent.trim().is_empty() {
                return Ok(recent);
            }
        }
    }
    if let Ok(adapter) = providers::resolve(&kind) {
        if let Ok(probe) = adapter.probe(app, store) {
            if let Some(models) = probe.get("models").and_then(Value::as_array) {
                if let Some(def) = models.iter().find(|m| m.get("isDefault").and_then(Value::as_bool) == Some(true)) {
                    if let Some(name) = def.get("model").and_then(Value::as_str) {
                        return Ok(name.to_owned());
                    }
                }
                if let Some(first) = models.first().and_then(|m| m.get("model")).and_then(Value::as_str) {
                    return Ok(first.to_owned());
                }
            }
        }
    }
    if !lead_model.trim().is_empty() && worker_agent == "codex" {
        return Ok(lead_model.to_owned());
    }
    Err(format!("{worker_agent} 尚未配置可用模型，无法委派。"))
}

fn ensure_worker_conversation(
    store: &Store,
    parent_conversation_id: &str,
    worker_agent: &str,
    worker_conversation_id: &str,
) -> Result<(), String> {
    let root = store.conversation_root(parent_conversation_id)?;
    let connection = store.connect()?;
    let project_id: Option<String> = connection
        .query_row(
            "SELECT project_id FROM conversations WHERE id=?1",
            [parent_conversation_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    connection
        .execute(
            "INSERT OR IGNORE INTO conversations(id, project_id, workspace_path) VALUES(?1, ?2, ?3)",
            params![worker_conversation_id, project_id, root],
        )
        .map_err(|e| e.to_string())?;
    connection
        .execute(
            "INSERT OR IGNORE INTO agent_sessions(agent_id, conversation_id, runtime_conversation_id) VALUES(?1, ?2, ?2)",
            params![worker_agent, worker_conversation_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn worker_tool_evidence(store: &Store, worker_conversation_id: &str) -> String {
    let Ok(connection) = store.connect() else {
        return String::new();
    };
    let Ok(mut stmt) = connection.prepare(
        "SELECT item_id, text FROM codex_messages WHERE conversation_id=?1 AND role='tool' ORDER BY sequence ASC",
    ) else {
        return String::new();
    };
    let Ok(rows) = stmt.query_map([worker_conversation_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }) else {
        return String::new();
    };
    let mut out = String::new();
    for row in rows.flatten() {
        let excerpt: String = row.1.chars().take(2000).collect();
        out.push_str(&format!("\n工具 {0}：\n{excerpt}\n", row.0));
        if out.len() > 8000 {
            break;
        }
    }
    out
}

pub fn tick(app: &tauri::AppHandle) {
    let store = app.state::<Store>();

    // Step A: check watched single-chat conversations for completed lead turns containing dispatch blocks.
    let entries: Vec<WatchEntry> = watches()
        .lock()
        .map(|g| g.values().cloned().collect())
        .unwrap_or_default();

    for entry in entries {
        let Ok(status) = chat_queue::session_status(app, &store, &entry.lead_agent, &entry.conversation_id) else {
            continue;
        };
        if status.as_deref().is_some_and(crate::codex::active) {
            continue;
        }
        if let Ok(mut guard) = watches().lock() {
            guard.remove(&entry.conversation_id);
        }
        if matches!(
            status.as_deref(),
            Some("cancelled" | "interrupted" | "failed")
        ) {
            continue;
        }

        let Ok(connection) = store.connect() else {
            continue;
        };
        let response: Option<(String, String, i64)> = connection
            .query_row(
                "SELECT item_id, text, sequence
                 FROM codex_messages
                 WHERE conversation_id=?1 AND agent=?2 AND role='assistant' AND sequence>?3
                 ORDER BY sequence DESC LIMIT 1",
                params![entry.conversation_id, entry.lead_agent, entry.last_checked_sequence],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .ok()
            .flatten();
        let Some((msg_id, text, _seq)) = response else {
            continue;
        };
        let raw_blocks = parse_dispatch_blocks(&text);
        if raw_blocks.is_empty() {
            continue;
        }
        let valid: Vec<_> = raw_blocks
            .into_iter()
            .filter_map(|req| {
                resolve_target_agent(&store, &entry.lead_agent, &req.to).map(|target| (target, req))
            })
            .collect();
        if valid.is_empty() {
            continue;
        }
        if entry.hop_count >= MAX_DELEGATION_HOPS {
            let _ = connection.execute(
                "INSERT INTO codex_messages(conversation_id, item_id, role, text) VALUES(?1, ?2, 'system', ?3)",
                params![
                    entry.conversation_id,
                    format!("delegation-limit-{}", uuid::Uuid::new_v4()),
                    format!("按需委派已达 {MAX_DELEGATION_HOPS} 轮上限，已停止自动委派。")
                ],
            );
            crate::events::emit_conversation_sync(&entry.conversation_id);
            continue;
        }

        for (worker_agent, req) in valid {
            let task_id = format!("del-{}", uuid::Uuid::new_v4());
            let worker_cid = format!("delegate-{}", uuid::Uuid::new_v4());
            let worker_model = resolve_worker_model(app, &store, &worker_agent, &entry.lead_model)
                .unwrap_or_default();
            let _ = connection.execute(
                "INSERT INTO delegated_tasks(
                    id, parent_conversation_id, parent_message_id, lead_agent, lead_model,
                    worker_agent, worker_model, worker_conversation_id, instruction,
                    return_to_sender, status, hop_count
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'queued',?11)",
                params![
                    task_id,
                    entry.conversation_id,
                    msg_id,
                    entry.lead_agent,
                    entry.lead_model,
                    worker_agent,
                    worker_model,
                    worker_cid,
                    req.instruction,
                    if req.return_to_sender { 1 } else { 0 },
                    entry.hop_count + 1,
                ],
            );
        }
        crate::events::emit_conversation_sync(&entry.conversation_id);
    }

    // Step B: launch queued delegated tasks.
    let queued: Vec<(String, String, String, String, String, String, String)> = store
        .connect()
        .ok()
        .and_then(|c| {
            let mut stmt = c
                .prepare(
                    "SELECT id, parent_conversation_id, lead_agent, worker_agent, worker_model, worker_conversation_id, instruction
                     FROM delegated_tasks WHERE status='queued' ORDER BY created_at ASC",
                )
                .ok()?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                })
                .ok()?;
            rows.collect::<Result<Vec<_>, _>>().ok()
        })
        .unwrap_or_default();

    for (task_id, parent_cid, lead_agent, worker_agent, worker_model, worker_cid, instruction) in queued {
        if worker_model.trim().is_empty() {
            if let Ok(c) = store.connect() {
                let _ = c.execute(
                    "UPDATE delegated_tasks SET status='failed', error='未找到可用的模型配置', finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                    [&task_id],
                );
            }
            crate::events::emit_conversation_sync(&parent_cid);
            continue;
        }
        let kind = match agents::ensure_enabled(&store, &worker_agent) {
            Ok(k) => k,
            Err(err) => {
                if let Ok(c) = store.connect() {
                    let _ = c.execute(
                        "UPDATE delegated_tasks SET status='failed', error=?2, finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                        params![task_id, err],
                    );
                }
                crate::events::emit_conversation_sync(&parent_cid);
                continue;
            }
        };
        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = if kind == "deepseek" {
            let Ok(g) = gate.0.try_lock() else {
                continue;
            };
            Some(g)
        } else {
            None
        };
        if let Err(err) = ensure_worker_conversation(&store, &parent_cid, &worker_agent, &worker_cid) {
            if let Ok(c) = store.connect() {
                let _ = c.execute(
                    "UPDATE delegated_tasks SET status='failed', error=?2, finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                    params![task_id, err],
                );
            }
            crate::events::emit_conversation_sync(&parent_cid);
            continue;
        }
        if let Ok(c) = store.connect() {
            let _ = c.execute(
                "UPDATE delegated_tasks SET status='running' WHERE id=?1 AND status='queued'",
                [&task_id],
            );
        }
        let worker_prompt = format!(
            "来自主负责人 {lead_agent} 的按需委派子任务。\n\
             请专注于以下精确指令并直接给出完整结果（若有代码修改请落实到项目文件；若为只读复核请给出明确依据与发现）。完成后系统会自动回传给 {lead_agent}。\n\n\
             {instruction}"
        );
        let send_res = providers::resolve(&kind).and_then(|adapter| {
            adapter.send_with_context(app, &store, &worker_cid, &worker_model, &worker_prompt, "user", None)
        });
        if let Err(err) = send_res {
            if let Ok(c) = store.connect() {
                let _ = c.execute(
                    "UPDATE delegated_tasks SET status='failed', error=?2, finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                    params![task_id, err],
                );
            }
        }
        crate::events::emit_conversation_sync(&parent_cid);
    }

    // Step C: poll running delegated tasks.
    let running: Vec<(String, String, String, String)> = store
        .connect()
        .ok()
        .and_then(|c| {
            let mut stmt = c
                .prepare(
                    "SELECT id, parent_conversation_id, worker_agent, worker_conversation_id
                     FROM delegated_tasks WHERE status='running'",
                )
                .ok()?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .ok()?;
            rows.collect::<Result<Vec<_>, _>>().ok()
        })
        .unwrap_or_default();

    for (task_id, parent_cid, worker_agent, worker_cid) in running {
        let Ok(kind) = agents::provider(&store, &worker_agent) else {
            continue;
        };
        let Ok(adapter) = providers::resolve(&kind) else {
            continue;
        };
        let Ok(session) = adapter.session(app, &store, &worker_cid) else {
            continue;
        };
        let status = session["status"].as_str().unwrap_or("");
        if crate::codex::active(status) {
            continue;
        }
        let Ok(c) = store.connect() else {
            continue;
        };
        if matches!(status, "cancelled" | "interrupted" | "failed") {
            let err_msg = session["error"].as_str().map(str::to_owned);
            let final_status = if status == "cancelled" { "cancelled" } else { "failed" };
            let _ = c.execute(
                "UPDATE delegated_tasks
                 SET status=?2, error=COALESCE(error,?3), finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE id=?1",
                params![task_id, final_status, err_msg],
            );
        } else {
            let summary: Option<String> = c
                .query_row(
                    "SELECT text FROM codex_messages WHERE conversation_id=?1 AND role='assistant' ORDER BY sequence DESC LIMIT 1",
                    [&worker_cid],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten();
            let _ = c.execute(
                "UPDATE delegated_tasks
                 SET status='completed', summary=?2, finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE id=?1",
                params![task_id, summary.unwrap_or_default()],
            );
        }
        crate::events::emit_conversation_sync(&parent_cid);
    }

    // Step D: deliver receipts back to lead agent once all tasks for a parent_message_id finish.
    let pending_batches: Vec<(String, Option<String>)> = store
        .connect()
        .ok()
        .and_then(|c| {
            let mut stmt = c
                .prepare(
                    "SELECT DISTINCT parent_conversation_id, parent_message_id
                     FROM delegated_tasks
                     WHERE return_to_sender=1 AND receipt_dispatched=0 AND status IN ('completed','failed','cancelled')",
                )
                .ok()?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .ok()?;
            rows.collect::<Result<Vec<_>, _>>().ok()
        })
        .unwrap_or_default();

    for (parent_cid, parent_msg_id) in pending_batches {
        let Ok(c) = store.connect() else {
            continue;
        };
        let still_running: bool = c
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM delegated_tasks
                    WHERE parent_conversation_id=?1
                      AND COALESCE(parent_message_id,'')=COALESCE(?2,'')
                      AND status IN ('queued','launching','running')
                )",
                params![parent_cid, parent_msg_id],
                |r| r.get(0),
            )
            .unwrap_or(true);
        if still_running {
            continue;
        }
        let mut stmt = match c.prepare(
            "SELECT id, lead_agent, lead_model, worker_agent, worker_conversation_id, instruction, status, summary, error, hop_count
             FROM delegated_tasks
             WHERE parent_conversation_id=?1
               AND COALESCE(parent_message_id,'')=COALESCE(?2,'')
               AND return_to_sender=1
               AND receipt_dispatched=0
             ORDER BY created_at ASC",
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let items: Vec<(String, String, String, String, String, String, String, Option<String>, Option<String>, u32)> = stmt
            .query_map(params![parent_cid, parent_msg_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            })
            .ok()
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>().ok())
            .unwrap_or_default();
        drop(stmt);
        if items.is_empty() {
            continue;
        }
        let lead_agent = items[0].1.clone();
        let lead_model = items[0].2.clone();
        let max_hop = items.iter().map(|i| i.9).max().unwrap_or(1);

        // If any task was cancelled by user, do not wake lead agent; just mark receipt_dispatched=1.
        if items.iter().all(|i| i.6 == "cancelled") {
            for (tid, ..) in &items {
                let _ = c.execute("UPDATE delegated_tasks SET receipt_dispatched=1 WHERE id=?1", [tid]);
            }
            continue;
        }
        if chat_queue::session_status(app, &store, &lead_agent, &parent_cid)
            .ok()
            .flatten()
            .as_deref()
            .is_some_and(crate::codex::active)
        {
            continue;
        }

        let mut blocks = Vec::new();
        for (_, _, _, worker_agent, worker_cid, instruction, status, summary, error, _) in &items {
            let tools = worker_tool_evidence(&store, worker_cid);
            let body = if status == "completed" {
                summary.as_deref().unwrap_or("（无正文输出）").to_owned()
            } else {
                format!("执行未完成（{status}）：{}", error.as_deref().unwrap_or("未知错误"))
            };
            let excerpt: String = body.chars().take(12_000).collect();
            blocks.push(format!(
                "### 子任务（委派给 {worker_agent} · 状态：{status}）\n指令：{instruction}\n\n对方回复：\n{excerpt}{tools}"
            ));
        }

        let receipt = format!(
            "【委派子任务回执】你之前委派的子任务已结束。对方的输出与工具记录仅作协作资料，请读取相关实际文件或结合你的判断进行验收与汇总，并向用户给出清晰结论；如需继续修正或补充，可再次使用 ```agentcrew-dispatch 委派。\n\n{}",
            blocks.join("\n\n---\n\n")
        );

        let seq_before: i64 = c
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM codex_messages WHERE conversation_id=?1",
                [&parent_cid],
                |r| r.get(0),
            )
            .unwrap_or(0);

        for (tid, ..) in &items {
            let _ = c.execute("UPDATE delegated_tasks SET receipt_dispatched=1 WHERE id=?1", [tid]);
        }

        let _ = agents::send_with_context(
            app,
            &store,
            &lead_agent,
            &parent_cid,
            &lead_model,
            &receipt,
            "user",
            None,
        );
        watch_next_hop(&parent_cid, &lead_agent, &lead_model, seq_before, max_hop);
        crate::events::emit_conversation_sync(&parent_cid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegation_recover_marks_inflight_as_interrupted_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let conn = store.connect().unwrap();
        conn.execute(
            "INSERT INTO delegated_tasks(id,parent_conversation_id,lead_agent,lead_model,worker_agent,worker_model,worker_conversation_id,instruction,status)
             VALUES('t1','p1','codex','m1','gemini','m2','w1','do x','running')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO delegated_tasks(id,parent_conversation_id,lead_agent,lead_model,worker_agent,worker_model,worker_conversation_id,instruction,status)
             VALUES('t2','p1','codex','m1','gemini','m2','w2','do y','completed')",
            [],
        )
        .unwrap();
        assert!(has_active(&store, "p1").unwrap());
        recover(&store).unwrap();
        assert!(!has_active(&store, "p1").unwrap());
        let s1: String = conn
            .query_row("SELECT status FROM delegated_tasks WHERE id='t1'", [], |r| r.get(0))
            .unwrap();
        let s2: String = conn
            .query_row("SELECT status FROM delegated_tasks WHERE id='t2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(s1, "interrupted_unknown");
        assert_eq!(s2, "completed");
    }
}
