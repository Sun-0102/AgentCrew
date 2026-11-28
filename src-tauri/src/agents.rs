use crate::{deepseek, providers, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

pub const PROVIDERS: &[(&str, &str)] = &[
    ("codex", "codex-app-server"),
    ("deepseek", "deepseek-harness-sdk"),
    ("gemini", "gemini-cli-acp"),
    ("claude", "claude-code-cli"),
];

#[derive(Serialize)]
pub struct Agent {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub runtime_kind: String,
    pub runtime_label: String,
    pub registration_state: String,
    pub availability: String,
    pub enabled: bool,
    pub runtime_version: Option<String>,
    pub executable_path: Option<String>,
    pub capabilities: Value,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct DiscoveryItem {
    pub provider_kind: String,
    pub display_name: String,
    pub runtime_kind: String,
    pub runtime_label: String,
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub auth: Option<String>,
    pub model_count: usize,
    pub capabilities: Value,
    pub message: String,
    pub auth_hint: Option<String>,
}

pub fn default_display_name(kind: &str) -> &'static str {
    providers::driver(kind)
        .map(|d| d.display_name)
        .unwrap_or("Agent")
}

pub fn runtime_kind(provider: &str) -> Option<&'static str> {
    providers::driver(provider).map(|d| d.runtime_kind)
}

pub fn supports_runtime(provider: &str, runtime: &str) -> bool {
    runtime_kind(provider) == Some(runtime)
}

pub fn capabilities(provider: &str, probe: &Value) -> Value {
    providers::resolve(provider)
        .map(|adapter| adapter.capabilities(probe))
        .unwrap_or_else(|_| json!({}))
}

pub fn session_table(provider: &str) -> Result<&'static str, String> {
    Ok(providers::resolve(provider)?.driver().session_table)
}

pub fn supported(provider: &str) -> bool {
    providers::driver(provider).is_some()
}

