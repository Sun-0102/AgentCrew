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
}

fn validate(name: &str, members: &[String]) -> Result<Vec<String>, String> {
    if name.trim().is_empty() || name.trim().chars().count() > 40 {
        return Err("群名称请填写 1–40 个字。".into());
    }
    if members.is_empty()
        || members.len() > 2
        || members
            .iter()
            .any(|m| !matches!(m.as_str(), "codex" | "deepseek"))
    {
        return Err("请选择本机 Codex 或 DeepSeek 成员。".into());
    }
    let mut members = members.to_vec();
    members.sort();
    members.dedup();
    Ok(members)
}

impl Store {
    pub fn groups(&self) -> Result<Vec<Group>, String> {
        let connection = self.connect()?;
        let mut statement=connection.prepare("SELECT g.conversation_id,g.name,g.members,g.project_id,g.created_at,(SELECT text FROM codex_messages WHERE conversation_id=g.conversation_id ORDER BY sequence DESC LIMIT 1),(SELECT created_at FROM codex_messages WHERE conversation_id=g.conversation_id ORDER BY sequence DESC LIMIT 1),s.status FROM chat_groups g LEFT JOIN codex_sessions s ON s.conversation_id=g.conversation_id ORDER BY COALESCE((SELECT MAX(sequence) FROM codex_messages WHERE conversation_id=g.conversation_id),0) DESC,g.created_at DESC").map_err(|e|e.to_string())?;
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
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            let (id, name, members, project_id, created_at, preview, updated_at, status) =
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
            })
        })
        .collect()
    }

    pub fn group(&self, id: &str) -> Result<Group, String> {
        self.groups()?
            .into_iter()
            .find(|g| g.id == id)
            .ok_or("群聊不存在".into())
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
        tx.commit().map_err(|e| e.to_string())?;
        self.group(&id)
    }

    pub fn update_group(
        &self,
        id: &str,
        name: &str,
        members: Vec<String>,
    ) -> Result<Group, String> {
        let members = validate(name, &members)?;
        let old = self.group(id)?;
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let running:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if running {
            return Err("请先完成或取消当前任务，再调整群成员。".into());
        }
        tx.execute(
            "UPDATE chat_groups SET name=?2,members=?3 WHERE conversation_id=?1",
            params![
                id,
                name.trim(),
                serde_json::to_string(&members).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        if old.members != members {
            tx.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,?2,'system',?3)",params![id,uuid::Uuid::new_v4().to_string(),format!("群成员已更新 · {}",members.join("、"))]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.group(id)
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
        if target != "codex" {
            return Err("DeepSeek 尚未接入，暂时不能执行任务。请先选择 Codex。".into());
        }
        Ok(())
    }

    pub fn group_name(&self, id: &str) -> Result<Option<String>, String> {
        self.connect()?
            .query_row(
                "SELECT name FROM chat_groups WHERE conversation_id=?1",
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
            )
            .unwrap();
        assert!(store.validate_group_target(&one.id, "deepseek").is_err());
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
