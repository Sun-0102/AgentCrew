use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct GroupSession {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub archived: bool,
    pub created_at: String,
    pub preview: Option<String>,
    pub updated_at: Option<String>,
    pub status: Option<String>,
}

fn name(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 40 {
        return Err("会话名称请填写 1–40 个字。".into());
    }
    Ok(value)
}

impl Store {
    pub(crate) fn parent_group_id(&self, conversation: &str) -> Result<String, String> {
        self.connect()?.query_row(
            "SELECT COALESCE(parent_group_id,conversation_id) FROM chat_groups WHERE conversation_id=?1",
            [conversation], |r| r.get(0),
        ).map_err(|_| "群聊或会话不存在。".into())
    }

    pub fn group_sessions(&self, group: &str) -> Result<Vec<GroupSession>, String> {
        let parent = self.parent_group_id(group)?;
        if parent != group { return Err("请从群聊打开会话列表。".into()); }
        let connection = self.connect()?;
        let mut statement = connection.prepare("SELECT s.conversation_id,s.group_id,s.name,s.archived,s.created_at,(SELECT text FROM codex_messages WHERE conversation_id=s.conversation_id AND role IN ('user','assistant') ORDER BY sequence DESC LIMIT 1),(SELECT created_at FROM codex_messages WHERE conversation_id=s.conversation_id ORDER BY sequence DESC LIMIT 1),(SELECT status FROM group_tasks WHERE conversation_id=s.conversation_id ORDER BY rowid DESC LIMIT 1) FROM group_sessions s WHERE s.group_id=?1 ORDER BY COALESCE((SELECT MAX(sequence) FROM codex_messages WHERE conversation_id=s.conversation_id),0) DESC,s.created_at DESC").map_err(|e| e.to_string())?;
        let rows = statement.query_map([group], |r| Ok(GroupSession {
            id: r.get(0)?, group_id: r.get(1)?, name: r.get(2)?, archived: r.get(3)?,
            created_at: r.get(4)?, preview: r.get(5)?, updated_at: r.get(6)?, status: r.get(7)?,
        })).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn create_group_session(&self, group: &str, title: &str) -> Result<GroupSession, String> {
        let title = name(title)?;
        let parent = self.parent_group_id(group)?;
        if parent != group { return Err("请选择有效群聊。".into()); }
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO conversations(id,workspace_path) SELECT ?2,workspace_path FROM conversations WHERE id=?1", params![group,id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO chat_groups(conversation_id,name,members,project_id,parent_group_id) SELECT ?2,name,members,project_id,conversation_id FROM chat_groups WHERE conversation_id=?1 AND parent_group_id IS NULL",params![group,id]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO group_sessions(conversation_id,group_id,name) VALUES(?1,?2,?3)",params![id,group,title]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.group_sessions(group)?.into_iter().find(|s| s.id == id).ok_or("会话创建后无法读取。".into())
    }

    pub fn update_group_session(&self, group: &str, id: &str, title: &str, archived: bool) -> Result<GroupSession, String> {
        let title = name(title)?;
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let previous: Option<bool> = tx.query_row("SELECT archived FROM group_sessions WHERE conversation_id=?1 AND group_id=?2",params![id,group],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let previous = previous.ok_or("会话不属于当前群聊。")?;
        if archived && !previous {
            let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM group_tasks WHERE conversation_id=?1 AND status IN ('running','paused','cancelling')) OR EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
            if busy { return Err("请先完成或停止本会话的任务，再归档。".into()); }
        }
        tx.execute("UPDATE group_sessions SET name=?3,archived=?4 WHERE conversation_id=?1 AND group_id=?2",params![id,group,title,archived]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e|e.to_string())?;
        self.group_sessions(group)?.into_iter().find(|s|s.id==id).ok_or("会话不存在。".into())
    }

    pub(crate) fn ensure_session_open(&self, conversation: &str) -> Result<(), String> {
        let archived: bool = self.connect()?.query_row("SELECT archived FROM group_sessions WHERE conversation_id=?1",[conversation],|r|r.get(0)).map_err(|e|e.to_string())?;
        if archived { Err("会话已归档，请恢复后再发送消息或继续任务。".into()) } else { Ok(()) }
    }

    pub fn delete_group_session(&self, group: &str, id: &str) -> Result<(), String> {
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM group_sessions WHERE group_id=?1 AND conversation_id=?2)",params![group,id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !exists { return Err("会话不属于当前群聊或已删除。".into()); }
        let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM group_tasks WHERE conversation_id=?1 AND status IN ('running','paused','cancelling')) OR EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if busy { return Err("请先完成或停止本会话的任务，再删除。".into()); }
        crate::agents::clear_bindings(&tx,id)?;
        for table in ["group_task_steps", "group_task_inputs", "group_task_members"] {
            tx.execute(&format!("DELETE FROM {table} WHERE task_id IN (SELECT id FROM group_tasks WHERE conversation_id=?1)"),[id]).map_err(|e|e.to_string())?;
        }
        for table in ["group_tasks", "group_message_summaries", "codex_messages", "codex_sessions", "deepseek_sessions", "gemini_sessions", "reasoning_preferences", "speed_preferences", "group_sessions"] {
            tx.execute(&format!("DELETE FROM {table} WHERE conversation_id=?1"),[id]).map_err(|e|e.to_string())?;
        }
        // The default conversation also hosts the parent group and must remain addressable.
        if id != group {
            tx.execute("DELETE FROM chat_groups WHERE conversation_id=?1 AND parent_group_id=?2",params![id,group]).map_err(|e|e.to_string())?;
            tx.execute("DELETE FROM conversations WHERE id=?1",[id]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e|e.to_string())
    }
}
