use super::*;

#[tauri::command]
pub fn schedule_list(store: tauri::State<'_, Store>) -> Result<Vec<ScheduledTask>, String> {
    list(&store)
}

#[tauri::command]
pub fn schedule_save(
    store: tauri::State<'_, Store>,
    input: ScheduledTaskInput,
) -> Result<ScheduledTask, String> {
    save(&store, input)
}

#[tauri::command]
pub fn schedule_toggle(
    store: tauri::State<'_, Store>,
    id: String,
    enabled: bool,
) -> Result<ScheduledTask, String> {
    toggle(&store, &id, enabled)
}

#[tauri::command]
pub fn schedule_delete(store: tauri::State<'_, Store>, id: String) -> Result<(), String> {
    delete(&store, &id)
}

#[tauri::command]
pub async fn schedule_run_now(
    app: tauri::AppHandle,
    id: String,
) -> Result<ScheduledTaskRun, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let task = get(&store, &id)?;
        // Validate provider availability synchronously before returning
        if matches!(task.target_type.as_str(), "agent" | "project") {
            let agent_id = task.agent_id.as_deref().ok_or("任务未指定执行 Agent。")?;
            let kind = agents::ensure_enabled(&store, agent_id)?;
            providers::resolve(&kind)?;
        }
        Ok(dispatch_task(&app, &store, &task, "manual"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn schedule_runs(
    store: tauri::State<'_, Store>,
    task_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<ScheduledTaskRun>, String> {
    list_runs(&store, task_id.as_deref(), limit.unwrap_or(50))
}

#[tauri::command]
pub fn schedule_codex_scan(store: tauri::State<'_, Store>) -> Result<CodexScanResult, String> {
    let Some(path) = codex_db_path() else {
        return Ok(CodexScanResult {
            found: false,
            db_path: None,
            items: Vec::new(),
        });
    };
    scan_codex_in(&path, &store)
}

#[tauri::command]
pub fn schedule_codex_import(
    store: tauri::State<'_, Store>,
    ids: Vec<String>,
    target_agent_id: Option<String>,
    default_model: Option<String>,
    enable_imported: Option<bool>,
) -> Result<Vec<ScheduledTask>, String> {
    let path = codex_db_path().ok_or("未在本机找到 Codex 数据库（~/.codex/sqlite/codex.db）。")?;
    import_codex_in(
        &path,
        &store,
        &ids,
        target_agent_id.as_deref(),
        default_model.as_deref(),
        enable_imported.unwrap_or(false),
    )
}