pub fn list(store: &Store) -> Result<Vec<Agent>, String> {
    let connection = store.connect()?;
    let mut statement = connection
        .prepare("SELECT id,display_name,provider_kind,registration_state,availability,enabled,runtime_version,error,runtime_kind,executable_path,capabilities FROM agents ORDER BY created_at,id")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |r| {
            let provider_kind: String = r.get(2)?;
            let runtime_kind: String = r.get(8)?;
            let runtime_label = providers::driver(&provider_kind)
                .map(|d| d.runtime_label.to_owned())
                .unwrap_or_else(|| runtime_kind.clone());
            let caps_raw: Option<String> = r.get(10)?;
            let capabilities = caps_raw
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or_else(|| json!({}));
            Ok(Agent {
                id: r.get(0)?,
                display_name: r.get(1)?,
                provider_kind,
                runtime_kind,
                runtime_label,
                registration_state: r.get(3)?,
                availability: r.get(4)?,
                enabled: r.get(5)?,
                runtime_version: r.get(6)?,
                executable_path: r.get(9)?,
                capabilities,
                error: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn provider(store: &Store, id: &str) -> Result<String, String> {
    store
        .connect()?
        .query_row("SELECT provider_kind FROM agents WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(|_| "Agent 不存在。".into())
}

pub fn ensure_enabled(store: &Store, id: &str) -> Result<String, String> {
    let connection = store.connect()?;
    let kind: Option<String> = connection
        .query_row(
            "SELECT provider_kind FROM agents WHERE id=?1 AND enabled=1 AND registration_state='active'",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let kind = kind.ok_or("Agent 尚未接入、已停用或已移除。")?;
    if !supported(&kind) {
        return Err("provider_not_supported：AgentCrew 尚未适配此运行组件。".into());
    }
    Ok(kind)
}

pub fn label(store: &Store, id: &str) -> Result<String, String> {
    store
        .connect()?
        .query_row("SELECT display_name FROM agents WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())
}

// Explicit IDs make mentions stable even when display names are duplicated or renamed.
pub fn mention_targets(store: &Store, members: &[String]) -> Result<Vec<(String, String)>, String> {
    let agents = list(store)?;
    let mut targets = Vec::new();
    for id in members {
        targets.push((format!("@{id}"), id.clone()));
        if let Some(agent) = agents.iter().find(|a| &a.id == id) {
            if agents
                .iter()
                .filter(|a| {
                    members.contains(&a.id)
                        && a.display_name.eq_ignore_ascii_case(&agent.display_name)
                })
                .count()
                == 1
            {
                targets.push((format!("@{}", agent.display_name), id.clone()));
            }
        }
    }
    Ok(targets)
}

pub fn private_conversation(
    store: &Store,
    id: &str,
    project: Option<&str>,
) -> Result<String, String> {
    let kind = provider(store, id)?;
    if id == "codex" {
        return store.conversation_id(project);
    }
    if id == "deepseek" && project.is_none() {
        return Ok(deepseek::PERSONAL.to_owned());
    }
    let conversation = format!("agent:{id}:{}", project.unwrap_or("personal"));
    let root = if let Some(project) = project {
        store.conversation_root(project)?
    } else {
        let root = store.chat_workspace_parent()?.join("agents").join(id);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        root.to_string_lossy().into_owned()
    };
    store
        .connect()?
        .execute(
            "INSERT OR IGNORE INTO conversations(id,workspace_path) VALUES(?1,?2)",
            params![conversation, root],
        )
        .map_err(|e| e.to_string())?;
    let _ = kind;
    Ok(conversation)
}

pub fn binding(store: &Store, id: &str, conversation: &str) -> Result<String, String> {
    ensure_enabled(store, id)?;
    if matches!(id, "codex" | "deepseek") {
        return Ok(conversation.to_owned());
    }
    let root = store.conversation_root(conversation)?;
    let mut connection = store.connect()?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT runtime_conversation_id,workspace_path FROM agent_sessions WHERE agent_id=?1 AND conversation_id=?2",
            params![id, conversation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((runtime, expected)) = existing {
        if expected != root {
            return Err("成员会话工作目录已变更，请重新建立运行绑定。".into());
        }
        return Ok(runtime);
    }
    let runtime = format!("runtime:{}", uuid::Uuid::new_v4());
    tx.execute(
        "INSERT INTO conversations(id,workspace_path) VALUES(?1,?2)",
        params![runtime, root],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO agent_sessions(agent_id,conversation_id,runtime_conversation_id,workspace_path) VALUES(?1,?2,?3,?4)",
        params![id, conversation, runtime, root],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(runtime)
}

pub fn logical_identity(store: &Store, runtime: &str) -> Result<Option<(String, String)>, String> {
    store
        .connect()?
        .query_row(
            "SELECT agent_id,conversation_id FROM agent_sessions WHERE runtime_conversation_id=?1",
            [runtime],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())
}

pub fn session(
    app: &tauri::AppHandle,
    store: &Store,
    id: &str,
    conversation: &str,
) -> Result<Value, String> {
    let kind = provider(store, id)?;
    let runtime = if matches!(id, "codex" | "deepseek") {
        conversation.to_owned()
    } else {
        let existing: Option<String> = store
            .connect()?
            .query_row(
                "SELECT runtime_conversation_id FROM agent_sessions WHERE agent_id=?1 AND conversation_id=?2",
                params![id, conversation],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(runtime) = existing else {
            return Ok(Value::Null);
        };
        runtime
    };
    match providers::resolve(&kind) {
        Ok(adapter) => adapter.session(app, store, &runtime),
        Err(_) => Ok(Value::Null),
    }
}

pub fn send(
    app: &tauri::AppHandle,
    store: &Store,
    id: &str,
    conversation: &str,
    model: &str,
    text: &str,
    role: &str,
) -> Result<Value, String> {
    send_with_context(app, store, id, conversation, model, text, role, None)
}

pub fn send_with_context(
    app: &tauri::AppHandle,
    store: &Store,
    id: &str,
    conversation: &str,
    model: &str,
    text: &str,
    role: &str,
    extra_context: Option<&str>,
) -> Result<Value, String> {
    use tauri::Manager;
    if app
        .state::<crate::lifecycle::Lifecycle>()
        .exiting
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("应用正在退出，未启动新任务。".into());
    }
    let kind = ensure_enabled(store, id)?;
    let runtime = binding(store, id, conversation)?;
    let adapter = providers::resolve(&kind)?;
    let result = adapter.send_with_context(app, store, &runtime, model, text, role, extra_context);
    // Diagnostics must not turn an external launch into an automatic retry.
    if result.is_ok() {
        if let Ok(connection) = store.connect() {
            let _ = connection.execute(
                "UPDATE agents SET availability='ready',error=NULL WHERE id=?1",
                [id],
            );
        }
    } else if let Ok(connection) = store.connect() {
        let _ = connection.execute(
            "UPDATE agents SET availability='degraded',error='执行启动未完成，请查看会话错误并处理。' WHERE id=?1",
            [id],
        );
    }
    result
}

pub fn cancel(
    app: &tauri::AppHandle,
    store: &Store,
    id: &str,
    conversation: &str,
) -> Result<Value, String> {
    let kind = provider(store, id)?;
    let runtime = binding(store, id, conversation)?;
    providers::resolve(&kind)?.cancel(app, store, &runtime)
}

pub fn probe(app: &tauri::AppHandle, store: &Store, kind: &str) -> Result<Value, String> {
    providers::resolve(kind)?.probe(app, store)
}

pub fn record_probe(store: &Store, id: &str, result: &Result<Value, String>) -> Result<(), String> {
    match result {
        Ok(probe) => {
            let kind = provider(store, id)?;
            let caps = capabilities(&kind, probe);
            let needs_auth = probe["requires_auth"] == true && probe["auth"].is_null();
            let auth_hint = providers::driver(&kind)
                .map(|d| d.auth_hint)
                .unwrap_or("请先完成原厂登录；认证与额度以真实执行为准。");
            store.connect()?.execute(
                "UPDATE agents SET availability=?5,runtime_version=?2,executable_path=?3,capabilities=?4,error=?6,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                params![
                    id,
                    probe["version"].as_str(),
                    probe["executable"].as_str(),
                    caps.to_string(),
                    if needs_auth { "degraded" } else { "ready" },
                    if needs_auth { Some(auth_hint) } else { None }
                ],
            ).map_err(|e| e.to_string())?;
        }
        Err(_) => {
            store.connect()?.execute(
                "UPDATE agents SET availability='unavailable',error='运行组件检查失败，请在连接设置中重试。' WHERE id=?1",
                [id],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

enum ProbeOutcome {
    Busy,
    Probed(Result<Value, String>),
}

/// Parallel provider discovery modeled after T3 Code's provider probing flow.
/// Each registered driver probes concurrently in its own scoped thread without
/// blocking other providers.
pub fn auto_discover(
    app: &tauri::AppHandle,
    store: &Store,
    auto_enable: bool,
) -> Result<Vec<DiscoveryItem>, String> {
    let drivers = providers::all();
    let probe_results: Vec<(&'static dyn providers::ProviderAdapter, ProbeOutcome)> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = drivers
                .iter()
                .map(|&adapter| {
                    scope.spawn(move || {
                        if adapter.is_busy(store) {
                            (adapter, ProbeOutcome::Busy)
                        } else {
                            let res = adapter.probe(app, store);
                            (adapter, ProbeOutcome::Probed(res))
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        (
                            drivers[0],
                            ProbeOutcome::Probed(Err("探针线程异常退出".into())),
                        )
                    })
                })
                .collect()
        });

    let mut items = Vec::with_capacity(probe_results.len());
    for (adapter, outcome) in probe_results {
        let driver = adapter.driver();
        let kind = driver.provider_kind;
        let display_name = driver.display_name.to_owned();
        let runtime_kind = driver.runtime_kind.to_owned();
        let runtime_label = driver.runtime_label.to_owned();

        match outcome {
            ProbeOutcome::Busy => {
                let current = list(store)?.into_iter().find(|a| a.provider_kind == kind);
                let is_ready = current
                    .as_ref()
                    .map(|a| {
                        a.enabled && a.registration_state == "active" && a.availability == "ready"
                    })
                    .unwrap_or(false);
                items.push(DiscoveryItem {
                    provider_kind: kind.to_owned(),
                    display_name,
                    runtime_kind,
                    runtime_label,
                    status: if is_ready { "connected" } else { "unavailable" }.to_owned(),
                    version: current.as_ref().and_then(|a| a.runtime_version.clone()),
                    executable: current.as_ref().and_then(|a| a.executable_path.clone()),
                    auth: None,
                    model_count: 0,
                    capabilities: current
                        .map(|a| a.capabilities)
                        .unwrap_or_else(|| json!({})),
                    message: "当前有任务正在执行，保留现有连接状态。".to_owned(),
                    auth_hint: None,
                });
            }
            ProbeOutcome::Probed(result) => match result {
                Ok(ref info) => {
                    let needs_auth = info["requires_auth"] == true && info["auth"].is_null();
                    let availability = if needs_auth { "degraded" } else { "ready" };
                    let err_msg: Option<&str> = if needs_auth {
                        Some(driver.auth_hint)
                    } else {
                        None
                    };
                    let caps_val = adapter.capabilities(info);
                    let caps = caps_val.to_string();
                    let version = info["version"].as_str().map(str::to_owned);
                    let executable = info["executable"].as_str().map(str::to_owned);
                    let auth = info["auth"].as_str().map(str::to_owned);
                    let model_count = info["models"].as_array().map(|m| m.len()).unwrap_or(0);
                    {
                        let connection = store.connect()?;
                        let existing: Option<(String, bool)> = connection
                            .query_row(
                                "SELECT registration_state, enabled FROM agents WHERE id=?1",
                                [kind],
                                |r| Ok((r.get(0)?, r.get(1)?)),
                            )
                            .optional()
                            .map_err(|e| e.to_string())?;
                        match existing {
                            None => {
                                let has_other_active: bool = connection
                                    .query_row(
                                        "SELECT EXISTS(SELECT 1 FROM agents WHERE provider_kind=?1 AND registration_state='active' AND enabled=1)",
                                        [kind],
                                        |r| r.get(0),
                                    )
                                    .map_err(|e| e.to_string())?;
                                if !has_other_active {
                                    connection
                                        .execute(
                                            "INSERT INTO agents(id,display_name,provider_kind,runtime_kind,registration_state,availability,enabled,source,runtime_version,executable_path,capabilities,error,updated_at) VALUES(?1,?2,?3,?4,'active',?5,1,'auto',?6,?7,?8,?9,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                                            params![
                                                kind,
                                                &display_name,
                                                kind,
                                                driver.runtime_kind,
                                                availability,
                                                version.as_deref(),
                                                executable.as_deref(),
                                                &caps,
                                                err_msg
                                            ],
                                        )
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                            Some((reg_state, enabled)) => {
                                if auto_enable || (reg_state == "active" && enabled) {
                                    connection
                                        .execute(
                                            "UPDATE agents SET registration_state='active',enabled=1,availability=?2,runtime_version=?3,executable_path=?4,capabilities=?5,error=?6,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                                            params![
                                                kind,
                                                availability,
                                                version.as_deref(),
                                                executable.as_deref(),
                                                &caps,
                                                err_msg
                                            ],
                                        )
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                        }
                    }
                    if let Ok(registry) = list(store) {
                        for agent in registry.iter().filter(|a| {
                            a.id != kind
                                && a.provider_kind == kind
                                && a.enabled
                                && a.registration_state == "active"
                        }) {
                            let _ = record_probe(store, &agent.id, &result);
                        }
                    }
                    items.push(DiscoveryItem {
                        provider_kind: kind.to_owned(),
                        display_name,
                        runtime_kind,
                        runtime_label,
                        status: if needs_auth { "needs_auth" } else { "connected" }.to_owned(),
                        version,
                        executable,
                        auth,
                        model_count,
                        capabilities: caps_val,
                        message: if needs_auth {
                            "已检测到本机组件，需先完成原厂登录".to_owned()
                        } else if model_count > 0 {
                            format!("已就绪 · {model_count} 个模型")
                        } else {
                            "已就绪".to_owned()
                        },
                        auth_hint: if needs_auth {
                            Some(driver.auth_hint.to_owned())
                        } else {
                            None
                        },
                    });
                }
                Err(_) => {
                    if let Ok(registry) = list(store) {
                        for agent in registry.iter().filter(|a| {
                            a.provider_kind == kind
                                && a.enabled
                                && a.registration_state == "active"
                        }) {
                            let _ = record_probe(store, &agent.id, &result);
                        }
                    }
                    items.push(DiscoveryItem {
                        provider_kind: kind.to_owned(),
                        display_name,
                        runtime_kind,
                        runtime_label,
                        status: "unavailable".to_owned(),
                        version: None,
                        executable: None,
                        auth: None,
                        model_count: 0,
                        capabilities: json!({}),
                        message: "未检测到本机可用组件".to_owned(),
                        auth_hint: None,
                    });
                }
            },
        }
    }
    Ok(items)
}

pub fn clear_bindings(connection: &rusqlite::Connection, conversation: &str) -> Result<(), String> {
    let mut statement = connection
        .prepare("SELECT runtime_conversation_id FROM agent_sessions WHERE conversation_id=?1")
        .map_err(|e| e.to_string())?;
    let ids = statement
        .query_map([conversation], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(statement);
    connection
        .execute(
            "DELETE FROM agent_sessions WHERE conversation_id=?1",
            [conversation],
        )
        .map_err(|e| e.to_string())?;
    for id in ids {
        for adapter in providers::all() {
            connection
                .execute(
                    &format!(
                        "DELETE FROM {} WHERE conversation_id=?1",
                        adapter.driver().session_table
                    ),
                    [&id],
                )
                .map_err(|e| e.to_string())?;
        }
        for table in [
            "codex_messages",
            "reasoning_preferences",
            "speed_preferences",
        ] {
            connection
                .execute(
                    &format!("DELETE FROM {table} WHERE conversation_id=?1"),
                    [&id],
                )
                .map_err(|e| e.to_string())?;
        }
        connection
            .execute("DELETE FROM conversations WHERE id=?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
