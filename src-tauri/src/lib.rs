mod codex;
mod codex_sidebar;
mod commands;
mod deepseek;
mod groups;
mod storage;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = storage::Store::open(data_dir.join("ai-gent.sqlite"))?;
            codex::recover(&store)?;
            deepseek::recover(&store)?;
            app.manage(store);
            app.manage(codex::Runtime::default());
            app.manage(deepseek::Runtime::default());
            app.manage(deepseek::ExecutionGate::default());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let store = handle.state::<storage::Store>();
                if let Err(error) = handle.state::<codex::Runtime>().sync_personal_sidebar(&store) {
                    if let Ok(connection) = store.connect() {
                        let _ = connection.execute("UPDATE codex_sessions SET error=COALESCE(error,?1) WHERE conversation_id='codex-personal'",[format!("同步官方最近列表失败：{error}")]);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::project_list,
            commands::project_add,
            commands::codex_probe,
            commands::codex_send,
            commands::codex_snapshot,
            commands::codex_cancel,
            commands::codex_approve,
            commands::deepseek_probe,
            commands::deepseek_snapshot,
            commands::deepseek_send,
            commands::deepseek_cancel,
            commands::group_list,
            commands::group_create,
            commands::group_update
        ])
        .build(tauri::generate_context!())
        .expect("AI-gent 启动失败，请检查本地数据目录和桌面运行环境")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<codex::Runtime>().shutdown();
                app.state::<deepseek::Runtime>().shutdown();
            }
        });
}
