mod agent_commands;
mod agents;
mod attachments;
mod chat_queue;
mod claude;
mod codex;
mod codex_sidebar;
mod collaboration;
mod commands;
mod deepseek;
mod events;
mod gemini;
mod git;
mod group_context;
mod group_sessions;
mod groups;
mod images;
mod lifecycle;
mod local_files;
mod model_defaults;
mod pairing;
mod private_sessions;
mod process;
mod project_sessions;
mod providers;
mod reasoning;
mod schedules;
mod skills;
mod speed;
mod storage;
mod task_results;
mod usage;
mod wiki;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            events::init(app.handle().clone());
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(lifecycle::Lifecycle::acquire(&data_dir)?);
            let store = storage::Store::open(data_dir.join("ai-gent.sqlite"))?;
            images::recover(&store)?;
            codex::recover(&store)?;
            deepseek::recover(&store)?;
            gemini::recover(&store)?;
            claude::recover(&store)?;
            collaboration::recover(&store)?;
            chat_queue::recover(&store)?;
            schedules::recover(&store)?;
            pairing::recover(&store)?;
            app.manage(store);
            app.manage(codex::Runtime::default());
            app.manage(deepseek::Runtime::default());
            app.manage(gemini::Runtime::default());
            app.manage(claude::Runtime::default());
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
                    let _=agents::auto_discover(&bootstrap,&store,false);
                    events::emit_agents_sync();
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
            agent_commands::agent_auto_discover,
            agent_commands::agent_probe,
            agent_commands::agent_send,
            agent_commands::agent_snapshot,
            agent_commands::agent_cancel,
            agent_commands::agent_approve,
            agent_commands::agent_queue_update,
            agent_commands::agent_queue_remove,
            agent_commands::agent_queue_clear,
            agent_commands::agent_queue_resume,
            agent_commands::agent_queue_send_now,
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
            commands::attachment_save,
            commands::attachment_read,
            commands::project_list,
            commands::project_add,
            commands::project_rename,
            commands::project_remove,
            commands::project_open_directory,
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
            commands::group_delete,
            commands::group_session_list,
            commands::group_session_create,
            commands::group_session_update,
            commands::group_session_delete,
            commands::private_session_list,
            commands::private_session_create,
            commands::private_session_update,
            commands::private_session_delete,
            commands::project_session_list,
            commands::project_session_create,
            commands::project_session_update,
            commands::project_session_delete,
            commands::group_members_update,
            commands::group_task_start,
            commands::group_task_control,
            commands::group_task_list,
            commands::group_task_result,
            commands::group_task_export,
            commands::usage_dashboard,
            git::git_overview,
            git::git_file_diff,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_commit,
            git::git_push,
            git::git_checkout_branch,
            git::git_create_pr,
            git::git_worktree_create,
            git::git_worktree_remove,
            git::git_commits,
            git::git_commit_files,
            git::git_commit_file_diff,
            git::git_generate_commit_message,
            git::git_open_url,
            git::git_turn_file_diff,
            git::git_turn_revert,
            local_files::local_file_action,
            local_files::local_image_read,
            git::workspace_read_file,
            schedules::schedule_list,
            schedules::schedule_save,
            schedules::schedule_toggle,
            schedules::schedule_delete,
            schedules::schedule_run_now,
            schedules::schedule_runs,
            schedules::schedule_codex_scan,
            schedules::schedule_codex_import,
            model_defaults::model_defaults_get,
            model_defaults::model_defaults_save,
            model_defaults::model_defaults_set_group_follow,
            wiki::wiki_settings_get,
            wiki::wiki_settings_save,
            wiki::wiki_test_connection,
            wiki::wiki_sync_projects,
            wiki::wiki_source_list,
            wiki::wiki_source_add_local,
            wiki::wiki_source_remove,
            wiki::wiki_source_reindex,
            wiki::wiki_habit_list,
            wiki::wiki_habit_set,
            wiki::wiki_binding_get,
            wiki::wiki_binding_set,
            wiki::wiki_search,
            wiki::wiki_read_doc,
            wiki::wiki_archive_note
        ])
        .build(tauri::generate_context!())
        .expect("AgentCrew 启动失败，请检查本地数据目录和桌面运行环境")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                lifecycle::show_main_window(app);
            }
            tauri::RunEvent::Exit => {
                lifecycle::shutdown(app);
            }
            _ => {}
        });
}
