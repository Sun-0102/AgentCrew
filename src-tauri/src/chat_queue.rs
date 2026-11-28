use crate::{agents, providers, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
pub struct QueuedMessage {
    pub id: String,
    pub conversation_id: String,
    pub agent_id: String,
    pub model: String,
    pub text: String,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub status: String,
    pub created_at: String,
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedMessage> {
    Ok(QueuedMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        agent_id: row.get(2)?,
        model: row.get(3)?,
        text: row.get(4)?,
        reasoning_effort: row.get(5)?,
        service_tier: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
    })
}

/// On cold app startup, preserve all queued items in `paused` state so they
/// survive restarts without unexpectedly firing stale prompts on boot.
pub fn recover(store: &Store) -> Result<(), String> {
    store
        .connect()?
        .execute(
            "UPDATE single_chat_queue SET status='paused' WHERE status='queued'",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list(store: &Store, conversation_id: &str) -> Result<Vec<QueuedMessage>, String> {
    let connection = store.connect()?;
    let mut statement = connection
        .prepare(
            "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
             FROM single_chat_queue
             WHERE conversation_id=?1
             ORDER BY rowid ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([conversation_id], row_to_item)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn enqueue(
    store: &Store,
    conversation_id: &str,
    agent_id: &str,
    model: &str,
    text: &str,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
) -> Result<QueuedMessage, String> {
    if text.trim().is_empty() || text.len() > 100_000 {
        return Err("消息不能为空且不能超过 100000 字节。".into());
    }
    let id = format!("queue-{}", uuid::Uuid::new_v4());
    let connection = store.connect()?;
    connection
        .execute(
            "INSERT INTO single_chat_queue(id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'queued')",
            params![
                id,
                conversation_id,
                agent_id,
                model,
                text,
                reasoning_effort,
                service_tier
            ],
        )
        .map_err(|e| e.to_string())?;
    let item = connection
        .query_row(
            "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
             FROM single_chat_queue WHERE id=?1",
            [&id],
            row_to_item,
        )
        .map_err(|e| e.to_string())?;
    crate::events::emit_conversation_sync(conversation_id);
    Ok(item)
}

pub fn update_text(
    store: &Store,
    conversation_id: &str,
    id: &str,
    text: &str,
) -> Result<QueuedMessage, String> {
    if text.trim().is_empty() || text.len() > 100_000 {
        return Err("消息不能为空且不能超过 100000 字节。".into());
    }
    let connection = store.connect()?;
    let changed = connection
        .execute(
            "UPDATE single_chat_queue SET text=?3 WHERE id=?1 AND conversation_id=?2",
            params![id, conversation_id, text],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("排队消息不存在或已开始执行。".into());
    }
    let item = connection
        .query_row(
            "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
             FROM single_chat_queue WHERE id=?1",
            [id],
            row_to_item,
        )
        .map_err(|e| e.to_string())?;
    crate::events::emit_conversation_sync(conversation_id);
    Ok(item)
}

pub fn remove(store: &Store, conversation_id: &str, id: &str) -> Result<(), String> {
    store
        .connect()?
        .execute(
            "DELETE FROM single_chat_queue WHERE id=?1 AND conversation_id=?2",
            params![id, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    crate::events::emit_conversation_sync(conversation_id);
    Ok(())
}

pub fn clear(store: &Store, conversation_id: &str) -> Result<(), String> {
    store
        .connect()?
        .execute(
            "DELETE FROM single_chat_queue WHERE conversation_id=?1",
            [conversation_id],
        )
        .map_err(|e| e.to_string())?;
    crate::events::emit_conversation_sync(conversation_id);
    Ok(())
}

pub fn pause_all(store: &Store, conversation_id: &str) -> Result<(), String> {
    let changed = store
        .connect()?
        .execute(
            "UPDATE single_chat_queue SET status='paused' WHERE conversation_id=?1 AND status='queued'",
            [conversation_id],
        )
        .map_err(|e| e.to_string())?;
    if changed > 0 {
        crate::events::emit_conversation_sync(conversation_id);
    }
    Ok(())
}

pub fn resume_all(store: &Store, conversation_id: &str) -> Result<(), String> {
    let changed = store
        .connect()?
        .execute(
            "UPDATE single_chat_queue SET status='queued' WHERE conversation_id=?1 AND status='paused'",
            [conversation_id],
        )
        .map_err(|e| e.to_string())?;
    if changed > 0 {
        crate::events::emit_conversation_sync(conversation_id);
    }
    Ok(())
}

/// Resume paused items and, if the conversation is currently idle (including after
/// a previous cancel/error), immediately dispatch the oldest queued item.
pub fn resume_and_dispatch(
    app: &tauri::AppHandle,
    store: &Store,
    conversation_id: &str,
) -> Result<Value, String> {
    resume_all(store, conversation_id)?;
    let first: Option<QueuedMessage> = store
        .connect()?
        .query_row(
            "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
             FROM single_chat_queue
             WHERE conversation_id=?1 AND status='queued'
             ORDER BY rowid ASC LIMIT 1",
            [conversation_id],
            row_to_item,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(item) = first else {
        return Ok(json!({"resumed": true}));
    };
    let status = session_status(app, store, &item.agent_id, conversation_id)?;
    if status.as_deref().is_some_and(crate::codex::active) {
        return Ok(json!({"resumed": true, "dispatched": false}));
    }
    send_now(app, store, conversation_id, &item.id, false)
}

pub fn session_status(
    app: &tauri::AppHandle,
    store: &Store,
    agent_id: &str,
    conversation_id: &str,
) -> Result<Option<String>, String> {
    let session = agents::session(app, store, agent_id, conversation_id)?;
    Ok(session["status"].as_str().map(str::to_owned))
}

fn dispatch_item(app: &tauri::AppHandle, store: &Store, item: &QueuedMessage) -> Result<Value, String> {
    let kind = agents::ensure_enabled(store, &item.agent_id)?;
    let runtime = agents::binding(store, &item.agent_id, &item.conversation_id)?;
    crate::reasoning::save(
        store,
        &runtime,
        &kind,
        &item.model,
        item.reasoning_effort.as_deref(),
    )?;
    if kind == "codex" {
        crate::speed::save(store, &runtime, &item.model, item.service_tier.as_deref())?;
    }
    agents::send(
        app,
        store,
        &item.agent_id,
        &item.conversation_id,
        &item.model,
        &item.text,
        "user",
    )
}

fn restore_paused(store: &Store, item: &QueuedMessage) {
    if let Ok(connection) = store.connect() {
        let _ = connection.execute(
            "INSERT OR IGNORE INTO single_chat_queue(id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'paused',?8)",
            params![
                item.id,
                item.conversation_id,
                item.agent_id,
                item.model,
                item.text,
                item.reasoning_effort,
                item.service_tier,
                item.created_at
            ],
        );
        let _ = connection.execute(
            "UPDATE single_chat_queue SET status='paused' WHERE conversation_id=?1 AND status='queued'",
            [&item.conversation_id],
        );
    }
}

/// Immediately send a queued item:
/// - If the conversation is running and `prefer_steer` is true on a provider that supports
///   native steer (e.g. Codex `turn/steer`), inject directly into the active turn.
/// - If the conversation is active otherwise, cancel the current turn and immediately send.
/// - If idle, pop and send immediately.
pub fn send_now(
    app: &tauri::AppHandle,
    store: &Store,
    conversation_id: &str,
    id: &str,
    prefer_steer: bool,
) -> Result<Value, String> {
    let item: QueuedMessage = store
        .connect()?
        .query_row(
            "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
             FROM single_chat_queue WHERE id=?1 AND conversation_id=?2",
            params![id, conversation_id],
            row_to_item,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("排队消息不存在或已开始执行。")?;

    let kind = agents::ensure_enabled(store, &item.agent_id)?;
    let adapter = providers::resolve(&kind)?;
    let status = session_status(app, store, &item.agent_id, conversation_id)?;
    let is_active = status.as_deref().is_some_and(crate::codex::active);

    if is_active && prefer_steer && adapter.supports_steer() && status.as_deref() == Some("running") {
        let runtime = agents::binding(store, &item.agent_id, conversation_id)?;
        let res = adapter.steer(app, store, &runtime, &item.text)?;
        remove(store, conversation_id, id)?;
        return Ok(res);
    }

    if is_active {
        let _ = agents::cancel(app, store, &item.agent_id, conversation_id);
    }

    let gate = app.state::<crate::deepseek::ExecutionGate>();
    let _guard = if kind == "deepseek" {
        Some(gate.0.lock().map_err(|e| e.to_string())?)
    } else {
        None
    };

    let deleted = store
        .connect()?
        .execute(
            "DELETE FROM single_chat_queue WHERE id=?1 AND conversation_id=?2",
            params![id, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    if deleted == 0 {
        return Err("排队消息不存在或已开始执行。".into());
    }

    match dispatch_item(app, store, &item) {
        Ok(res) => {
            crate::events::emit_conversation_sync(conversation_id);
            Ok(res)
        }
        Err(error) => {
            restore_paused(store, &item);
            crate::events::emit_conversation_sync(conversation_id);
            Err(error)
        }
    }
}

/// Called periodically from the background scheduler to auto-dispatch queued
/// single-chat messages once the current turn finishes cleanly.
pub fn tick(app: &tauri::AppHandle) {
    let store = app.state::<Store>();
    let paused: bool = store
        .connect()
        .ok()
        .and_then(|c| {
            c.query_row("SELECT paused FROM app_controls WHERE id=1", [], |r| {
                r.get(0)
            })
            .ok()
        })
        .unwrap_or(false);
    if paused {
        return;
    }

    let conversations: Vec<String> = (|| -> Result<Vec<String>, String> {
        let connection = store.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT conversation_id FROM single_chat_queue WHERE status='queued' ORDER BY rowid ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    })()
    .unwrap_or_default();

    for conversation_id in conversations {
        let next: Option<QueuedMessage> = store
            .connect()
            .ok()
            .and_then(|c| {
                c.query_row(
                    "SELECT id,conversation_id,agent_id,model,text,reasoning_effort,service_tier,status,created_at
                     FROM single_chat_queue
                     WHERE conversation_id=?1 AND status='queued'
                     ORDER BY rowid ASC LIMIT 1",
                    [&conversation_id],
                    row_to_item,
                )
                .optional()
                .ok()
                .flatten()
            });
        let Some(item) = next else {
            continue;
        };

        let status = match session_status(app, &store, &item.agent_id, &conversation_id) {
            Ok(status) => status,
            Err(_) => continue,
        };

        if status.as_deref().is_some_and(crate::codex::active)
            || crate::delegation::has_active(&store, &conversation_id).unwrap_or(false)
        {
            continue;
        }

        // Safety guard: if the previous turn failed, was cancelled, or interrupted,
        // pause remaining queued items instead of blindly continuing.
        if matches!(
            status.as_deref(),
            Some("cancelled" | "interrupted" | "failed")
        ) {
            let _ = pause_all(&store, &conversation_id);
            continue;
        }

        let kind = match agents::provider(&store, &item.agent_id) {
            Ok(kind) => kind,
            Err(_) => {
                let _ = pause_all(&store, &conversation_id);
                continue;
            }
        };

        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = if kind == "deepseek" {
            let Ok(guard) = gate.0.try_lock() else {
                continue;
            };
            Some(guard)
        } else {
            None
        };

        let claimed = store
            .connect()
            .ok()
            .and_then(|c| {
                c.execute(
                    "DELETE FROM single_chat_queue WHERE id=?1 AND status='queued'",
                    [&item.id],
                )
                .ok()
            })
            .unwrap_or(0);
        if claimed == 0 {
            continue;
        }

        match dispatch_item(app, &store, &item) {
            Ok(_) => {
                crate::events::emit_conversation_sync(&conversation_id);
            }
            Err(_) => {
                restore_paused(&store, &item);
                crate::events::emit_conversation_sync(&conversation_id);
            }
        }
    }
}
