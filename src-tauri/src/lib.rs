mod agent_commands;
mod agents;
mod codex;
mod codex_sidebar;
mod collaboration;
mod commands;
mod deepseek;
mod gemini;
mod group_context;
mod group_sessions;
mod groups;
mod images;
mod lifecycle;
mod pairing;
mod private_sessions;
mod process;
mod reasoning;
mod skills;
mod speed;
mod storage;
mod task_results;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(lifecycle::Lifecycle::acquire(&data_dir)?);
            let store = storage::Store::open(data_dir.join("ai-gent.sqlite"))?;
            images::recover(&store)?;
            codex::recover(&store)?;
            deepseek::recover(&store)?;
            gemini::recover(&store)?;
            collaboration::recover(&store)?;
            pairing::recover(&store)?;
            app.manage(store);
            app.manage(codex::Runtime::default());
            app.manage(deepseek::Runtime::default());
            app.manage(gemini::Runtime::default());
            app.manage(deepseek::ExecutionGate::default());
            app.manage(std::sync::Arc::new(pairing::Service::default()));
            {
                let service=app.state::<std::sync::Arc<pairing::Service>>();
                app.state::<storage::Store>().connect()?.execute("UPDATE app_controls SET instance_id=?1 WHERE id=1",[&service.instance])?;
            }
            let bootstrap=app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                {
                    let gate=bootstrap.state::<deepseek::ExecutionGate>();
                    let _guard=gate.0.lock().unwrap_or_else(|e|e.into_inner());
                    let store=bootstrap.state::<storage::Store>();
                    if let Ok(registry)=agents::list(&store) {
                        for (kind,_) in agents::PROVIDERS {
                            if registry.iter().any(|a|a.provider_kind==*kind && a.enabled && a.registration_state=="active") {
                                let result=agents::probe(&bootstrap,&store,kind);
                                for agent in registry.iter().filter(|a|a.provider_kind==*kind && a.enabled && a.registration_state=="active") {let _=agents::record_probe(&store,&agent.id,&result);}
                            }
                        }
                    }
                }
                collaboration::start_scheduler(&bootstrap);
            });
            if let Err(error)=pairing::start(app.handle()) {
                *app.state::<std::sync::Arc<pairing::Service>>().address.lock().unwrap_or_else(|e|e.into_inner())=Err(error);
            }
            // A failed tray must leave normal window closing available.
            let _=lifecycle::tray(app.handle());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let store = handle.state::<storage::Store>();
                if !agents::list(&store).map(|members| members.iter().any(|agent| agent.id == "codex" && agent.enabled && agent.registration_state == "active")).unwrap_or(false) {
                    return;
                }
                if let Err(error) = handle.state::<codex::Runtime>().sync_personal_sidebar(&store) {
                    if let Ok(connection) = store.connect() {
                        let _ = connection.execute("UPDATE codex_sessions SET error=COALESCE(error,?1) WHERE conversation_id='codex-personal'",[format!("同步官方最近列表失败：{error}")]);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window,event| {
            if let tauri::WindowEvent::CloseRequested {api,..}=event {
                let lifecycle=window.state::<lifecycle::Lifecycle>();
                if lifecycle.tray_ready.load(std::sync::atomic::Ordering::SeqCst) && !lifecycle.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                    if window.hide().is_ok() {api.prevent_close();}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            agent_commands::agent_list,
            agent_commands::agent_probe,
            agent_commands::agent_send,
            agent_commands::agent_snapshot,
            agent_commands::agent_cancel,
            agent_commands::agent_approve,
            agent_commands::agent_group_send,
            agent_commands::agent_update,
            agent_commands::pairing_create,
            agent_commands::pairing_snapshot,
            agent_commands::pairing_cancel,
            lifecycle::dispatch_control,
            lifecycle::dispatch_snapshot,
            commands::app_info,
            commands::skills_list,
            commands::skills_prepare,
            commands::image_read,
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
            commands::group_update,
            commands::group_session_list,
            commands::group_session_create,
            commands::group_session_update,
            commands::group_session_delete,
            commands::private_session_list,
            commands::private_session_create,
            commands::private_session_update,
            commands::private_session_delete,
            commands::group_members_update,
            commands::group_task_start,
            commands::group_task_control,
            commands::group_task_list,
            commands::group_task_result,
            commands::group_task_export
        ])
        .build(tauri::generate_context!())
        .expect("AgentCrew 启动失败，请检查本地数据目录和桌面运行环境")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                lifecycle::shutdown(app);
            }
        });
}
