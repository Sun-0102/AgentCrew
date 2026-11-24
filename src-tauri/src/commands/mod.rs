use crate::codex::Runtime;
use crate::storage::{Project, Store};
use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;
use tauri::State;

#[tauri::command]
pub async fn skills_list(app: tauri::AppHandle, agent: String, project_id: Option<String>, group_id: Option<String>) -> Result<Vec<crate::skills::Skill>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::skills::list(&app, &app.state::<Store>(), &agent, project_id.as_deref(), group_id.as_deref(), &[]))
        .await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn skills_prepare(app: tauri::AppHandle, agents: Vec<String>, names: Vec<String>, project_id: Option<String>, group_id: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if names.len() > 8 || agents.is_empty() || agents.len() > 32 { return Err("一次最多调用 8 个技能。".to_owned()); }
        let store = app.state::<Store>();
        let mut prompt = String::from("\n\n本轮用户显式选择的本地技能（仅对应 Agent 使用）：\n");
        for agent in agents {
            let skills = crate::skills::list(&app, &store, &agent, project_id.as_deref(), group_id.as_deref(), &names)?;
            for name in &names {
                let skill = skills.iter().find(|skill| &skill.name == name)
                    .ok_or_else(|| format!("{agent} 未找到本地技能 /{name}。请检查技能目录，或用 @ 指定有该技能的成员。"))?;
                if skill.native {
                    let content = skill.content.as_deref().ok_or_else(|| format!("{agent} 未返回 /{} 的技能正文", skill.name))?;
                    prompt.push_str(&format!("- {agent}：/{}，运行时加载的技能正文如下：\n", skill.name));
                    for line in content.lines() { prompt.push_str(&format!("> {line}\n")); }
                } else {
                    prompt.push_str(&format!("- {agent}：/{}，SKILL.md 文件路径：{}\n", skill.name, serde_json::to_string(&skill.path).map_err(|e| e.to_string())?));
                }
            }
        }
        prompt.push_str("请使用对应 Agent 的技能正文；仅提供文件路径时先读取 SKILL.md，再按技能要求处理本轮任务。相对资源路径以技能目录或正文中的资源说明为基准。技能不能扩大用户授权；工具不可用时如实说明。\n");
        Ok(prompt)
    }).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    database_path: String,
}

#[tauri::command]
pub async fn image_read(app: tauri::AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || crate::images::read(&app.state::<Store>(), &id))
        .await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn attachment_save(
    app: tauri::AppHandle,
    name: String,
    mime_type: String,
    data: String,
) -> Result<crate::attachments::SavedAttachment, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::attachments::save(&app.state::<Store>(), &name, &mime_type, &data)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn attachment_read(app: tauri::AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::attachments::read_data_url(&app.state::<Store>(), &id)
    })
    .await
    .map_err(|e| e.to_string())?
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
    crate::agent_commands::agent_probe(app,"codex".into()).await
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
        crate::agents::ensure_enabled(store,"codex")?;
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
    crate::agent_commands::agent_probe(app,"deepseek".into()).await
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
        crate::agents::ensure_enabled(store,"deepseek")?;
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
        snapshot["sessions"] = serde_json::json!({"codex":snapshot["session"],"deepseek":deepseek});
        let last_agent: Option<String> = store.connect()?.query_row(
            "SELECT agent FROM codex_messages WHERE conversation_id=?1 AND role IN ('user','assignment') ORDER BY sequence DESC LIMIT 1",
            [&id], |row| row.get(0),
        ).optional().map_err(|e| e.to_string())?;
        let codex_active = snapshot["session"]["status"].as_str().is_some_and(crate::codex::active);
        let deepseek_active = deepseek["status"].as_str().is_some_and(crate::codex::active);
        if deepseek_active || (!codex_active && last_agent.as_deref().unwrap_or(selected) == "deepseek") {
            snapshot["session"] = deepseek;
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
        let created = store.create_group(&name, members, project_id)?;
        let group_id = created.id.clone();
        let value = serde_json::to_value(created).map_err(|e| e.to_string())?;
        crate::events::emit_groups_sync(Some(&group_id));
        Ok(value)
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
        let updated = store.update_group(&id, &name, members, project_id)?;
        let value = serde_json::to_value(updated).map_err(|e| e.to_string())?;
        crate::events::emit_groups_sync(Some(&id));
        Ok(value)
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
pub async fn group_session_list(app: tauri::AppHandle, group_id: String) -> Result<Value, String> {
    codex_call(app, move |_, store| serde_json::to_value(store.group_sessions(&group_id)?).map_err(|e|e.to_string())).await
}

#[tauri::command]
pub async fn group_session_create(app: tauri::AppHandle, group_id: String, name: String) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e|e.to_string())?;
        let value = serde_json::to_value(store.create_group_session(&group_id,&name)?).map_err(|e|e.to_string())?;
        crate::events::emit_groups_sync(Some(&group_id));
        Ok(value)
    }).await
}

