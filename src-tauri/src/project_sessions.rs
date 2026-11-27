use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ProjectSession {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub archived: bool,
    pub is_default: bool,
    pub is_scheduled: bool,
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

fn ensure_default_project_session(store: &Store, project_id: &str) -> Result<String, String> {
    let connection = store.connect()?;
    let primary_root: String = connection
        .query_row(
            "SELECT primary_root FROM projects WHERE id=?1",
            [project_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "项目不存在。".to_owned())?;
    connection
        .execute(
            "INSERT OR IGNORE INTO conversations(id,project_id,workspace_path) VALUES(?1,?1,?2)",
            params![project_id, primary_root],
        )
        .map_err(|e| e.to_string())?;
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM project_sessions WHERE project_id=?1 AND is_default=1)",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        connection
            .execute(
                "INSERT OR IGNORE INTO project_sessions(conversation_id,project_id,name,archived,is_default) VALUES(?1,?1,'默认会话',0,1)",
                [project_id],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(primary_root)
}

fn session_busy(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<bool, String> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
         OR EXISTS(SELECT 1 FROM claude_sessions WHERE conversation_id IN (SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
         OR EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
         OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",
        [id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

impl Store {
    pub fn project_sessions(&self, project_id: &str) -> Result<Vec<ProjectSession>, String> {
        ensure_default_project_session(self, project_id)?;
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT s.conversation_id, s.project_id, s.name, s.archived, s.is_default, s.created_at,
                    (SELECT text FROM codex_messages WHERE conversation_id=s.conversation_id AND role IN ('user','assistant') ORDER BY sequence DESC LIMIT 1),
                    (SELECT created_at FROM codex_messages WHERE conversation_id=s.conversation_id ORDER BY sequence DESC LIMIT 1),
                    COALESCE(
                        (SELECT status FROM codex_sessions WHERE conversation_id=s.conversation_id AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1),
                        (SELECT status FROM deepseek_sessions WHERE conversation_id=s.conversation_id AND status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1),
                        (SELECT g.status FROM gemini_sessions g JOIN agent_sessions a ON a.runtime_conversation_id=g.conversation_id WHERE a.conversation_id=s.conversation_id AND g.status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1),
                        (SELECT c.status FROM claude_sessions c JOIN agent_sessions a ON a.runtime_conversation_id=c.conversation_id WHERE a.conversation_id=s.conversation_id AND c.status IN ('starting','running','waiting_approval','cancel_requested','stopping') LIMIT 1)
                    ),
                    COALESCE((SELECT MAX(sequence) FROM codex_messages WHERE conversation_id=s.conversation_id), 0) AS last_seq,
                    (s.is_default = 0 AND (
                        EXISTS(SELECT 1 FROM scheduled_task_runs r WHERE r.conversation_id=s.conversation_id)
                        OR EXISTS(SELECT 1 FROM scheduled_tasks t WHERE t.session_id=s.conversation_id)
                    )) AS is_scheduled
                 FROM project_sessions s
                 WHERE s.project_id=?1
                 ORDER BY last_seq DESC, s.created_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([project_id], |r| {
                Ok(ProjectSession {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    name: r.get(2)?,
                    archived: r.get(3)?,
                    is_default: r.get(4)?,
                    created_at: r.get(5)?,
                    preview: r.get(6)?,
                    updated_at: r.get(7)?,
                    status: r.get(8)?,
                    is_scheduled: r.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn create_project_session(&self, project_id: &str, title: &str) -> Result<ProjectSession, String> {
        let title = validate_name(title)?;
        let root = ensure_default_project_session(self, project_id)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().to_string();
        // conversations.project_id is unique and reserved for the default conversation.
        // Additional conversations belong to the project through project_sessions below.
        tx.execute(
            "INSERT INTO conversations(id,workspace_path) VALUES(?1,?2)",
            params![id, root],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO project_sessions(conversation_id,project_id,name,archived,is_default) VALUES(?1,?2,?3,0,0)",
            params![id, project_id, title],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.project_sessions(project_id)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "会话创建后无法读取。".into())
    }

    pub fn update_project_session(
        &self,
        project_id: &str,
        id: &str,
        title: &str,
        archived: bool,
    ) -> Result<ProjectSession, String> {
        let title = validate_name(title)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let previous: Option<bool> = tx
            .query_row(
                "SELECT archived FROM project_sessions WHERE conversation_id=?1 AND project_id=?2",
                params![id, project_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let previous = previous.ok_or("会话不属于当前项目。")?;
        if archived && !previous && session_busy(&tx, id)? {
            return Err("请先完成或停止本会话的任务，再归档。".into());
        }
        tx.execute(
            "UPDATE project_sessions SET name=?3,archived=?4 WHERE conversation_id=?1 AND project_id=?2",
            params![id, project_id, title, archived],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.project_sessions(project_id)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| "会话不存在。".into())
    }

    pub(crate) fn verify_project_session(&self, project_id: &str, id: &str) -> Result<(), String> {
        let exists: bool = self
            .connect()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM project_sessions WHERE project_id=?1 AND conversation_id=?2)",
                params![project_id, id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            Ok(())
        } else {
            Err("会话不存在或不属于当前项目。".into())
        }
    }

    pub(crate) fn ensure_project_session_open(&self, conversation: &str) -> Result<(), String> {
        let archived: Option<bool> = self
            .connect()?
            .query_row(
                "SELECT archived FROM project_sessions WHERE conversation_id=?1",
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

    pub fn delete_project_session(&self, project_id: &str, id: &str) -> Result<(), String> {
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let is_default: Option<bool> = tx
            .query_row(
                "SELECT is_default FROM project_sessions WHERE project_id=?1 AND conversation_id=?2",
                params![project_id, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let is_default = is_default.ok_or("会话不属于当前项目或已删除。")?;
        if session_busy(&tx, id)? {
            return Err("请先完成或停止本会话的任务，再删除。".into());
        }
        crate::agents::clear_bindings(&tx, id)?;
        for table in [
            "codex_messages",
            "codex_sessions",
            "deepseek_sessions",
            "gemini_sessions",
            "claude_sessions",
            "reasoning_preferences",
            "speed_preferences",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE conversation_id=?1"), [id])
                .map_err(|e| e.to_string())?;
        }
        if !is_default {
            tx.execute(
                "DELETE FROM project_sessions WHERE conversation_id=?1 AND project_id=?2",
                params![id, project_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM conversations WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}
