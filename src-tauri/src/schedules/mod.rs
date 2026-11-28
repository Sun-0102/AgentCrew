use crate::{agents, chat_queue, collaboration, providers, storage::Store};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicI64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

static LAST_TICK_AT: AtomicI64 = AtomicI64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub schedule_kind: String,
    pub interval_minutes: Option<i64>,
    pub time_of_day: Option<String>,
    pub weekdays: Option<String>,
    pub cron_expr: Option<String>,
    pub target_type: String,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub group_id: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub session_mode: String,
    pub session_id: Option<String>,
    pub source: String,
    pub source_external_id: Option<String>,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub run_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduledTaskRun {
    pub id: String,
    pub task_id: String,
    pub trigger_kind: String,
    pub status: String,
    pub conversation_id: Option<String>,
    pub target_summary: String,
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduledTaskInput {
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub schedule_kind: String,
    pub interval_minutes: Option<i64>,
    pub time_of_day: Option<String>,
    pub weekdays: Option<String>,
    pub cron_expr: Option<String>,
    pub target_type: String,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub group_id: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub session_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexAutomationCandidate {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub status: String,
    pub rrule: String,
    pub cwds: Vec<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub schedule_kind: String,
    pub interval_minutes: Option<i64>,
    pub time_of_day: Option<String>,
    pub weekdays: Option<String>,
    pub cron_expr: Option<String>,
    pub already_imported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexScanResult {
    pub found: bool,
    pub db_path: Option<String>,
    pub items: Vec<CodexAutomationCandidate>,
}


mod codex_import;
mod commands;
mod rrule;

pub use codex_import::*;
pub use commands::*;
pub use rrule::*;

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        schedule_kind: row.get(4)?,
        interval_minutes: row.get(5)?,
        time_of_day: row.get(6)?,
        weekdays: row.get(7)?,
        cron_expr: row.get(8)?,
        target_type: row.get(9)?,
        agent_id: row.get(10)?,
        project_id: row.get(11)?,
        group_id: row.get(12)?,
        model: row.get(13)?,
        reasoning_effort: row.get(14)?,
        service_tier: row.get(15)?,
        source: row.get(16)?,
        source_external_id: row.get(17)?,
        next_run_at: row.get(18)?,
        last_run_at: row.get(19)?,
        last_status: row.get(20)?,
        last_error: row.get(21)?,
        run_count: row.get(22)?,
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
        session_mode: row.get(25)?,
        session_id: row.get(26)?,
    })
}

const SELECT_TASK_SQL: &str = "SELECT id,name,prompt,enabled,schedule_kind,interval_minutes,time_of_day,weekdays,cron_expr,target_type,agent_id,project_id,group_id,model,reasoning_effort,service_tier,source,source_external_id,next_run_at,last_run_at,last_status,last_error,run_count,created_at,updated_at,session_mode,session_id FROM scheduled_tasks";