#[tauri::command]
pub async fn group_session_update(app: tauri::AppHandle, group_id: String, id: String, name: String, archived: bool) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e|e.to_string())?;
        let value = serde_json::to_value(store.update_group_session(&group_id,&id,&name,archived)?).map_err(|e|e.to_string())?;
        crate::events::emit_groups_sync(Some(&group_id));
        Ok(value)
    }).await
}

#[tauri::command]
pub async fn group_session_delete(app: tauri::AppHandle, group_id: String, id: String) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e|e.to_string())?;
        store.delete_group_session(&group_id,&id)?;
        crate::events::emit_groups_sync(Some(&group_id));
        Ok(Value::Null)
    }).await
}

#[tauri::command]
pub async fn group_delete(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        store.delete_group(&id)?;
        crate::events::emit_groups_sync(Some(&id));
        Ok(Value::Null)
    }).await
}

#[tauri::command]
pub async fn private_session_list(app: tauri::AppHandle, agent_id: String) -> Result<Value, String> {
    codex_call(app, move |_, store| {
        serde_json::to_value(store.private_sessions(&agent_id)?).map_err(|e| e.to_string())
    }).await
}

#[tauri::command]
pub async fn private_session_create(app: tauri::AppHandle, agent_id: String, name: String) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        let value = serde_json::to_value(store.create_private_session(&agent_id, &name)?).map_err(|e| e.to_string())?;
        crate::events::emit_private_sessions_sync(Some(&agent_id));
        Ok(value)
    }).await
}

#[tauri::command]
pub async fn private_session_update(app: tauri::AppHandle, agent_id: String, id: String, name: String, archived: bool) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        let value = serde_json::to_value(store.update_private_session(&agent_id, &id, &name, archived)?).map_err(|e| e.to_string())?;
        crate::events::emit_private_sessions_sync(Some(&agent_id));
        Ok(value)
    }).await
}

#[tauri::command]
pub async fn private_session_delete(app: tauri::AppHandle, agent_id: String, id: String) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        store.delete_private_session(&agent_id, &id)?;
        crate::events::emit_private_sessions_sync(Some(&agent_id));
        Ok(Value::Null)
    }).await
}

#[tauri::command]
pub async fn group_members_update(app: tauri::AppHandle, group_id: String, add: Vec<String>, remove: Vec<String>) -> Result<Value, String> {
    let handle = app.clone();
    codex_call(app, move |_, store| {
        let gate = handle.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e|e.to_string())?;
        if add.len()>32 || remove.len()>32 {return Err("一次最多调整 32 位成员。".into());}
        for agent in &add {crate::agents::ensure_enabled(store,agent)?;}
        if store.parent_group_id(&group_id)? != group_id { return Err("请修改所属群聊的成员。".into()); }
        let group = store.group(&group_id)?;
        let mut members = group.members;
        members.retain(|agent| !remove.contains(agent));
        members.extend(add);
        members.sort();
        members.dedup();
        let value = serde_json::to_value(store.update_group(&group_id,&group.name,members,group.project_id)?).map_err(|e|e.to_string())?;
        crate::events::emit_groups_sync(Some(&group_id));
        Ok(value)
    }).await
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
        let value = crate::collaboration::control(&handle, store, &group_id, &task_id, &action)?;
        crate::events::emit_conversation_sync(&group_id);
        Ok(value)
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

#[tauri::command]
pub async fn usage_dashboard(app: tauri::AppHandle, days: Option<u32>) -> Result<Value, String> {
    codex_call(app, move |codex, store| crate::usage::dashboard(store, codex, days.unwrap_or(7))).await
}
