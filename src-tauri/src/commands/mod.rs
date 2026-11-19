use crate::codex::Runtime;
use crate::storage::{Project, Store};
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
    codex_call(app, move |runtime, store| {
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

#[tauri::command]
pub async fn codex_snapshot(
    app: tauri::AppHandle,
    project_id: Option<String>,
    group_id: Option<String>,
) -> Result<Value, String> {
    codex_call(app, move |runtime, store| {
        runtime.snapshot(
            store,
            &store.resolve_chat(project_id.as_deref(), group_id.as_deref())?,
        )
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
    codex_call(app, move |_, store| {
        serde_json::to_value(store.update_group(&id, &name, members)?).map_err(|e| e.to_string())
    })
    .await
}
