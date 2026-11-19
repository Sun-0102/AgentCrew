use crate::codex::Runtime;
use crate::storage::{Project, Store};
use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;
use tauri::State;

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    database_path: String,
}

#[tauri::command]
pub fn app_info(store: State<'_, Store>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        database_path: store.path.to_string_lossy().into_owned(),
    }
}

#[tauri::command]
pub async fn project_list(store: State<'_, Store>) -> Result<Vec<Project>, String> {
    let path = store.path.clone();
    tauri::async_runtime::spawn_blocking(move || Store::open(path)?.list_projects())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn project_add(path: String, store: State<'_, Store>) -> Result<Project, String> {
    let database_path = store.path.clone();
    tauri::async_runtime::spawn_blocking(move || Store::open(database_path)?.add_project(&path))
        .await
        .map_err(|error| error.to_string())?
}

async fn codex_call(
    app: tauri::AppHandle,
    action: impl FnOnce(&Runtime, &Store) -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        action(&app.state::<Runtime>(), &app.state::<Store>())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn codex_probe(app: tauri::AppHandle) -> Result<Value, String> {
    codex_call(app, |runtime, store| runtime.probe(store)).await
}

#[tauri::command]
pub async fn codex_send(
    app: tauri::AppHandle,
    project_id: Option<String>,
    group_id: Option<String>,
    target_agent: Option<String>,
    model: String,
    text: String,
) -> Result<Value, String> {
    let admission = app.clone();
    codex_call(app, move |runtime, store| {
        let gate = admission.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        crate::deepseek::ensure_idle(store)?;
        if target_agent
            .as_deref()
            .is_some_and(|target| target != "codex")
        {
            return Err("Codex 接口只能执行 Codex 任务。".into());
        }
        if let Some(id) = group_id.as_deref() {
            store.validate_group_target(id, target_agent.as_deref().unwrap_or("codex"))?;
        }
        runtime.send(
            store,
            &store.resolve_chat(project_id.as_deref(), group_id.as_deref())?,
            &model,
            &text,
        )
    })
    .await
}

async fn deepseek_call(
    app: tauri::AppHandle,
    action: impl FnOnce(&crate::deepseek::Runtime, &Store) -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        action(
            &app.state::<crate::deepseek::Runtime>(),
            &app.state::<Store>(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_probe(app: tauri::AppHandle) -> Result<Value, String> {
    deepseek_call(app, |runtime, store| runtime.probe(store)).await
}

#[tauri::command]
pub async fn deepseek_snapshot(
    app: tauri::AppHandle,
    group_id: Option<String>,
) -> Result<Value, String> {
    if let Some(id) = group_id {
        return group_snapshot(app, id, "deepseek").await;
    }
    deepseek_call(app, |runtime, store| runtime.snapshot(store)).await
}

#[tauri::command]
pub async fn deepseek_send(
    app: tauri::AppHandle,
    model: String,
    text: String,
    group_id: Option<String>,
    target_agent: Option<String>,
) -> Result<Value, String> {
    let admission = app.clone();
    deepseek_call(app, move |runtime, store| {
        let gate = admission.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        if target_agent
            .as_deref()
            .is_some_and(|target| target != "deepseek")
        {
            return Err("DeepSeek 接口只能执行 DeepSeek 任务。".into());
        }
        if let Some(id) = group_id {
            store.validate_group_target(&id, "deepseek")?;
            runtime.send_to(store, &id, &model, &text)
        } else {
            runtime.send(store, &model, &text)
        }
    })
    .await
}

#[tauri::command]
pub async fn deepseek_cancel(
    app: tauri::AppHandle,
    group_id: Option<String>,
) -> Result<Value, String> {
    deepseek_call(app, move |runtime, store| {
        if let Some(id) = group_id {
            store.group(&id)?;
            runtime.cancel_in(store, &id)
        } else {
            runtime.cancel(store)
        }
    })
    .await
}

async fn group_snapshot(
    app: tauri::AppHandle,
    id: String,
    selected: &'static str,
) -> Result<Value, String> {
    codex_call(app, move |runtime, store| {
        store.group(&id)?;
        let mut snapshot = runtime.snapshot(store, &id)?;
        let deepseek = crate::deepseek::session(store, &id)?;
        let last_agent: Option<String> = store.connect()?.query_row(
            "SELECT agent FROM codex_messages WHERE conversation_id=?1 AND role='user' ORDER BY sequence DESC LIMIT 1",
            [&id], |row| row.get(0),
        ).optional().map_err(|e| e.to_string())?;
        let codex_active = snapshot["session"]["status"].as_str().is_some_and(crate::codex::active);
        let deepseek_active = deepseek["status"].as_str().is_some_and(crate::codex::active);
        if deepseek_active || (!codex_active && last_agent.as_deref().unwrap_or(selected) == "deepseek") {
            snapshot["session"] = deepseek;
            snapshot["approvals"] = serde_json::json!([]);
            snapshot["agent"] = serde_json::json!("deepseek");
        } else {
            snapshot["agent"] = serde_json::json!("codex");
        }
        if snapshot["session"].is_object() {
            snapshot["session"]["workspace_path"] = serde_json::json!(store.conversation_root(&id)?);
        }
        Ok(snapshot)
    }).await
}

#[tauri::command]
pub async fn codex_snapshot(
    app: tauri::AppHandle,
    project_id: Option<String>,
    group_id: Option<String>,
) -> Result<Value, String> {
    if let Some(id) = group_id {
        return group_snapshot(app, id, "codex").await;
    }
    codex_call(app, move |runtime, store| {
        runtime.snapshot(store, &store.resolve_chat(project_id.as_deref(), None)?)
    })
    .await
}

#[tauri::command]
pub async fn codex_cancel(
    app: tauri::AppHandle,
    project_id: Option<String>,
    group_id: Option<String>,
) -> Result<Value, String> {
    codex_call(app, move |runtime, store| {
        runtime.cancel(
            store,
            &store.resolve_chat(project_id.as_deref(), group_id.as_deref())?,
        )
    })
    .await
}

#[tauri::command]
pub async fn codex_approve(
    app: tauri::AppHandle,
    project_id: Option<String>,
    group_id: Option<String>,
    id: String,
    allow: bool,
) -> Result<Value, String> {
    codex_call(app, move |runtime, store| {
        runtime.approve(
            store,
            &store.resolve_chat(project_id.as_deref(), group_id.as_deref())?,
            &id,
            allow,
        )
    })
    .await
}

#[tauri::command]
pub async fn group_list(app: tauri::AppHandle) -> Result<Value, String> {
    codex_call(app, |_, store| {
        serde_json::to_value(store.groups()?).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn group_create(
    app: tauri::AppHandle,
    name: String,
    members: Vec<String>,
    project_id: Option<String>,
) -> Result<Value, String> {
    codex_call(app, move |_, store| {
        serde_json::to_value(store.create_group(&name, members, project_id)?)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn group_update(
    app: tauri::AppHandle,
    id: String,
    name: String,
    members: Vec<String>,
) -> Result<Value, String> {
    let admission = app.clone();
    codex_call(app, move |_, store| {
        let gate = admission.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        serde_json::to_value(store.update_group(&id, &name, members)?).map_err(|e| e.to_string())
    })
    .await
}