pub fn list(store: &Store) -> Result<Vec<ScheduledTask>, String> {
    let conn = store.connect()?;
    let mut stmt = conn
        .prepare(&format!("{SELECT_TASK_SQL} ORDER BY enabled DESC, COALESCE(next_run_at, 9999999999) ASC, created_at DESC"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_task)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn get(store: &Store, id: &str) -> Result<ScheduledTask, String> {
    store
        .connect()?
        .query_row(&format!("{SELECT_TASK_SQL} WHERE id=?1"), [id], row_to_task)
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "定时任务不存在。".into())
}

pub fn list_runs(store: &Store, task_id: Option<&str>, limit: usize) -> Result<Vec<ScheduledTaskRun>, String> {
    let conn = store.connect()?;
    let limit = limit.clamp(1, 200) as i64;
    let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ScheduledTaskRun> {
        Ok(ScheduledTaskRun {
            id: row.get(0)?,
            task_id: row.get(1)?,
            trigger_kind: row.get(2)?,
            status: row.get(3)?,
            conversation_id: row.get(4)?,
            target_summary: row.get(5)?,
            error: row.get(6)?,
            started_at: row.get(7)?,
            finished_at: row.get(8)?,
        })
    };
    if let Some(tid) = task_id {
        let mut stmt = conn
            .prepare("SELECT id,task_id,trigger_kind,status,conversation_id,target_summary,error,started_at,finished_at FROM scheduled_task_runs WHERE task_id=?1 ORDER BY started_at DESC LIMIT ?2")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![tid, limit], map_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    } else {
        let mut stmt = conn
            .prepare("SELECT id,task_id,trigger_kind,status,conversation_id,target_summary,error,started_at,finished_at FROM scheduled_task_runs ORDER BY started_at DESC LIMIT ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![limit], map_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
}

fn validate_and_normalize(store: &Store, input: &ScheduledTaskInput) -> Result<Option<i64>, String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("任务名称请填写 1–60 个字。".into());
    }
    let prompt = input.prompt.trim();
    if prompt.is_empty() || prompt.len() > 100_000 {
        return Err("任务指令不能为空且不能超过 100000 字节。".into());
    }
    match input.target_type.as_str() {
        "agent" => {
            let agent = input
                .agent_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("请选择执行任务的 Agent。")?;
            agents::ensure_enabled(store, agent)?;
        }
        "project" => {
            let project = input
                .project_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("请选择绑定的项目空间。")?;
            let exists: bool = store
                .connect()?
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND removed_at IS NULL)",
                    [project],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                return Err("所选项目不存在或已被移除。".into());
            }
            let agent = input
                .agent_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("请选择执行任务的 Agent。")?;
            agents::ensure_enabled(store, agent)?;
        }
        "group" => {
            let group = input
                .group_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("请选择目标协作群聊。")?;
            store.group(group)?;
        }
        _ => return Err("不支持的目标类型。".into()),
    }
    if let Some(mode) = input.session_mode.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !matches!(mode, "new" | "dedicated") {
            return Err("不支持的会话模式。".into());
        }
    }
    // Always validate schedule expression even when disabled
    let next = compute_next_run(
        &input.schedule_kind,
        input.interval_minutes,
        input.time_of_day.as_deref(),
        input.weekdays.as_deref(),
        input.cron_expr.as_deref(),
        now_epoch(),
    )?;
    Ok(if input.enabled { Some(next) } else { None })
}

pub fn save(store: &Store, input: ScheduledTaskInput) -> Result<ScheduledTask, String> {
    let next_run_at = validate_and_normalize(store, &input)?;
    let conn = store.connect()?;
    let model = input.model.unwrap_or_default().trim().to_owned();
    let reasoning = input
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let tier = input
        .service_tier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let agent_id = input
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let project_id = if input.target_type == "project" {
        input
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    } else {
        None
    };
    let group_id = if input.target_type == "group" {
        input
            .group_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    } else {
        None
    };

    let session_mode = match input.session_mode.as_deref().map(str::trim) {
        Some("dedicated") => "dedicated",
        _ => "new",
    };

    let id = if let Some(existing_id) = input.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let prev = get(store, existing_id)?;
        let target_changed = prev.target_type != input.target_type
            || prev.agent_id != agent_id
            || prev.project_id != project_id
            || prev.group_id != group_id;
        let next_session_id = if session_mode == "dedicated" && !target_changed {
            prev.session_id
        } else {
            None
        };
        let updated = conn
            .execute(
                "UPDATE scheduled_tasks SET name=?2,prompt=?3,enabled=?4,schedule_kind=?5,interval_minutes=?6,time_of_day=?7,weekdays=?8,cron_expr=?9,target_type=?10,agent_id=?11,project_id=?12,group_id=?13,model=?14,reasoning_effort=?15,service_tier=?16,next_run_at=?17,session_mode=?18,session_id=?19,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                params![
                    existing_id,
                    input.name.trim(),
                    input.prompt.trim(),
                    i64::from(input.enabled),
                    input.schedule_kind,
                    input.interval_minutes,
                    input.time_of_day,
                    input.weekdays,
                    input.cron_expr,
                    input.target_type,
                    agent_id,
                    project_id,
                    group_id,
                    model,
                    reasoning,
                    tier,
                    next_run_at,
                    session_mode,
                    next_session_id,
                ],
            )
            .map_err(|e| e.to_string())?;
        if updated == 0 {
            return Err("定时任务不存在。".into());
        }
        existing_id.to_owned()
    } else {
        let new_id = format!("sched-{}", uuid::Uuid::new_v4());
        conn.execute(
            "INSERT INTO scheduled_tasks(id,name,prompt,enabled,schedule_kind,interval_minutes,time_of_day,weekdays,cron_expr,target_type,agent_id,project_id,group_id,model,reasoning_effort,service_tier,next_run_at,session_mode) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                new_id,
                input.name.trim(),
                input.prompt.trim(),
                i64::from(input.enabled),
                input.schedule_kind,
                input.interval_minutes,
                input.time_of_day,
                input.weekdays,
                input.cron_expr,
                input.target_type,
                agent_id,
                project_id,
                group_id,
                model,
                reasoning,
                tier,
                next_run_at,
                session_mode,
            ],
        )
        .map_err(|e| e.to_string())?;
        new_id
    };
    crate::events::emit_schedules_sync();
    get(store, &id)
}

