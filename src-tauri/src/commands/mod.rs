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
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
) -> Result<Value, String> {
    let admission = app.clone();
    codex_call(app, move |runtime, store| {
        let gate = admission.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        if target_agent
            .as_deref()
            .is_some_and(|target| target != "codex")
        {
            return Err("Codex 接口只能执行 Codex 任务。".into());
        }
        let conversation = store.resolve_chat(project_id.as_deref(), group_id.as_deref())?;
        crate::reasoning::save(
            store,
            &conversation,
            "codex",
            &model,
            reasoning_effort.as_deref(),
        )?;
        crate::speed::save(store, &conversation, &model, service_tier.as_deref())?;
        if let Some(id) = group_id.as_deref() {
            let other = crate::collaboration::latest(store, id)?
                .map(|task| task.deepseek_model)
                .filter(|model| !model.is_empty())
                .unwrap_or_else(|| String::new());
            let other = if other.is_empty() {
                crate::deepseek::session(store, id)?["model"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
            } else {
                other
            };
            return crate::collaboration::start(
                &admission, store, id, "codex", &model, &other, &text,
            );
        }
        crate::deepseek::ensure_idle(store)?;
        crate::collaboration::manual_send(store, &model, &text)?;
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
    reasoning_effort: Option<String>,
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
        let conversation = group_id.as_deref().unwrap_or(crate::deepseek::PERSONAL);
        crate::reasoning::save(
            store,
            conversation,
            "deepseek",
            &model,
            reasoning_effort.as_deref(),
        )?;
        if let Some(id) = group_id {
            let other = crate::collaboration::latest(store, &id)?
                .map(|task| task.codex_model)
                .filter(|model| !model.is_empty())
                .unwrap_or_else(|| String::new());
            let other = if other.is_empty() {
                admission.state::<Runtime>().snapshot(store, &id)?["session"]["model"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
            } else {
                other
            };
            crate::collaboration::start(&admission, store, &id, "deepseek", &other, &model, &text)
        } else {
            crate::deepseek::ensure_idle(store)?;
            crate::collaboration::manual_send(store, &model, &text)?;
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
    let handle = app.clone();
    deepseek_call(app, move |runtime, store| {
        if let Some(id) = group_id {
            let gate = handle.state::<crate::deepseek::ExecutionGate>();
            let _guard = gate.0.lock().map_err(|e| e.to_string())?;
            if let Some(task) = crate::collaboration::latest(store, &id)? {
                if matches!(task.status.as_str(), "running" | "paused" | "cancelling") {
                    return crate::collaboration::control(&handle, store, &id, &task.id, "cancel");
                }
            }
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
            "SELECT agent FROM codex_messages WHERE conversation_id=?1 AND role IN ('user','assignment') ORDER BY sequence DESC LIMIT 1",
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
        snapshot["task"] = serde_json::to_value(crate::collaboration::latest(store, &id)?).map_err(|e| e.to_string())?;
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
    let handle = app.clone();
    codex_call(app, move |runtime, store| {
        if let Some(id) = group_id.as_deref() {
            let gate = handle.state::<crate::deepseek::ExecutionGate>();
            let _guard = gate.0.lock().map_err(|e| e.to_string())?;
            if let Some(task) = crate::collaboration::latest(store, id)? {
                if matches!(task.status.as_str(), "running" | "paused" | "cancelling") {
                    return crate::collaboration::control(&handle, store, id, &task.id, "cancel");
                }
            }
        }
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
    project_id: Option<String>,
) -> Result<Value, String> {
    let admission = app.clone();
    codex_call(app, move |_, store| {
        let gate = admission.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        serde_json::to_value(store.update_group(&id, &name, members, project_id)?)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn group_task_start(
    app: tauri::AppHandle,
    group_id: String,
    target_agent: String,
    codex_model: String,
    deepseek_model: String,
    text: String,
    codex_reasoning_effort: Option<String>,
    deepseek_reasoning_effort: Option<String>,
    codex_service_tier: Option<String>,
) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        store.group(&group_id)?;
        crate::reasoning::save(
            store,
            &group_id,
            "codex",
            &codex_model,
            codex_reasoning_effort.as_deref(),
        )?;
        crate::reasoning::save(
            store,
            &group_id,
            "deepseek",
            &deepseek_model,
            deepseek_reasoning_effort.as_deref(),
        )?;
        crate::speed::save(
            store,
            &group_id,
            &codex_model,
            codex_service_tier.as_deref(),
        )?;
        crate::collaboration::start(
            &handle,
            store,
            &group_id,
            &target_agent,
            &codex_model,
            &deepseek_model,
            &text,
        )
    })
    .await
}

#[tauri::command]
pub async fn group_task_control(
    app: tauri::AppHandle,
    group_id: String,
    task_id: String,
    action: String,
) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        crate::collaboration::control(&handle, store, &group_id, &task_id, &action)
    })
    .await
}

#[tauri::command]
pub async fn group_task_list(app: tauri::AppHandle, group_id: String) -> Result<Value, String> {
    codex_call(app, move |_, store| crate::task_results::list(store, &group_id)).await
}

#[tauri::command]
pub async fn group_task_result(app: tauri::AppHandle, group_id: String, task_id: String) -> Result<Value, String> {
    codex_call(app, move |_, store| crate::task_results::result(store, &group_id, &task_id)).await
}

#[tauri::command]
pub async fn group_task_export(app: tauri::AppHandle, group_id: String, task_id: String, path: String, format: String) -> Result<Value, String> {
    codex_call(app, move |_, store| {
        crate::task_results::export(store, &group_id, &task_id, &path, &format)?;
        Ok(Value::Null)
    }).await
}
