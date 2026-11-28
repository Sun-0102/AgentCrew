use crate::storage::Store;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelDefaultEntry {
    pub scope: String,
    pub agent_id: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelDefaultsConfig {
    pub group_follow_private: bool,
    pub entries: Vec<ModelDefaultEntry>,
}

fn validate_scope(scope: &str) -> Result<&str, String> {
    match scope.trim() {
        "private" => Ok("private"),
        "group" => Ok("group"),
        _ => Err("无效的配置场景，仅支持私聊或群聊。".into()),
    }
}

fn clean_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn get_config(store: &Store) -> Result<ModelDefaultsConfig, String> {
    let connection = store.connect()?;
    let group_follow_private: bool = connection
        .query_row(
            "SELECT group_follow_private_defaults FROM app_controls WHERE id=1",
            [],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(true);

    let mut stmt = connection
        .prepare(
            "SELECT scope, agent_id, model, reasoning_effort, service_tier
             FROM model_defaults
             ORDER BY scope, agent_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ModelDefaultEntry {
                scope: r.get(0)?,
                agent_id: r.get(1)?,
                model: r.get(2)?,
                reasoning_effort: r.get(3)?,
                service_tier: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let entries = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ModelDefaultsConfig {
        group_follow_private,
        entries,
    })
}

pub fn resolve_for(
    store: &Store,
    scope: &str,
    agent_id: &str,
) -> Result<Option<ModelDefaultEntry>, String> {
    let scope = validate_scope(scope)?;
    let connection = store.connect()?;
    let effective_scope = if scope == "group" {
        let follow: bool = connection
            .query_row(
                "SELECT group_follow_private_defaults FROM app_controls WHERE id=1",
                [],
                |r| r.get::<_, i64>(0).map(|v| v != 0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(true);
        if follow {
            "private"
        } else {
            "group"
        }
    } else {
        "private"
    };

    connection
        .query_row(
            "SELECT scope, agent_id, model, reasoning_effort, service_tier
             FROM model_defaults
             WHERE scope=?1 AND agent_id=?2",
            params![effective_scope, agent_id],
            |r| {
                Ok(ModelDefaultEntry {
                    scope: r.get(0)?,
                    agent_id: r.get(1)?,
                    model: r.get(2)?,
                    reasoning_effort: r.get(3)?,
                    service_tier: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
}

pub fn save_entry(
    store: &Store,
    scope: &str,
    agent_id: &str,
    model: &str,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
) -> Result<ModelDefaultsConfig, String> {
    let scope = validate_scope(scope)?;
    let agent_id = agent_id.trim();
    if agent_id.is_empty() {
        return Err("Agent ID 不能为空。".into());
    }
    let model = model.trim();
    let effort = clean_opt(reasoning_effort);
    let tier = clean_opt(service_tier).filter(|t| t != "default");

    let connection = store.connect()?;
    if model.is_empty() && effort.is_none() && tier.is_none() {
        connection
            .execute(
                "DELETE FROM model_defaults WHERE scope=?1 AND agent_id=?2",
                params![scope, agent_id],
            )
            .map_err(|e| e.to_string())?;
    } else {
        connection
            .execute(
                "INSERT INTO model_defaults(scope, agent_id, model, reasoning_effort, service_tier, updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(scope, agent_id) DO UPDATE SET
                   model=excluded.model,
                   reasoning_effort=excluded.reasoning_effort,
                   service_tier=excluded.service_tier,
                   updated_at=excluded.updated_at",
                params![scope, agent_id, model, effort, tier],
            )
            .map_err(|e| e.to_string())?;
    }
    get_config(store)
}

pub fn set_group_follow_private(
    store: &Store,
    follow: bool,
) -> Result<ModelDefaultsConfig, String> {
    store
        .connect()?
        .execute(
            "UPDATE app_controls SET group_follow_private_defaults=?1 WHERE id=1",
            [if follow { 1 } else { 0 }],
        )
        .map_err(|e| e.to_string())?;
    get_config(store)
}

#[tauri::command]
pub fn model_defaults_get(store: tauri::State<'_, Store>) -> Result<ModelDefaultsConfig, String> {
    get_config(&store)
}

#[tauri::command]
pub fn model_defaults_save(
    store: tauri::State<'_, Store>,
    scope: String,
    agent_id: String,
    model: String,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
) -> Result<ModelDefaultsConfig, String> {
    save_entry(
        &store,
        &scope,
        &agent_id,
        &model,
        reasoning_effort.as_deref(),
        service_tier.as_deref(),
    )
}

#[tauri::command]
pub fn model_defaults_set_group_follow(
    store: tauri::State<'_, Store>,
    follow: bool,
) -> Result<ModelDefaultsConfig, String> {
    set_group_follow_private(&store, follow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_persist_and_group_follow_private_switches_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let store = Store::open(path.clone()).unwrap();

        let initial = get_config(&store).unwrap();
        assert!(initial.group_follow_private);
        assert!(initial.entries.is_empty());

        save_entry(&store, "private", "codex", "gpt-5.4", Some("high"), Some("priority")).unwrap();
        save_entry(&store, "group", "codex", "gpt-5.4-mini", Some("low"), None).unwrap();

        // While group_follow_private is true, group resolution returns private config.
        let resolved = resolve_for(&store, "group", "codex").unwrap().unwrap();
        assert_eq!(resolved.model, "gpt-5.4");
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(resolved.service_tier.as_deref(), Some("priority"));

        // Disable group_follow_private -> group resolution returns group config.
        set_group_follow_private(&store, false).unwrap();
        let resolved_group = resolve_for(&store, "group", "codex").unwrap().unwrap();
        assert_eq!(resolved_group.model, "gpt-5.4-mini");
        assert_eq!(resolved_group.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(resolved_group.service_tier, None);

        // Clearing all fields removes the row cleanly.
        save_entry(&store, "group", "codex", "", None, Some("default")).unwrap();
        assert!(resolve_for(&store, "group", "codex").unwrap().is_none());

        // Survives reopen.
        let reopened = Store::open(path).unwrap();
        let cfg = get_config(&reopened).unwrap();
        assert!(!cfg.group_follow_private);
        assert_eq!(cfg.entries.len(), 1);
        assert_eq!(cfg.entries[0].agent_id, "codex");
    }
}