pub fn toggle(store: &Store, id: &str, enabled: bool) -> Result<ScheduledTask, String> {
    let task = get(store, id)?;
    let next_run_at = if enabled {
        Some(compute_next_run(
            &task.schedule_kind,
            task.interval_minutes,
            task.time_of_day.as_deref(),
            task.weekdays.as_deref(),
            task.cron_expr.as_deref(),
            now_epoch(),
        )?)
    } else {
        None
    };
    store
        .connect()?
        .execute(
            "UPDATE scheduled_tasks SET enabled=?2,next_run_at=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            params![id, i64::from(enabled), next_run_at],
        )
        .map_err(|e| e.to_string())?;
    crate::events::emit_schedules_sync();
    get(store, id)
}

pub fn delete(store: &Store, id: &str) -> Result<(), String> {
    let deleted = store
        .connect()?
        .execute("DELETE FROM scheduled_tasks WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    if deleted == 0 {
        return Err("定时任务不存在。".into());
    }
    crate::events::emit_schedules_sync();
    Ok(())
}

/// On cold startup, advance any overdue enabled schedules to their next future slot
/// instead of burst-firing missed runs that accumulated while the computer slept or app was closed.
pub fn recover(store: &Store) -> Result<(), String> {
    let now = now_epoch();
    let tasks = list(store)?;
    let conn = store.connect()?;
    for task in tasks {
        if !task.enabled {
            continue;
        }
        let overdue = task.next_run_at.map(|t| t <= now).unwrap_or(true);
        if overdue {
            if let Ok(next) = compute_next_run(
                &task.schedule_kind,
                task.interval_minutes,
                task.time_of_day.as_deref(),
                task.weekdays.as_deref(),
                task.cron_expr.as_deref(),
                now,
            ) {
                let _ = conn.execute(
                    "UPDATE scheduled_tasks SET next_run_at=?2 WHERE id=?1",
                    params![task.id, next],
                );
            }
        }
    }
    Ok(())
}

fn resolve_fallback_model(store: &Store, provider_kind: &str) -> Option<String> {
    let table = match provider_kind {
        "codex" => "codex_sessions",
        "deepseek" => "deepseek_sessions",
        "gemini" => "gemini_sessions",
        "claude" => "claude_sessions",
        _ => return None,
    };
    let sql = format!(
        "SELECT model FROM {table} WHERE model IS NOT NULL AND trim(model) != '' ORDER BY rowid DESC LIMIT 1"
    );
    store
        .connect()
        .ok()
        .and_then(|conn| conn.query_row(&sql, [], |r| r.get::<_, String>(0)).optional().ok().flatten())
}

fn build_scheduled_prompt(task: &ScheduledTask) -> String {
    format!(
        "[AgentCrew 定时任务已自动触发 · 任务「{}」]\n说明：定时调度已由 AgentCrew 托管，请直接执行本次任务并输出当前结果，不要尝试另行创建、配置外部定时推送或打开网页登录。\n\n{}",
        task.name.trim(),
        task.prompt.trim()
    )
}

