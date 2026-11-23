use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct PrivateSession {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    pub archived: bool,
    pub is_default: bool,
    pub created_at: String,
    pub preview: Option<String>,
    pub updated_at: Option<String>,
    pub status: Option<String>,
}

fn validate_name(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 40 {
        return Err("会话名称请填写 1–40 个字。".into());
    }
    Ok(value)
}

fn ensure_default_conversation(store: &Store, agent_id: &str) -> Result<String, String> {
    let default_id = crate::agents::private_conversation(store, agent_id, None)?;
    if default_id == crate::deepseek::PERSONAL {
        let root = store.chat_workspace_parent()?.to_string_lossy().into_owned();
        store
            .connect()?
            .execute(
                "INSERT OR IGNORE INTO conversations(id,workspace_path) VALUES(?1,?2)",
                params![default_id, root],
            )
            .map_err(|e| e.to_string())?;
    }
    let connection = store.connect()?;
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM private_sessions WHERE agent_id=?1 AND is_default=1)",
            [agent_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        connection
            .execute(
                "INSERT OR IGNORE INTO private_sessions(conversation_id,agent_id,name,archived,is_default) VALUES(?1,?2,'默认会话',0,1)",
                params![default_id, agent_id],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(default_id)
}

fn session_busy(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<bool, String> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
         OR EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
         OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",
        [id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

impl Store {
    pub fn private_sessions(&self, agent_id: &str) -> Result<Vec<PrivateSession>, String> {
        ensure_default_conversation(self, agent_id)?;
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT s.conversation_id, s.agent_id, s.name, s.archived, s.is_default, s.created_at,
                    CASE WHEN s.conversation_id = 'deepseek-personal'
                        THEN (SELECT text FROM deepseek_messages WHERE role IN ('user','assistant') ORDER BY sequence DESC LIMIT 1)
                        ELSE (SELECT text FROM codex_messages WHERE conversation_id=s.conversation_id AND role IN ('user','assistant') ORDER BY sequence DESC LIMIT 1)
                    END,
                    CASE WHEN s.conversation_id = 'deepseek-personal'
                        THEN (SELECT created_at FROM deepseek_messages ORDER BY sequence DESC LIMIT 1)
                        ELSE (SELECT created_at FROM codex_messages WHERE conversation_id=s.conversation_id ORDER BY sequence DESC LIMIT 1)
                    END,
                    COALESCE(
                        (SELECT status FROM codex_sessions WHERE conversation_id=s.conversation_id AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1),
                        (SELECT status FROM deepseek_sessions WHERE conversation_id=s.conversation_id AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1),
                        (SELECT g.status FROM gemini_sessions g JOIN agent_sessions a ON a.runtime_conversation_id=g.conversation_id WHERE a.conversation_id=s.conversation_id AND g.status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1)
                    ),
                    CASE WHEN s.conversation_id = 'deepseek-personal'
                        THEN COALESCE((SELECT MAX(sequence) FROM deepseek_messages), 0)
                        ELSE COALESCE((SELECT MAX(sequence) FROM codex_messages WHERE conversation_id=s.conversation_id), 0)
                    END AS last_seq
                 FROM private_sessions s
                 WHERE s.agent_id=?1
                 ORDER BY last_seq DESC, s.created_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([agent_id], |r| {
                Ok(PrivateSession {
                    id: r.get(0)?,
                    agent_id: r.get(1)?,
                    name: r.get(2)?,
                    archived: r.get(3)?,
                    is_default: r.get(4)?,
                    created_at: r.get(5)?,
                    preview: r.get(6)?,
                    updated_at: r.get(7)?,
                    status: r.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn create_private_session(&self, agent_id: &str, title: &str) -> Result<PrivateSession, String> {
        let title = validate_name(title)?;
        let default_id = ensure_default_conversation(self, agent_id)?;
        let root = self.conversation_root(&default_id)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO conversations(id,workspace_path) VALUES(?1,?2)",
            params![id, root],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO private_sessions(conversation_id,agent_id,name,archived,is_default) VALUES(?1,?2,?3,0,0)",
            params![id, agent_id, title],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.private_sessions(agent_id)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "会话创建后无法读取。".into())
    }

    pub fn update_private_session(
        &self,
        agent_id: &str,
        id: &str,
        title: &str,
        archived: bool,
    ) -> Result<PrivateSession, String> {
        let title = validate_name(title)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let previous: Option<bool> = tx
            .query_row(
                "SELECT archived FROM private_sessions WHERE conversation_id=?1 AND agent_id=?2",
                params![id, agent_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let previous = previous.ok_or("会话不属于当前 Agent。")?;
        if archived && !previous && session_busy(&tx, id)? {
            return Err("请先完成或停止本会话的任务，再归档。".into());
        }
        tx.execute(
            "UPDATE private_sessions SET name=?3,archived=?4 WHERE conversation_id=?1 AND agent_id=?2",
            params![id, agent_id, title, archived],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.private_sessions(agent_id)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "会话不存在。".into())
    }

    pub(crate) fn verify_private_session(&self, agent_id: &str, id: &str) -> Result<(), String> {
        let exists: bool = self
            .connect()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM private_sessions WHERE agent_id=?1 AND conversation_id=?2)",
                params![agent_id, id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            Ok(())
        } else {
            Err("会话不存在或不属于当前 Agent。".into())
        }
    }

    pub(crate) fn ensure_private_session_open(&self, conversation: &str) -> Result<(), String> {
        let archived: Option<bool> = self
            .connect()?
            .query_row(
                "SELECT archived FROM private_sessions WHERE conversation_id=?1",
                [conversation],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if archived == Some(true) {
            Err("会话已归档，请恢复后再发送消息。".into())
        } else {
            Ok(())
        }
    }

    pub fn delete_private_session(&self, agent_id: &str, id: &str) -> Result<(), String> {
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let is_default: Option<bool> = tx
            .query_row(
                "SELECT is_default FROM private_sessions WHERE agent_id=?1 AND conversation_id=?2",
                params![agent_id, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let is_default = is_default.ok_or("会话不属于当前 Agent 或已删除。")?;
        if session_busy(&tx, id)? {
            return Err("请先完成或停止本会话的任务，再删除。".into());
        }
        crate::agents::clear_bindings(&tx, id)?;
        for table in [
            "codex_messages",
            "codex_sessions",
            "deepseek_sessions",
            "gemini_sessions",
            "reasoning_preferences",
            "speed_preferences",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE conversation_id=?1"), [id])
                .map_err(|e| e.to_string())?;
        }
        if id == crate::deepseek::PERSONAL {
            tx.execute("DELETE FROM deepseek_messages", [])
                .map_err(|e| e.to_string())?;
        }
        if !is_default {
            tx.execute(
                "DELETE FROM private_sessions WHERE conversation_id=?1 AND agent_id=?2",
                params![id, agent_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM conversations WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}
