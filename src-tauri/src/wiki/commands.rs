use super::*;
use tauri::Manager;

#[tauri::command]
pub fn wiki_settings_get(store: tauri::State<'_, Store>) -> Result<WikiSettings, String> {
    load_settings(&store)
}

#[tauri::command]
pub fn wiki_settings_save(
    store: tauri::State<'_, Store>,
    settings: WikiSettings,
) -> Result<WikiSettings, String> {
    save_settings(&store, &settings)
}

#[tauri::command]
pub async fn wiki_test_connection(
    base_url: String,
    api_token: String,
) -> Result<WikiConnectionTest, String> {
    tauri::async_runtime::spawn_blocking(move || test_connection(&base_url, &api_token))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn wiki_sync_projects(app: tauri::AppHandle) -> Result<Vec<WikiSource>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        sync_llm_wiki_projects(&store)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn wiki_source_list(store: tauri::State<'_, Store>) -> Result<Vec<WikiSource>, String> {
    list_sources(&store)
}

#[tauri::command]
pub async fn wiki_source_add_local(
    app: tauri::AppHandle,
    root_path: String,
    name: Option<String>,
    tier: Option<String>,
    content_subdir: Option<String>,
) -> Result<WikiSource, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        add_local_source(
            &store,
            &root_path,
            name.as_deref(),
            tier.as_deref(),
            content_subdir.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn wiki_source_remove(
    store: tauri::State<'_, Store>,
    source_id: String,
) -> Result<(), String> {
    remove_source(&store, &source_id)
}

#[tauri::command]
pub async fn wiki_source_reindex(
    app: tauri::AppHandle,
    source_id: String,
    force: Option<bool>,
) -> Result<IndexStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        index_source(&store, &source_id, force.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn wiki_habit_list(store: tauri::State<'_, Store>) -> Result<Vec<WikiHabitPage>, String> {
    list_habit_pages(&store)
}

#[tauri::command]
pub fn wiki_habit_set(
    store: tauri::State<'_, Store>,
    source_id: String,
    rel_path: String,
    title: String,
    enabled: bool,
) -> Result<Vec<WikiHabitPage>, String> {
    set_habit_page(&store, &source_id, &rel_path, &title, enabled)
}

#[tauri::command]
pub fn wiki_binding_get(
    store: tauri::State<'_, Store>,
    conversation_id: String,
) -> Result<Option<WikiBinding>, String> {
    get_binding(&store, &conversation_id)
}

#[tauri::command]
pub fn wiki_binding_set(
    store: tauri::State<'_, Store>,
    conversation_id: String,
    source_id: Option<String>,
    source_ids: Option<Vec<String>>,
    domain_filter: Option<String>,
    include_archives: Option<bool>,
) -> Result<Option<WikiBinding>, String> {
    let resolved_source = if source_id.as_deref() == Some(BINDING_NONE_SENTINEL) {
        Some(BINDING_NONE_SENTINEL.to_owned())
    } else if let Some(ids) = source_ids {
        encode_binding_source_ids(&ids)
    } else {
        source_id
    };
    set_binding(
        &store,
        &conversation_id,
        resolved_source.as_deref(),
        domain_filter.as_deref(),
        include_archives.unwrap_or(false),
    )
}

#[tauri::command]
pub async fn wiki_search(
    app: tauri::AppHandle,
    query: String,
    source_id: Option<String>,
    source_ids: Option<Vec<String>>,
    domain_filter: Option<String>,
    include_archives: Option<bool>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<WikiSearchHit>, String> {
    let resolved_source = match source_ids {
        Some(ids) if !ids.is_empty() => encode_binding_source_ids(&ids),
        _ => source_id,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        search_fts(
            &store,
            resolved_source.as_deref(),
            &query,
            domain_filter.as_deref(),
            include_archives.unwrap_or(false),
            false,
            limit.unwrap_or(20),
            offset.unwrap_or(0),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wiki_read_doc(
    app: tauri::AppHandle,
    source_id: Option<String>,
    rel_path: String,
    section: Option<String>,
) -> Result<WikiReadResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        read_document(&store, source_id.as_deref(), &rel_path, section.as_deref(), None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wiki_archive_note(
    app: tauri::AppHandle,
    source_id: String,
    title: String,
    content: String,
    tags: Option<Vec<String>>,
    source_conversation: Option<String>,
) -> Result<ArchiveNoteResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        archive_note(
            &store,
            &source_id,
            &title,
            &content,
            &tags.unwrap_or_default(),
            source_conversation.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