fn build_session_title(task_name: &str, started_at: i64, dedicated: bool) -> String {
    let base = task_name.trim();
    let base = if base.is_empty() { "定时任务" } else { base };
    if dedicated {
        let truncated: String = base.chars().take(35).collect();
        return format!("定时 · {truncated}");
    }
    let suffix = epoch_to_local(started_at)
        .map(|dt| format!(" {:02}-{:02} {:02}:{:02}", dt.month, dt.day, dt.hour, dt.minute))
        .unwrap_or_default();
    let max_name_chars = 40usize.saturating_sub("定时 · ".chars().count() + suffix.chars().count());
    let truncated: String = base.chars().take(max_name_chars.max(1)).collect();
    format!("定时 · {truncated}{suffix}")
}

fn dedicated_session_usable(store: &Store, task: &ScheduledTask, conv_id: &str) -> Result<bool, String> {
    let conn = store.connect()?;
    let usable: bool = match task.target_type.as_str() {
        "agent" => {
            let Some(agent_id) = task.agent_id.as_deref() else {
                return Ok(false);
            };
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM private_sessions WHERE conversation_id=?1 AND agent_id=?2 AND archived=0 AND is_default=0)",
                params![conv_id, agent_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?
        }
        "project" => {
            let Some(project_id) = task.project_id.as_deref() else {
                return Ok(false);
            };
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM project_sessions WHERE conversation_id=?1 AND project_id=?2 AND archived=0 AND is_default=0)",
                params![conv_id, project_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?
        }
        "group" => {
            let Some(group_id) = task.group_id.as_deref() else {
                return Ok(false);
            };
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM group_sessions WHERE conversation_id=?1 AND group_id=?2 AND archived=0 AND conversation_id != group_id)",
                params![conv_id, group_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?
        }
        _ => false,
    };
    Ok(usable)
}

