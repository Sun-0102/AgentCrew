use crate::storage::{Project, Store};
use serde::Serialize;
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
