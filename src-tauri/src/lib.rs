mod commands;
mod storage;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = storage::Store::open(data_dir.join("ai-gent.sqlite"))?;
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::project_list,
            commands::project_add
        ])
        .run(tauri::generate_context!())
        .expect("AI-gent 启动失败，请检查本地数据目录和桌面运行环境");
}
