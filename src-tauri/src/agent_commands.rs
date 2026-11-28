use crate::{agents, providers, storage::Store};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tauri::Manager;

#[tauri::command]
pub fn agent_list(store: tauri::State<'_, Store>) -> Result<Vec<agents::Agent>, String> {
    agents::list(&store)
}

#[tauri::command]
pub async fn agent_auto_discover(
    app: tauri::AppHandle,
    auto_enable: Option<bool>,
) -> Result<Vec<agents::DiscoveryItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        agents::auto_discover(&app, &store, auto_enable.unwrap_or(true))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn pairing_create(
    app: tauri::AppHandle,
    display_name: String,
    previous_id: Option<String>,
) -> Result<Value, String> {
    crate::pairing::invitation(&app, &display_name, previous_id.as_deref())
}
#[tauri::command]
pub fn pairing_snapshot(store: tauri::State<'_, Store>, id: String) -> Result<Value, String> {
    store.connect()?.query_row("SELECT i.state,i.expires_at,c.state,c.reason,a.display_name,c.reported_provider_kind,c.reported_runtime_kind,c.verified_provider_kind,c.verified_runtime_kind,c.verified_runtime_version,i.protocol_version FROM pairing_invitations i LEFT JOIN pairing_claims c ON c.invitation_id=i.id LEFT JOIN agents a ON a.id=c.agent_id WHERE i.id=?1",[id],|r|Ok(json!({"state":r.get::<_,Option<String>>(2)?.unwrap_or(r.get(0)?),"expires_at":r.get::<_,i64>(1)?,"reason":r.get::<_,Option<String>>(3)?,"display_name":r.get::<_,Option<String>>(4)?,"reported_provider_kind":r.get::<_,Option<String>>(5)?,"reported_runtime_kind":r.get::<_,Option<String>>(6)?,"verified_provider_kind":r.get::<_,Option<String>>(7)?,"verified_runtime_kind":r.get::<_,Option<String>>(8)?,"runtime_version":r.get::<_,Option<String>>(9)?,"protocol_version":r.get::<_,u32>(10)?}))).map_err(|e|e.to_string())
}
#[tauri::command]
pub fn pairing_cancel(store: tauri::State<'_, Store>, id: String) -> Result<(), String> {
    let mut connection = store.connect()?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE pairing_invitations SET state='revoked' WHERE id=?1",
        [&id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("UPDATE agents SET registration_state='failed',availability='unavailable',error='用户取消接入。' WHERE id IN (SELECT agent_id FROM pairing_claims WHERE invitation_id=?1 AND state='validating')",[&id]).map_err(|e|e.to_string())?;
    tx.execute("UPDATE pairing_claims SET state='failed',reason_code='invitation_revoked',reason='用户取消接入。' WHERE invitation_id=?1 AND state='validating'",[id]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_probe(app: tauri::AppHandle, agent: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let kind = agents::ensure_enabled(&store, &agent)?;
        let adapter = providers::resolve(&kind)?;
        if adapter.is_busy(&store) {
            return Err("当前有任务正在执行，请等待完成后再重新检测。".into());
        }
        let result = adapter.probe(&app, &store);
        agents::record_probe(&store, &agent, &result)?;
        crate::events::emit_agents_sync();
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

fn conversation(
    store: &Store,
    agent: &str,
    group: Option<&str>,
    project: Option<&str>,
    session: Option<&str>,
) -> Result<String, String> {
    if let Some(group) = group {
        store.group(group)?;
        Ok(group.to_owned())
    } else if let (Some(project), Some(session)) = (project, session) {
        store.verify_project_session(project, session)?;
        Ok(session.to_owned())
    } else if let Some(session) = session {
        store.verify_private_session(agent, session)?;
        Ok(session.to_owned())
    } else {
        agents::private_conversation(store, agent, project)
    }
}

#[tauri::command]
pub async fn agent_snapshot(
    app: tauri::AppHandle,
    agent: String,
    group_id: Option<String>,
    project_id: Option<String>,
    session_id: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store=app.state::<Store>();
        let id=conversation(&store,&agent,group_id.as_deref(),project_id.as_deref(),session_id.as_deref())?;
        if id==crate::deepseek::PERSONAL && group_id.is_none() {
            let mut snapshot = app.state::<crate::deepseek::Runtime>().snapshot(&store)?;
            let mut sessions = serde_json::Map::new();
            sessions.insert("deepseek".into(), snapshot["session"].clone());
            snapshot["context_usage"] = crate::usage::conversation_context(
                &store,
                &id,
                &["deepseek".to_owned()],
                &sessions,
                None,
            )?;
            snapshot["queued_messages"] = json!(crate::chat_queue::list(&store, &id)?);
            return Ok(snapshot);
        }
        let mut snapshot=app.state::<crate::codex::Runtime>().snapshot(&store,&id)?;
        let members=if group_id.is_some() {store.group(&id)?.members} else {vec![agent.clone()]};
        let mut sessions=serde_json::Map::new();
        let mut approvals=Vec::new();
        let mut selected=agent.clone();
        for member in &members {
            let session=agents::session(&app,&store,member,&id)?;
            if session["status"].as_str().is_some_and(crate::codex::active) { selected=member.clone(); }
            sessions.insert(member.clone(),session);
            let kind=agents::provider(&store,member)?;
            if let Ok(adapter) = providers::resolve(&kind) {
                if adapter.supports_approval() {
                    let runtime=if member=="codex" {id.clone()} else {
                        use rusqlite::OptionalExtension;
                        let binding=store.connect()?.query_row("SELECT runtime_conversation_id FROM agent_sessions WHERE agent_id=?1 AND conversation_id=?2",params![member,id],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())?;
                        let Some(binding)=binding else {continue;}; binding
                    };
                    let items=adapter.approvals(&app,&store,&runtime)?;
                    for item in items {let mut item=item;item["agent_id"]=json!(member);approvals.push(item);}
                }
            }
        }
        let context_usage = crate::usage::conversation_context(
            &store,
            &id,
            &members,
            &sessions,
            Some(&app.state::<crate::codex::Runtime>()),
        )?;
        snapshot["conversation_id"]=json!(id);
        snapshot["session"]=sessions.get(&selected).cloned().unwrap_or(Value::Null);
        snapshot["agent"]=json!(selected);
        snapshot["sessions"]=json!(sessions);
        snapshot["approvals"]=json!(approvals);
        snapshot["context_usage"]=context_usage;
        if snapshot["session"].is_object() {snapshot["session"]["workspace_path"]=json!(store.conversation_root(&id)?);}
        if group_id.is_some() {
            snapshot["task"]=serde_json::to_value(crate::collaboration::latest(&store,&id)?).map_err(|e|e.to_string())?;
        } else {
            snapshot["queued_messages"] = json!(crate::chat_queue::list(&store, &id)?);
        }
        Ok(snapshot)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn agent_send(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
    model: String,
    text: String,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    follow_up_mode: Option<String>,
    wiki_refs: Option<Vec<crate::wiki::ExplicitWikiRef>>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let kind = agents::ensure_enabled(&store, &agent)?;
        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = if kind == "deepseek" {
            Some(gate.0.lock().map_err(|e| e.to_string())?)
        } else {
            None
        };
        if store
            .connect()?
            .query_row("SELECT paused FROM app_controls WHERE id=1", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())?
        {
            return Err("所有派发已暂停，请先继续。".into());
        }
        crate::collaboration::manual_send(&store, &model, &text)?;
        let id = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        if session_id.is_some() {
            if project_id.is_some() {
                store.ensure_project_session_open(&id)?;
            } else {
                store.ensure_private_session_open(&id)?;
            }
        }
        let status = crate::chat_queue::session_status(&app, &store, &agent, &id)?;
        let is_active = status.as_deref().is_some_and(crate::codex::active);
        if is_active {
            let adapter = providers::resolve(&kind)?;
            if follow_up_mode.as_deref() == Some("steer")
                && adapter.supports_steer()
                && status.as_deref() == Some("running")
            {
                let runtime = agents::binding(&store, &agent, &id)?;
                if let Ok(result) = adapter.steer(&app, &store, &runtime, &text) {
                    return Ok(result);
                }
            }
            let item = crate::chat_queue::enqueue(
                &store,
                &id,
                &agent,
                &model,
                &text,
                reasoning_effort.as_deref(),
                service_tier.as_deref(),
            )?;
            return Ok(json!({"status": "queued", "queued_item": item}));
        }
        let runtime = agents::binding(&store, &agent, &id)?;
        crate::reasoning::save(&store, &runtime, &kind, &model, reasoning_effort.as_deref())?;
        if kind == "codex" {
            crate::speed::save(&store, &runtime, &model, service_tier.as_deref())?;
        }
        let refs = wiki_refs.unwrap_or_default();
        let extra_context = crate::wiki::build_runtime_context(
            &store,
            &id,
            false,
            project_id.is_some(),
            &refs,
        )?;
        agents::send_with_context(
            &app,
            &store,
            &agent,
            &id,
            &model,
            &text,
            "user",
            extra_context.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_cancel(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    group_id: Option<String>,
    session_id: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        let store = app.state::<Store>();
        let id = conversation(&store, &agent, group_id.as_deref(), project_id.as_deref(), session_id.as_deref())?;
        if group_id.is_none() {
            let _ = crate::chat_queue::pause_all(&store, &id);
        }
        agents::cancel(&app, &store, &agent, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_approve(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    group_id: Option<String>,
    session_id: Option<String>,
    id: String,
    allow: bool,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        let store = app.state::<Store>();
        let kind = agents::ensure_enabled(&store, &agent)?;
        let adapter = providers::resolve(&kind)?;
        if !adapter.supports_approval() {
            return Err("此运行组件不提供交互审批。".into());
        }
        let logical = conversation(&store, &agent, group_id.as_deref(), project_id.as_deref(), session_id.as_deref())?;
        let runtime = agents::binding(&store, &agent, &logical)?;
        adapter.approve(&app, &store, &runtime, &id, allow)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_group_send(
    app: tauri::AppHandle,
    group_id: String,
    target_agent: String,
    models: BTreeMap<String, String>,
    efforts: BTreeMap<String, String>,
    tiers: BTreeMap<String, String>,
    text: String,
    wiki_refs: Option<Vec<crate::wiki::ExplicitWikiRef>>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let gate = app.state::<crate::deepseek::ExecutionGate>();
        let _guard = gate.0.lock().map_err(|e| e.to_string())?;
        let store = app.state::<Store>();
        let members = store.group(&group_id)?.members;
        for agent in members {
            let Some(model) = models.get(&agent).filter(|s| !s.is_empty()) else {
                continue;
            };
            let kind = agents::ensure_enabled(&store, &agent)?;
            let runtime = agents::binding(&store, &agent, &group_id)?;
            crate::reasoning::save(
                &store,
                &runtime,
                &kind,
                model,
                efforts
                    .get(&agent)
                    .filter(|s| !s.is_empty())
                    .map(String::as_str),
            )?;
            if kind == "codex" {
                crate::speed::save(
                    &store,
                    &runtime,
                    model,
                    tiers.get(&agent).map(String::as_str),
                )?;
            }
        }
        if let Some(refs) = wiki_refs.filter(|r| !r.is_empty()) {
            crate::wiki::set_pending_group_refs(&group_id, refs);
        }
        let result = crate::collaboration::start_instances(
            &app,
            &store,
            &group_id,
            &target_agent,
            models.get("codex").map(String::as_str).unwrap_or(""),
            models.get("deepseek").map(String::as_str).unwrap_or(""),
            &text,
            &models,
        )?;
        crate::events::emit_conversation_sync(&group_id);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_update(
    app: tauri::AppHandle,
    agent: String,
    enabled: bool,
    revoke: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let gate=app.state::<crate::deepseek::ExecutionGate>();
        let _guard=gate.0.lock().map_err(|e|e.to_string())?;
        let store=app.state::<Store>();
        let busy: bool=store.connect()?.query_row("SELECT EXISTS(SELECT 1 FROM group_task_members m JOIN group_tasks t ON t.id=m.task_id WHERE m.agent=?1 AND t.status IN ('running','paused','cancelling')) OR EXISTS(SELECT 1 FROM codex_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM gemini_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM claude_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping')) OR EXISTS(SELECT 1 FROM deepseek_sessions WHERE status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[&agent],|r|r.get(0)).map_err(|e|e.to_string())?;
        if busy {return Err("请先停止相关任务，再停用或移除成员。".into());}
        store.connect()?.execute("UPDATE agents SET enabled=?2,registration_state=CASE WHEN ?3 THEN 'revoked' ELSE registration_state END,availability=CASE WHEN ?2 THEN 'checking' ELSE 'disabled' END WHERE id=?1",params![agent,enabled && !revoke,revoke]).map_err(|e|e.to_string())?;
        crate::events::emit_agents_sync();
        Ok(())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn agent_queue_update(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
    id: String,
    text: String,
) -> Result<crate::chat_queue::QueuedMessage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let conv = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        crate::chat_queue::update_text(&store, &conv, &id, &text)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_queue_remove(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
    id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let conv = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        crate::chat_queue::remove(&store, &conv, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_queue_clear(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let conv = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        crate::chat_queue::clear(&store, &conv)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_queue_resume(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let conv = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        crate::chat_queue::resume_and_dispatch(&app, &store, &conv)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agent_queue_send_now(
    app: tauri::AppHandle,
    agent: String,
    project_id: Option<String>,
    session_id: Option<String>,
    id: String,
    prefer_steer: Option<bool>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let conv = conversation(&store, &agent, None, project_id.as_deref(), session_id.as_deref())?;
        crate::chat_queue::send_now(&app, &store, &conv, &id, prefer_steer.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}
