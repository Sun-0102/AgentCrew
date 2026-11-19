use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

pub fn save(
    store: &Store,
    conversation: &str,
    model: &str,
    tier: Option<&str>,
) -> Result<(), String> {
    let Some(tier) = tier else { return Ok(()) };
    if model.is_empty() || tier.is_empty() || tier.len() > 64 {
        return Err("请选择有效的 Codex 速度。".into());
    }
    store.connect()?.execute("INSERT INTO speed_preferences(conversation_id,model,service_tier) VALUES(?1,?2,?3) ON CONFLICT(conversation_id,model) DO UPDATE SET service_tier=excluded.service_tier", params![conversation,model,tier]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read(store: &Store, conversation: &str, model: &str) -> Result<Option<String>, String> {
    store
        .connect()?
        .query_row(
            "SELECT service_tier FROM speed_preferences WHERE conversation_id=?1 AND model=?2",
            params![conversation, model],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

pub fn validate(models: &Value, model: &str, tier: Option<&str>) -> Result<(), String> {
    let Some(tier) = tier else { return Ok(()) };
    if !models.as_array().is_some_and(|models| {
        models.iter().any(|m| {
            m["model"] == model
                && (tier == "default"
                    || m["serviceTiers"]
                        .as_array()
                        .is_some_and(|tiers| tiers.iter().any(|t| t["id"] == tier)))
        })
    }) {
        return Err("当前模型或账号不支持所选速度，请重新选择。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_nine_upgrade_preserves_reasoning_and_backs_up() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let connection = rusqlite::Connection::open(&path).unwrap();
        for migration in [
            include_str!("../migrations/001_projects.sql"),
            include_str!("../migrations/002_codex.sql"),
            include_str!("../migrations/003_conversations.sql"),
            include_str!("../migrations/004_groups.sql"),
            include_str!("../migrations/005_deepseek.sql"),
            include_str!("../migrations/006_deepseek_groups.sql"),
            include_str!("../migrations/007_group_tasks.sql"),
            include_str!("../migrations/008_group_inputs.sql"),
            include_str!("../migrations/009_reasoning.sql"),
        ] { connection.execute_batch(migration).unwrap(); }
        drop(connection);
        let store = Store { path: path.clone() };
        crate::reasoning::save(&store, "chat", "codex", "model", Some("high")).unwrap();
        let store = Store::open(path).unwrap();
        assert_eq!(
            crate::reasoning::read(&store, "chat", "codex", "model")
                .unwrap()
                .as_deref(),
            Some("high")
        );
        assert_eq!(read(&store, "chat", "model").unwrap(), None);
        let backup = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|p| p.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().contains("v9-"))
            .unwrap();
        let backup = rusqlite::Connection::open(backup).unwrap();
        assert_eq!(
            backup
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            9
        );
    }

    #[test]
    fn speed_persists_separately_from_reasoning_and_validates_model_access() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let store = Store::open(path.clone()).unwrap();
        crate::reasoning::save(&store, "chat", "codex", "model", Some("high")).unwrap();
        save(&store, "chat", "model", Some("priority")).unwrap();
        let store = Store::open(path).unwrap();
        assert_eq!(
            read(&store, "chat", "model").unwrap().as_deref(),
            Some("priority")
        );
        assert_eq!(read(&store, "other", "model").unwrap(), None);
        assert_eq!(read(&store, "chat", "other").unwrap(), None);
        save(&store, "chat", "model", Some("default")).unwrap();
        assert_eq!(
            read(&store, "chat", "model").unwrap().as_deref(),
            Some("default")
        );
        assert_eq!(
            crate::reasoning::read(&store, "chat", "codex", "model")
                .unwrap()
                .as_deref(),
            Some("high")
        );
        let models = json!([{"model":"model","serviceTiers":[{"id":"priority"}]},{"model":"standard-only","serviceTiers":[]}]);
        assert!(validate(&models, "model", Some("priority")).is_ok());
        assert!(validate(&models, "model", Some("default")).is_ok());
        assert!(validate(&models, "model", Some("ultrafast")).is_err());
        assert!(validate(&models, "standard-only", Some("priority")).is_err());
        assert!(validate(&models, "missing", Some("default")).is_err());
        assert!(save(&store, "chat", "model", Some("")).is_err());
    }
}