fn resolve_task_conversation(
    store: &Store,
    task: &ScheduledTask,
    started_at: i64,
) -> Result<String, String> {
    let dedicated = task.session_mode == "dedicated";
    if dedicated {
        if let Some(existing_id) = task.session_id.as_deref().filter(|s| !s.is_empty()) {
            if dedicated_session_usable(store, task, existing_id)? {
                return Ok(existing_id.to_owned());
            }
        }
    }

    let title = build_session_title(&task.name, started_at, dedicated);
    let conv_id = match task.target_type.as_str() {
        "agent" => {
            let agent_id = task.agent_id.as_deref().ok_or("任务未指定执行 Agent。")?;
            store.create_private_session(agent_id, &title)?.id
        }
        "project" => {
            let project_id = task.project_id.as_deref().ok_or("任务未绑定项目空间。")?;
            store.create_project_session(project_id, &title)?.id
        }
        "group" => {
            let group_id = task.group_id.as_deref().ok_or("任务未选择目标群聊。")?;
            store.create_group_session(group_id, &title)?.id
        }
        _ => return Err("不支持的目标类型。".into()),
    };

    if dedicated {
        store
            .connect()?
            .execute(
                "UPDATE scheduled_tasks SET session_id=?2,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                params![task.id, conv_id],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok(conv_id)
}

fn dispatch_task(
    app: &tauri::AppHandle,
    store: &Store,
    task: &ScheduledTask,
    trigger_kind: &str,
) -> ScheduledTaskRun {
    let run_id = format!("srun-{}", uuid::Uuid::new_v4());
    let started_at = now_epoch();
    let mut conversation_id: Option<String> = None;
    let mut target_summary = String::new();
    let dispatched_prompt = build_scheduled_prompt(task);

    let outcome: Result<&str, String> = (|| {
        if store
            .connect()?
            .query_row("SELECT paused FROM app_controls WHERE id=1", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())?
        {
            return Err("所有派发已暂停，请先在设置或托盘中恢复派发。".into());
        }

        match task.target_type.as_str() {
            "agent" | "project" => {
                let agent_id = task
                    .agent_id
                    .as_deref()
                    .ok_or("任务未指定执行 Agent。")?;
                let kind = agents::ensure_enabled(store, agent_id)?;
                let default_pref = crate::model_defaults::resolve_for(store, "private", agent_id)
                    .ok()
                    .flatten();
                let model = if !task.model.trim().is_empty() {
                    task.model.trim().to_owned()
                } else if let Some(m) = default_pref
                    .as_ref()
                    .map(|p| p.model.trim())
                    .filter(|m| !m.is_empty())
                {
                    m.to_owned()
                } else {
                    resolve_fallback_model(store, &kind)
                        .ok_or("未配置执行模型，请编辑任务并选择一个模型。")?
                };
                let effective_effort = task
                    .reasoning_effort
                    .as_deref()
                    .or_else(|| default_pref.as_ref().and_then(|p| p.reasoning_effort.as_deref()));
                let effective_tier = task
                    .service_tier
                    .as_deref()
                    .or_else(|| default_pref.as_ref().and_then(|p| p.service_tier.as_deref()));
                let gate = app.state::<crate::deepseek::ExecutionGate>();
                let _guard = if kind == "deepseek" {
                    Some(gate.0.lock().map_err(|e| e.to_string())?)
                } else {
                    None
                };
                if task.target_type == "project" {
                    let project_id = task
                        .project_id
                        .as_deref()
                        .ok_or("任务未绑定项目空间。")?;
                    let project_name: String = store
                        .connect()?
                        .query_row("SELECT name FROM projects WHERE id=?1 AND removed_at IS NULL", [project_id], |r| r.get(0))
                        .optional()
                        .map_err(|e| e.to_string())?
                        .ok_or("绑定的项目已不存在。")?;
                    target_summary = format!("{project_name} · {agent_id}");
                } else {
                    target_summary = format!("{agent_id} 私聊");
                }
                let conv_id = resolve_task_conversation(store, task, started_at)?;
                conversation_id = Some(conv_id.clone());

                let status = chat_queue::session_status(app, store, agent_id, &conv_id)?;
                let is_active = status.as_deref().is_some_and(crate::codex::active);
                if is_active {
                    chat_queue::enqueue(
                        store,
                        &conv_id,
                        agent_id,
                        &model,
                        &dispatched_prompt,
                        effective_effort,
                        effective_tier,
                    )?;
                    return Ok("queued");
                }
                let runtime = agents::binding(store, agent_id, &conv_id)?;
                let _ = crate::reasoning::save(
                    store,
                    &runtime,
                    &kind,
                    &model,
                    effective_effort,
                );
                if kind == "codex" {
                    let _ = crate::speed::save(store, &runtime, &model, effective_tier);
                }
                agents::send(app, store, agent_id, &conv_id, &model, &dispatched_prompt, "user")?;
                Ok("dispatched")
            }
            "group" => {
                let group_id = task
                    .group_id
                    .as_deref()
                    .ok_or("任务未选择目标群聊。")?;
                let group = store.group(group_id)?;
                target_summary = format!("群聊 · {}", group.name);
                let conv_id = resolve_task_conversation(store, task, started_at)?;
                conversation_id = Some(conv_id.clone());
                let gate = app.state::<crate::deepseek::ExecutionGate>();
                let _guard = gate.0.lock().map_err(|e| e.to_string())?;

                let mut models = BTreeMap::new();
                for member in &group.members {
                    if let Ok(kind) = agents::ensure_enabled(store, member) {
                        let default_pref =
                            crate::model_defaults::resolve_for(store, "group", member)
                                .ok()
                                .flatten();
                        let resolved = if !task.model.trim().is_empty()
                            && task.agent_id.as_deref() == Some(member.as_str())
                        {
                            task.model.trim().to_owned()
                        } else if let Some(m) = default_pref
                            .as_ref()
                            .map(|p| p.model.trim())
                            .filter(|m| !m.is_empty())
                        {
                            m.to_owned()
                        } else {
                            resolve_fallback_model(store, &kind).unwrap_or_default()
                        };
                        let effective_effort = task.reasoning_effort.as_deref().or_else(|| {
                            default_pref
                                .as_ref()
                                .and_then(|p| p.reasoning_effort.as_deref())
                        });
                        let effective_tier = task.service_tier.as_deref().or_else(|| {
                            default_pref.as_ref().and_then(|p| p.service_tier.as_deref())
                        });
                        if !resolved.is_empty() {
                            if let Ok(runtime) = agents::binding(store, member, &conv_id) {
                                let _ = crate::reasoning::save(
                                    store,
                                    &runtime,
                                    &kind,
                                    &resolved,
                                    effective_effort,
                                );
                                if kind == "codex" {
                                    let _ = crate::speed::save(
                                        store,
                                        &runtime,
                                        &resolved,
                                        effective_tier,
                                    );
                                }
                            }
                            models.insert(member.clone(), resolved);
                        }
                    }
                }
                let target_agent = task
                    .agent_id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("all");
                collaboration::start_instances(
                    app,
                    store,
                    &conv_id,
                    target_agent,
                    models.get("codex").map(String::as_str).unwrap_or(""),
                    models.get("deepseek").map(String::as_str).unwrap_or(""),
                    &dispatched_prompt,
                    &models,
                )?;
                Ok("dispatched")
            }
            _ => Err("不支持的目标类型。".into()),
        }
    })();

    let finished_at = now_epoch();
    let (run_status, last_status, error) = match outcome {
        Ok("queued") => ("queued", "queued", None),
        Ok(_) => ("dispatched", "ok", None),
        Err(err) => ("failed", "error", Some(err)),
    };

    if let Ok(conn) = store.connect() {
        let _ = conn.execute(
            "INSERT INTO scheduled_task_runs(id,task_id,trigger_kind,status,conversation_id,target_summary,error,started_at,finished_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                run_id,
                task.id,
                trigger_kind,
                run_status,
                conversation_id,
                target_summary,
                error,
                started_at,
                finished_at,
            ],
        );
        let _ = conn.execute(
            "UPDATE scheduled_tasks SET last_run_at=?2,last_status=?3,last_error=?4,run_count=run_count+1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            params![task.id, started_at, last_status, error],
        );
    }
    if let Some(ref conv_id) = conversation_id {
        crate::events::emit_conversation_sync(conv_id);
    }
    crate::events::emit_schedules_sync();

    ScheduledTaskRun {
        id: run_id,
        task_id: task.id.clone(),
        trigger_kind: trigger_kind.to_owned(),
        status: run_status.to_owned(),
        conversation_id,
        target_summary,
        error,
        started_at,
        finished_at: Some(finished_at),
    }
}

/// Called from `collaboration::start_scheduler` loop. Throttled to check at most every 2 seconds.
pub fn tick(app: &tauri::AppHandle) {
    let now = now_epoch();
    let prev = LAST_TICK_AT.load(Ordering::Relaxed);
    if now - prev < 2 {
        return;
    }
    if LAST_TICK_AT
        .compare_exchange(prev, now, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        return;
    }

    let store = app.state::<Store>();
    let paused = store
        .connect()
        .and_then(|c| {
            c.query_row("SELECT paused FROM app_controls WHERE id=1", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(|e| e.to_string())
        })
        .unwrap_or(false);
    if paused {
        return;
    }

    let due_tasks: Vec<ScheduledTask> = (|| -> Result<Vec<ScheduledTask>, String> {
        let conn = store.connect()?;
        let mut stmt = conn
            .prepare(&format!(
                "{SELECT_TASK_SQL} WHERE enabled=1 AND next_run_at IS NOT NULL AND next_run_at<=?1 ORDER BY next_run_at ASC"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([now], row_to_task)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    })()
    .unwrap_or_default();

    for task in due_tasks {
        // Advance next_run_at before dispatching to guarantee no double-fire on slow dispatch.
        let next = compute_next_run(
            &task.schedule_kind,
            task.interval_minutes,
            task.time_of_day.as_deref(),
            task.weekdays.as_deref(),
            task.cron_expr.as_deref(),
            now,
        )
        .ok();
        if let Ok(conn) = store.connect() {
            let changed = conn
                .execute(
                    "UPDATE scheduled_tasks SET next_run_at=?2 WHERE id=?1 AND enabled=1 AND next_run_at<=?3",
                    params![task.id, next, now],
                )
                .unwrap_or(0);
            if changed == 0 {
                continue;
            }
        }
        let app_clone = app.clone();
        std::thread::spawn(move || {
            let store = app_clone.state::<Store>();
            let _ = dispatch_task(&app_clone, &store, &task, "scheduled");
        });
    }
}

#[cfg(test)]
mod tests;
