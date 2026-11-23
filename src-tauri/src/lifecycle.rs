use crate::storage::Store;
use std::{
    fs::File,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

pub struct Lifecycle {
    pub exiting: AtomicBool,
    pub tray_ready: AtomicBool,
    _lock: File,
}

impl Lifecycle {
    pub fn acquire(directory: &std::path::Path) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("instance.lock"))
            .map_err(|e| e.to_string())?;
        file.try_lock()
            .map_err(|_| "此数据目录已有 AgentCrew 实例运行，请打开已有窗口。".to_owned())?;
        Ok(Self {
            exiting: AtomicBool::new(false),
            tray_ready: AtomicBool::new(false),
            _lock: file,
        })
    }
}

pub fn pause(app: &tauri::AppHandle, paused: bool) -> Result<(), String> {
    let gate = app.state::<crate::deepseek::ExecutionGate>();
    let _guard = gate.0.lock().map_err(|e| e.to_string())?;
    app.state::<Store>()
        .connect()?
        .execute("UPDATE app_controls SET paused=?1 WHERE id=1", [paused])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开 AgentCrew", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "暂停全部派发", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "继续全部派发", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "彻底退出（停止当前执行）", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &pause, &resume, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("AgentCrew · 本机协作")
        .title("AgentCrew")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "pause" | "resume" => {
                let paused = event.id.as_ref() == "pause";
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = self::pause(&app, paused);
                });
            }
            "quit" => {
                app.state::<Lifecycle>()
                    .exiting
                    .store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.state::<Lifecycle>()
        .tray_ready
        .store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn dispatch_control(app: tauri::AppHandle, paused: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || pause(&app, paused))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn dispatch_snapshot(store: tauri::State<'_, Store>) -> Result<bool, String> {
    store
        .connect()?
        .query_row("SELECT paused FROM app_controls WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())
}

pub fn shutdown(app: &tauri::AppHandle) {
    app.state::<Lifecycle>()
        .exiting
        .store(true, Ordering::SeqCst);
    let gate = app.state::<crate::deepseek::ExecutionGate>();
    let _guard = gate.0.lock().unwrap_or_else(|e| e.into_inner());
    app.state::<crate::codex::Runtime>().shutdown();
    app.state::<crate::deepseek::Runtime>().shutdown();
    app.state::<crate::gemini::Runtime>().shutdown();
    let _ = crate::collaboration::recover(&app.state::<Store>());
}
