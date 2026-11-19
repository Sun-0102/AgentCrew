use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

pub fn save(
    store: &Store,
    conversation: &str,
    agent: &str,
    model: &str,
    effort: Option<&str>,
) -> Result<(), String> {
    let Some(effort) = effort else { return Ok(()) };
    let valid = match agent {
        "codex" => matches!(
            effort,
            "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
        ),
        "deepseek" => matches!(effort, "off" | "low" | "high" | "max"),
        _ => false,
    };
    if !valid || model.is_empty() {
        return Err("请选择有效的思考模式。".into());
    }
    store.connect()?.execute("INSERT INTO reasoning_preferences(conversation_id,agent,model,effort) VALUES(?1,?2,?3,?4) ON CONFLICT(conversation_id,agent,model) DO UPDATE SET effort=excluded.effort", params![conversation,agent,model,effort]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read(
    store: &Store,
    conversation: &str,
    agent: &str,
    model: &str,
) -> Result<Option<String>, String> {
    store.connect()?.query_row("SELECT effort FROM reasoning_preferences WHERE conversation_id=?1 AND agent=?2 AND model=?3", params![conversation,agent,model], |r| r.get(0)).optional().map_err(|e| e.to_string())
}

pub fn validate(models: &Value, model: &str, effort: Option<&str>) -> Result<(), String> {
    let Some(effort) = effort else { return Ok(()) };
    if !models.as_array().is_some_and(|models| {
        models.iter().any(|m| {
            m["model"] == model
                && m["supportedReasoningEfforts"]
                    .as_array()
                    .is_some_and(|choices| choices.iter().any(|c| c["reasoningEffort"] == effort))
        })
    }) {
        return Err("当前模型不支持所选思考模式，请重新选择。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preferences_survive_restart_and_are_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let store = Store::open(path.clone()).unwrap();
        save(&store, "chat", "deepseek", "model", Some("off")).unwrap();
        save(&store, "chat", "codex", "model", Some("high")).unwrap();
        assert!(save(&store, "chat", "deepseek", "model", Some("medium")).is_err());
        let store = Store::open(path).unwrap();
        assert_eq!(
            read(&store, "chat", "deepseek", "model")
                .unwrap()
                .as_deref(),
            Some("off")
        );
        assert_eq!(
            read(&store, "chat", "codex", "model").unwrap().as_deref(),
            Some("high")
        );
        assert_eq!(read(&store, "other", "deepseek", "model").unwrap(), None);
        assert_eq!(read(&store, "chat", "deepseek", "other").unwrap(), None);
        let models = json!([{"model":"model","supportedReasoningEfforts":[{"reasoningEffort":"off"},{"reasoningEffort":"high"}]}]);
        assert!(validate(&models, "model", Some("off")).is_ok());
        assert!(validate(&models, "model", Some("medium")).is_err());
        assert!(validate(&models, "missing", Some("high")).is_err());
    }
}
