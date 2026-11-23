use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub project_id: Option<String>,
    pub created_at: String,
    pub preview: Option<String>,
    pub updated_at: Option<String>,
    pub status: Option<String>,
    pub session_count: u32,
}

fn validate(name: &str, members: &[String]) -> Result<Vec<String>, String> {
    if name.trim().is_empty() || name.trim().chars().count() > 40 {
        return Err("群名称请填写 1–40 个字。".into());
    }
    if members.is_empty() || members.len() > 32 {
        return Err("请选择 1–32 位已接入的本机 Agent。".into());
    }
    let mut members = members.to_vec();
    members.sort();
    members.dedup();
    Ok(members)
}

impl Store {
    pub fn groups(&self) -> Result<Vec<Group>, String> {
        let connection = self.connect()?;
        let mut statement=connection.prepare("SELECT g.conversation_id,g.name,g.members,g.project_id,g.created_at,(SELECT text FROM codex_messages WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=g.conversation_id) AND role IN ('user','assistant') ORDER BY sequence DESC LIMIT 1),(SELECT created_at FROM codex_messages WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=g.conversation_id) ORDER BY sequence DESC LIMIT 1),(SELECT t.status FROM group_tasks t JOIN group_sessions c ON c.conversation_id=t.conversation_id WHERE c.group_id=g.conversation_id ORDER BY CASE WHEN t.status IN ('running','paused','cancelling') THEN 0 ELSE 1 END,t.rowid DESC LIMIT 1),(SELECT COUNT(*) FROM group_sessions WHERE group_id=g.conversation_id AND archived=0) FROM chat_groups g WHERE g.parent_group_id IS NULL ORDER BY COALESCE((SELECT MAX(sequence) FROM codex_messages WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=g.conversation_id)),0) DESC,g.created_at DESC").map_err(|e|e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, u32>(8)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            let (id, name, members, project_id, created_at, preview, updated_at, status, session_count) =
                row.map_err(|e| e.to_string())?;
            Ok(Group {
                id,
                name,
                members: serde_json::from_str(&members)
                    .map_err(|e| format!("群成员记录无效：{e}"))?,
                project_id,
                created_at,
                preview,
                updated_at,
                status,
                session_count,
            })
        })
        .collect()
    }

    pub fn group(&self, id: &str) -> Result<Group, String> {
        let parent = self.parent_group_id(id)?;
        let mut group = self.groups()?
            .into_iter()
            .find(|g| g.id == parent)
            .ok_or("群聊不存在")?;
        // Runtime callers use the selected conversation, with live parent membership.
        group.id = id.to_owned();
        Ok(group)
    }

    pub fn create_group(
        &self,
        name: &str,
        members: Vec<String>,
        project_id: Option<String>,
    ) -> Result<Group, String> {
        let parent = self.chat_workspace_parent()?.join("groups");
        self.create_group_in(name, members, project_id, &parent)
    }

    pub(crate) fn create_group_in(
        &self,
        name: &str,
        members: Vec<String>,
        project_id: Option<String>,
        parent: &std::path::Path,
    ) -> Result<Group, String> {
        let members = validate(name, &members)?;
        for member in &members { crate::agents::ensure_enabled(self, member)?; }
        let id = uuid::Uuid::new_v4().to_string();
        let root = if let Some(project) = project_id.as_deref() {
            self.list_projects()?
                .into_iter()
                .find(|p| p.id == project)
                .ok_or("所选项目不存在")?
                .primary_root
        } else {
            let path = parent.join(&id);
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            path.canonicalize()
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("群工作目录不是有效 Unicode 路径")?
                .to_owned()
        };
        let mut connection = self.connect()?;
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO conversations(id,project_id,workspace_path) VALUES(?1,NULL,?2)",
            params![id, root],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO chat_groups(conversation_id,name,members,project_id) VALUES(?1,?2,?3,?4)",
            params![
                id,
                name.trim(),
                serde_json::to_string(&members).map_err(|e| e.to_string())?,
                project_id
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'system',?3)",params![id,uuid::Uuid::new_v4().to_string(),format!("群聊已创建 · {}",members.join("、"))]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO group_sessions(conversation_id,group_id,name) VALUES(?1,?1,'默认会话')",[&id]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.group(&id)
    }

    pub fn update_group(
        &self,
        id: &str,
        name: &str,
        members: Vec<String>,
        project_id: Option<String>,
    ) -> Result<Group, String> {
        let members = validate(name, &members)?;
        for member in &members { crate::agents::ensure_enabled(self, member)?; }
        if self.parent_group_id(id)? != id { return Err("请在群设置中修改所属群聊。".into()); }
        let old = self.group(id)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let running:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT a.runtime_conversation_id FROM agent_sessions a JOIN group_sessions s ON s.conversation_id=a.conversation_id WHERE s.group_id=?1) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM group_tasks WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1) AND status IN ('running','paused','cancelling'))",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if running {
            return Err(
                "请先完成或取消当前任务（含暂停的自动协作），再修改群成员或关联项目。".into(),
            );
        }
        if old.project_id != project_id {
            let previous_root: String = tx
                .query_row(
                    "SELECT workspace_path FROM conversations WHERE id=?1",
                    [id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let (root, project_label) = if let Some(project) = project_id.as_deref() {
                let project: Option<(String, String)> = tx
                    .query_row(
                        "SELECT primary_root,name FROM projects WHERE id=?1",
                        [project],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                let (path, label) = project.ok_or("所选项目不存在，请重新选择。")?;
                (std::path::PathBuf::from(path), label)
            } else {
                let path = self.chat_workspace_parent()?.join("groups").join(id);
                std::fs::create_dir_all(&path).map_err(|e| format!("无法建立群专用目录：{e}"))?;
                (path, "未关联，使用群专用目录".to_owned())
            };
            let root = root
                .canonicalize()
                .map_err(|e| format!("关联目录不可访问：{e}"))?;
            std::fs::read_dir(&root).map_err(|e| format!("关联目录不可读：{e}"))?;
            let root = root.to_str().ok_or("关联目录不是有效 Unicode 路径")?;
            let codex_thread: Option<String> = tx
                .query_row(
                    "SELECT thread_id FROM codex_sessions WHERE conversation_id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let deepseek_thread: Option<String> = tx
                .query_row(
                    "SELECT thread_id FROM deepseek_sessions WHERE conversation_id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let sessions=self.group_sessions(id)?;
            for session in sessions { crate::agents::clear_bindings(&tx,&session.id)?; }
            // A loaded provider session can retain its old cwd; only reset its binding, never its transcript.
            tx.execute("DELETE FROM codex_sessions WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1)", [id])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM deepseek_sessions WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1)",
                [id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE conversations SET workspace_path=?2 WHERE id=?1 OR id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1)",
                params![id, root],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("UPDATE group_tasks SET status='interrupted',error='群关联项目已变更，旧目录的验收结论不再适用；请重新指派。',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE conversation_id IN (SELECT conversation_id FROM group_sessions WHERE group_id=?1) AND status IN ('awaiting_review','idle')",[id]).map_err(|e| e.to_string())?;
            let sessions = serde_json::json!({"codex":codex_thread,"deepseek":deepseek_thread});
            let summary = format!("群关联项目已更新 · {project_label} · 聊天记录保留，旧文件未搬移，后续使用新的执行会话。");
            tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'system',?3)",params![id,uuid::Uuid::new_v4().to_string(),summary]).map_err(|e| e.to_string())?;
            let details = format!("原目录：{previous_root}\n当前目录：{root}\n原执行会话索引：{sessions}\n原厂会话文件保留。后续在新目录创建执行会话；需要旧任务内容时请引用群消息。");
            tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'workspace',?3)",params![id,uuid::Uuid::new_v4().to_string(),details]).map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE chat_groups SET name=?2,members=?3,project_id=?4 WHERE conversation_id=?1 OR parent_group_id=?1",
            params![
                id,
                name.trim(),
                serde_json::to_string(&members).map_err(|e| e.to_string())?,
                project_id
            ],
        )
        .map_err(|e| e.to_string())?;
        if old.members != members {
            tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'system',?3)",params![id,uuid::Uuid::new_v4().to_string(),format!("群成员已更新 · {}",members.join("、"))]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.group(id)
    }

    pub fn delete_group(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_groups WHERE conversation_id=?1 AND parent_group_id IS NULL)",
            [id], |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        if !exists { return Err("群聊不存在或已删除。".into()); }
        // Include the parent even when its default session has already been deleted.
        let conversations = {
            let mut statement = tx.prepare(
                "SELECT ?1 UNION SELECT conversation_id FROM group_sessions WHERE group_id=?1 UNION SELECT conversation_id FROM chat_groups WHERE parent_group_id=?1",
            ).map_err(|e| e.to_string())?;
            let rows = statement.query_map([id], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        // Check every conversation before removing any provider bindings.
        for conversation in &conversations {
            let busy: bool = tx.query_row(
                "WITH session_ids AS (SELECT ?1 AS id UNION SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1)
                 SELECT EXISTS(SELECT 1 FROM group_tasks WHERE conversation_id=?1 AND status IN ('running','paused','cancelling'))
                 OR EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id IN (SELECT id FROM session_ids) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
                 OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE conversation_id IN (SELECT id FROM session_ids) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))
                 OR EXISTS(SELECT 1 FROM gemini_sessions WHERE conversation_id IN (SELECT id FROM session_ids) AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",
                [conversation], |row| row.get(0),
            ).map_err(|e| e.to_string())?;
            if busy { return Err("请先完成或取消群内所有任务（含暂停或停止中的任务），再删除群。".into()); }
        }
        for conversation in &conversations {
            crate::agents::clear_bindings(&tx, conversation)?;
            for table in ["group_task_steps", "group_task_inputs", "group_task_members"] {
                tx.execute(&format!("DELETE FROM {table} WHERE task_id IN (SELECT id FROM group_tasks WHERE conversation_id=?1)"), [conversation])
                    .map_err(|e| e.to_string())?;
            }
            for table in ["group_tasks", "group_message_summaries", "codex_messages", "codex_sessions", "deepseek_sessions", "gemini_sessions", "reasoning_preferences", "speed_preferences", "group_sessions"] {
                tx.execute(&format!("DELETE FROM {table} WHERE conversation_id=?1"), [conversation])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.execute("DELETE FROM chat_groups WHERE parent_group_id=?1", [id]).map_err(|e| e.to_string())?;
        for conversation in &conversations {
            if conversation != id {
                tx.execute("DELETE FROM conversations WHERE id=?1", [conversation]).map_err(|e| e.to_string())?;
            }
        }
        tx.execute("DELETE FROM chat_groups WHERE conversation_id=?1", [id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM conversations WHERE id=?1", [id]).map_err(|e| e.to_string())?;
        // Project files, workspaces, image caches and native transcripts remain on disk.
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn resolve_chat(
        &self,
        project: Option<&str>,
        group: Option<&str>,
    ) -> Result<String, String> {
        match group {
            Some(id) => Ok(self.group(id)?.id),
            None => self.conversation_id(project),
        }
    }

    pub fn validate_group_target(&self, id: &str, target: &str) -> Result<(), String> {
        let group = self.group(id)?;
        if !group.members.iter().any(|m| m == target) {
            return Err("该 Agent 不在当前群内。".into());
        }
        crate::agents::ensure_enabled(self, target)?;
        Ok(())
    }

    pub fn group_name(&self, id: &str) -> Result<Option<String>, String> {
        self.connect()?
            .query_row(
                "SELECT g.name || ' / ' || s.name FROM group_sessions s JOIN chat_groups g ON g.conversation_id=s.group_id WHERE s.conversation_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_persist_members_and_have_separate_contexts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("app.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let one = store
            .create_group_in(
                "产品讨论",
                vec!["codex".into()],
                None,
                &temp.path().join("groups"),
            )
            .unwrap();
        let two = store
            .create_group_in(
                "内容小组",
                vec!["codex".into(), "deepseek".into()],
                None,
                &temp.path().join("groups"),
            )
            .unwrap();
        assert_ne!(one.id, two.id);
        assert_ne!(
            store.conversation_root(&one.id).unwrap(),
            store.conversation_root(&two.id).unwrap()
        );
        store
            .update_group(
                &one.id,
                "产品协作群",
                vec!["codex".into(), "deepseek".into()],
                None,
            )
            .unwrap();
        assert!(store.validate_group_target(&one.id, "deepseek").is_ok());
        assert!(store.validate_group_target(&one.id, "codex").is_ok());
        let store = Store::open(path).unwrap();
        assert_eq!(store.groups().unwrap().len(), 2);
        assert_eq!(store.group(&one.id).unwrap().name, "产品协作群");
        let connection = store.connect().unwrap();
        let time: Option<String> = connection
            .query_row(
                "SELECT created_at FROM codex_messages WHERE conversation_id=?1 LIMIT 1",
                [&one.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(time.unwrap().ends_with('Z'));
        assert!(store.create_group(" ", vec!["codex".into()], None).is_err());
        assert!(store
            .create_group("invalid", vec!["unknown".into()], None)
            .is_err());
        assert!(store.resolve_chat(None, Some("missing")).is_err());
    }
}
