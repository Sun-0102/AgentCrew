use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use tauri::Emitter;

pub const STREAM_EVENT: &str = "agentcrew:stream";
pub const SYNC_EVENT: &str = "agentcrew:sync";

const STREAM_FLUSH_WINDOW: Duration = Duration::from_millis(30);
const MAX_PENDING_DELTAS: usize = 32;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StreamDelta {
    pub conversation_id: String,
    pub item_id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub mode: String, // "append" | "replace"
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamBatchEvent {
    pub seq: u64,
    pub deltas: Vec<StreamDelta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncEvent {
    pub seq: u64,
    pub scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

struct PendingState {
    deltas: Vec<StreamDelta>,
}

struct EventHub {
    app: tauri::AppHandle,
    seq: AtomicU64,
    pending: Mutex<PendingState>,
    wake: Condvar,
}

static HUB: OnceLock<Arc<EventHub>> = OnceLock::new();

pub fn init(app: tauri::AppHandle) {
    let hub = Arc::new(EventHub {
        app,
        seq: AtomicU64::new(1),
        pending: Mutex::new(PendingState { deltas: Vec::new() }),
        wake: Condvar::new(),
    });
    if HUB.set(hub.clone()).is_ok() {
        std::thread::spawn(move || {
            let mut guard = hub.pending.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                while guard.deltas.is_empty() {
                    guard = hub.wake.wait(guard).unwrap_or_else(|e| e.into_inner());
                }
                if guard.deltas.len() < MAX_PENDING_DELTAS {
                    let (next_guard, _) = hub
                        .wake
                        .wait_timeout(guard, STREAM_FLUSH_WINDOW)
                        .unwrap_or_else(|e| e.into_inner());
                    guard = next_guard;
                }
                if guard.deltas.is_empty() {
                    continue;
                }
                let batch = std::mem::take(&mut guard.deltas);
                drop(guard);
                hub.emit_batch(batch);
                guard = hub.pending.lock().unwrap_or_else(|e| e.into_inner());
            }
        });
    }
}

impl EventHub {
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    fn emit_batch(&self, deltas: Vec<StreamDelta>) {
        if deltas.is_empty() {
            return;
        }
        let payload = StreamBatchEvent {
            seq: self.next_seq(),
            deltas,
        };
        let _ = self.app.emit(STREAM_EVENT, payload);
    }

    fn flush_deltas(&self) {
        let batch = {
            let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if guard.deltas.is_empty() {
                return;
            }
            std::mem::take(&mut guard.deltas)
        };
        self.emit_batch(batch);
    }
}

pub(crate) fn coalesce_delta(buffer: &mut Vec<StreamDelta>, incoming: StreamDelta) {
    if incoming.text.is_empty() && incoming.mode == "append" {
        return;
    }
    if let Some(last) = buffer.last_mut() {
        if last.conversation_id == incoming.conversation_id
            && last.item_id == incoming.item_id
            && last.role == incoming.role
            && last.agent == incoming.agent
        {
            if incoming.mode == "append" && last.mode == "append" {
                last.text.push_str(&incoming.text);
                return;
            }
            if incoming.mode == "replace" {
                last.mode = "replace".into();
                last.text = incoming.text;
                return;
            }
        }
    }
    buffer.push(incoming);
}

pub fn emit_delta(
    conversation_id: impl Into<String>,
    item_id: impl Into<String>,
    role: impl Into<String>,
    agent: Option<&str>,
    mode: &str,
    text: impl Into<String>,
) {
    let Some(hub) = HUB.get() else {
        return;
    };
    let delta = StreamDelta {
        conversation_id: conversation_id.into(),
        item_id: item_id.into(),
        role: role.into(),
        agent: agent.map(|s| s.to_owned()),
        mode: mode.to_owned(),
        text: text.into(),
    };
    if delta.text.is_empty() && delta.mode == "append" {
        return;
    }
    let mut guard = hub.pending.lock().unwrap_or_else(|e| e.into_inner());
    let was_empty = guard.deltas.is_empty();
    coalesce_delta(&mut guard.deltas, delta);
    if was_empty || guard.deltas.len() >= MAX_PENDING_DELTAS {
        hub.wake.notify_one();
    }
}

pub fn emit_sync(
    scopes: &[&str],
    conversation_id: Option<&str>,
    group_id: Option<&str>,
    agent_id: Option<&str>,
) {
    let Some(hub) = HUB.get() else {
        return;
    };
    if scopes.is_empty() {
        return;
    }
    // Keep strict ordering: flush any buffered stream deltas before domain state transitions.
    hub.flush_deltas();
    let unique: BTreeSet<&str> = scopes.iter().copied().collect();
    let payload = SyncEvent {
        seq: hub.next_seq(),
        scopes: unique.into_iter().map(str::to_owned).collect(),
        conversation_id: conversation_id.map(str::to_owned),
        group_id: group_id.map(str::to_owned),
        agent_id: agent_id.map(str::to_owned),
    };
    let _ = hub.app.emit(SYNC_EVENT, payload);
}

pub fn emit_conversation_sync(conversation_id: &str) {
    emit_sync(
        &["snapshot", "groups", "group_sessions", "private_sessions"],
        Some(conversation_id),
        None,
        None,
    );
}

pub fn emit_agents_sync() {
    emit_sync(&["agents", "dispatch"], None, None, None);
}

pub fn emit_groups_sync(group_id: Option<&str>) {
    emit_sync(&["groups", "group_sessions"], None, group_id, None);
}

pub fn emit_private_sessions_sync(agent_id: Option<&str>) {
    emit_sync(&["private_sessions"], None, None, agent_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_adjacent_appends_for_same_item() {
        let mut buf = Vec::new();
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "m1".into(),
                role: "assistant".into(),
                agent: Some("codex".into()),
                mode: "append".into(),
                text: "你".into(),
            },
        );
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "m1".into(),
                role: "assistant".into(),
                agent: Some("codex".into()),
                mode: "append".into(),
                text: "好".into(),
            },
        );
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0].text, "你好");
    }

    #[test]
    fn replace_overrides_prior_buffered_append() {
        let mut buf = Vec::new();
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "m1".into(),
                role: "assistant".into(),
                agent: None,
                mode: "append".into(),
                text: "partial".into(),
            },
        );
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "m1".into(),
                role: "assistant".into(),
                agent: None,
                mode: "replace".into(),
                text: "final".into(),
            },
        );
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0].mode, "replace");
        assert_eq!(buf[0].text, "final");
    }

    #[test]
    fn keeps_distinct_items_in_order() {
        let mut buf = Vec::new();
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "r1".into(),
                role: "thinking".into(),
                agent: Some("gemini".into()),
                mode: "append".into(),
                text: "step 1".into(),
            },
        );
        coalesce_delta(
            &mut buf,
            StreamDelta {
                conversation_id: "c1".into(),
                item_id: "m1".into(),
                role: "assistant".into(),
                agent: Some("gemini".into()),
                mode: "append".into(),
                text: "answer".into(),
            },
        );
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0].role, "thinking");
        assert_eq!(buf[1].role, "assistant");
    }
}
