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

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalDateTime {
    year: i32,
    month: u32,   // 1..=12
    day: u32,     // 1..=31
    hour: u32,    // 0..=23
    minute: u32,  // 0..=59
    weekday: u32, // 1=Mon .. 7=Sun
}

#[cfg(unix)]
fn epoch_to_local(ts: i64) -> Option<LocalDateTime> {
    let t = ts as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let res = unsafe { libc::localtime_r(&t, &mut tm) };
    if res.is_null() {
        return None;
    }
    let weekday = if tm.tm_wday == 0 {
        7
    } else {
        tm.tm_wday as u32
    };
    Some(LocalDateTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        weekday,
    })
}

#[cfg(unix)]
fn local_to_epoch(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<i64> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month as i32 - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = hour as i32;
    tm.tm_min = minute as i32;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    let res = unsafe { libc::mktime(&mut tm) };
    if res == -1 {
        return None;
    }
    Some(res as i64)
}

#[cfg(not(unix))]
fn epoch_to_local(ts: i64) -> Option<LocalDateTime> {
    // Civil date from Unix epoch (UTC fallback on non-Unix)
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let hour = (secs / 3600) as u32;
    let minute = ((secs % 3600) / 60) as u32;
    // 1970-01-01 was Thursday (4)
    let weekday = (((days + 3).rem_euclid(7)) + 1) as u32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    Some(LocalDateTime {
        year,
        month: m,
        day: d,
        hour,
        minute,
        weekday,
    })
}

#[cfg(not(unix))]
fn local_to_epoch(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<i64> {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    Some(days * 86_400 + hour as i64 * 3600 + minute as i64 * 60)
}

fn parse_time_of_day(raw: &str) -> Result<(u32, u32), String> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let hour: u32 = parts
        .next()
        .ok_or("时间格式需为 HH:MM")?
        .parse()
        .map_err(|_| "时间格式需为 HH:MM")?;
    let minute: u32 = parts
        .next()
        .ok_or("时间格式需为 HH:MM")?
        .parse()
        .map_err(|_| "时间格式需为 HH:MM")?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return Err("时间需在 00:00 到 23:59 之间。".into());
    }
    Ok((hour, minute))
}

fn parse_weekdays(raw: &str) -> Result<Vec<u32>, String> {
    let mut days = Vec::new();
    for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let day: u32 = token
            .parse()
            .map_err(|_| "星期格式无效，需为 1–7（周一至周日）。")?;
        if !(1..=7).contains(&day) {
            return Err("星期需在 1–7（周一至周日）之间。".into());
        }
        if !days.contains(&day) {
            days.push(day);
        }
    }
    if days.is_empty() {
        return Err("请至少选择一天执行。".into());
    }
    days.sort_unstable();
    Ok(days)
}

fn parse_cron_field(field: &str, min: u32, max: u32, allow_sunday_seven: bool) -> Result<HashSet<u32>, String> {
    let mut set = HashSet::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("Cron 表达式包含空段。".into());
        }
        let (range_part, step) = match part.split_once('/') {
            Some((lhs, rhs)) => {
                let s: u32 = rhs.parse().map_err(|_| "Cron 步长无效。")?;
                if s == 0 {
                    return Err("Cron 步长不能为 0。".into());
                }
                (lhs, s)
            }
            None => (part, 1),
        };
        let (start, end) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            let a: u32 = a.parse().map_err(|_| "Cron 范围起始值无效。")?;
            let b: u32 = b.parse().map_err(|_| "Cron 范围结束值无效。")?;
            if a > b {
                return Err("Cron 范围起始值不能大于结束值。".into());
            }
            (a, b)
        } else {
            let v: u32 = range_part.parse().map_err(|_| "Cron 数值无效。")?;
            (v, v)
        };
        let upper = if allow_sunday_seven { max.max(7) } else { max };
        if start < min || end > upper {
            return Err(format!("Cron 字段值超出范围（{min}–{max}）。"));
        }
        let mut cur = start;
        while cur <= end {
            let normalized = if allow_sunday_seven && cur == 0 {
                7
            } else {
                cur
            };
            set.insert(normalized);
            cur += step;
        }
    }
    if set.is_empty() {
        return Err("Cron 字段未匹配任何值。".into());
    }
    Ok(set)
}

pub fn compute_next_run(
    schedule_kind: &str,
    interval_minutes: Option<i64>,
    time_of_day: Option<&str>,
    weekdays: Option<&str>,
    cron_expr: Option<&str>,
    after_ts: i64,
) -> Result<i64, String> {
    match schedule_kind {
        "interval" => {
            let mins = interval_minutes.ok_or("请设置执行间隔（分钟）。")?;
            if !(1..=43_200).contains(&mins) {
                return Err("执行间隔需在 1 分钟到 30 天之间。".into());
            }
            Ok(after_ts + mins * 60)
        }
        "daily" => {
            let (hour, minute) = parse_time_of_day(time_of_day.ok_or("请设置每日执行时间。")?)?;
            let base = epoch_to_local(after_ts).ok_or("无法获取本地时间。")?;
            for offset in 0..=2 {
                let probe = epoch_to_local(after_ts + offset * 86_400).ok_or("无法计算日期。")?;
                if let Some(candidate) = local_to_epoch(probe.year, probe.month, probe.day, hour, minute) {
                    if candidate > after_ts {
                        return Ok(candidate);
                    }
                }
            }
            local_to_epoch(base.year, base.month, base.day, hour, minute)
                .map(|t| t + 86_400)
                .ok_or_else(|| "无法计算下次执行时间。".into())
        }
        "weekly" => {
            let (hour, minute) = parse_time_of_day(time_of_day.ok_or("请设置每周执行时间。")?)?;
            let days = parse_weekdays(weekdays.ok_or("请选择每周执行的星期。")?)?;
            for offset in 0..=8 {
                let probe = epoch_to_local(after_ts + offset * 86_400).ok_or("无法计算日期。")?;
                if !days.contains(&probe.weekday) {
                    continue;
                }
                if let Some(candidate) = local_to_epoch(probe.year, probe.month, probe.day, hour, minute) {
                    if candidate > after_ts {
                        return Ok(candidate);
                    }
                }
            }
            Err("无法计算每周任务的下次执行时间。".into())
        }
        "cron" => {
            let expr = cron_expr.ok_or("请填写 5 段 Cron 表达式。")?.trim();
            let fields: Vec<&str> = expr.split_whitespace().collect();
            if fields.len() != 5 {
                return Err("Cron 表达式需要 5 个字段（分 时 日 月 周），例如 0 9 * * 1-5。".into());
            }
            let minutes = parse_cron_field(fields[0], 0, 59, false)?;
            let hours = parse_cron_field(fields[1], 0, 23, false)?;
            let doms = parse_cron_field(fields[2], 1, 31, false)?;
            let months = parse_cron_field(fields[3], 1, 12, false)?;
            let dows = parse_cron_field(fields[4], 0, 6, true)?;
            let dom_any = fields[2].trim() == "*";
            let dow_any = fields[4].trim() == "*";

            // Start from the next whole minute strictly after after_ts
            let mut cursor = ((after_ts / 60) + 1) * 60;
            let limit = after_ts + 366 * 86_400;
            while cursor <= limit {
                let dt = epoch_to_local(cursor).ok_or("无法解析本地时间。")?;
                if !months.contains(&dt.month) {
                    cursor += 86_400;
                    continue;
                }
                let day_match = match (dom_any, dow_any) {
                    (true, true) => true,
                    (false, true) => doms.contains(&dt.day),
                    (true, false) => dows.contains(&dt.weekday),
                    (false, false) => doms.contains(&dt.day) || dows.contains(&dt.weekday),
                };
                if !day_match {
                    // Jump to 00:00 of next day
                    let rem_mins = (24 - dt.hour) * 60 - dt.minute;
                    cursor += (rem_mins.max(1) as i64) * 60;
                    continue;
                }
                if !hours.contains(&dt.hour) {
                    let rem_mins = 60 - dt.minute;
                    cursor += (rem_mins.max(1) as i64) * 60;
                    continue;
                }
                if minutes.contains(&dt.minute) {
                    return Ok(cursor);
                }
                cursor += 60;
            }
            Err("该 Cron 表达式在未来一年内没有匹配的执行时间。".into())
        }
        _ => Err("不支持的定时类型。".into()),
    }
}

/// Convert Codex iCalendar RRULE string into AgentCrew schedule fields.
pub fn parse_codex_rrule(rrule: &str) -> (String, Option<i64>, Option<String>, Option<String>, Option<String>) {
    let mut map = BTreeMap::new();
    for part in rrule.trim().trim_start_matches("RRULE:").split(';') {
        if let Some((k, v)) = part.split_once('=') {
            map.insert(k.trim().to_ascii_uppercase(), v.trim().to_ascii_uppercase());
        }
    }
    let freq = map.get("FREQ").map(String::as_str).unwrap_or("HOURLY");
    let interval: i64 = map
        .get("INTERVAL")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(1);
    let by_hour: Option<u32> = map
        .get("BYHOUR")
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| v <= 23);
    let by_minute: u32 = map
        .get("BYMINUTE")
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| v <= 59)
        .unwrap_or(0);

    match freq {
        "MINUTELY" => (
            "interval".into(),
            Some(interval.clamp(1, 43_200)),
            None,
            None,
            None,
        ),
        "HOURLY" => {
            if interval == 24 {
                if let Some(hour) = by_hour {
                    return (
                        "daily".into(),
                        None,
                        Some(format!("{hour:02}:{by_minute:02}")),
                        None,
                        None,
                    );
                }
                return ("interval".into(), Some(1440), None, None, None);
            }
            (
                "interval".into(),
                Some((interval * 60).clamp(1, 43_200)),
                None,
                None,
                None,
            )
        }
        "DAILY" => {
            let hour = by_hour.unwrap_or(9);
            (
                "daily".into(),
                None,
                Some(format!("{hour:02}:{by_minute:02}")),
                None,
                None,
            )
        }
        "WEEKLY" => {
            let hour = by_hour.unwrap_or(9);
            let mut days = Vec::new();
            if let Some(raw_days) = map.get("BYDAY") {
                for token in raw_days.split(',') {
                    let mapped = match token.trim() {
                        "MO" => Some(1),
                        "TU" => Some(2),
                        "WE" => Some(3),
                        "TH" => Some(4),
                        "FR" => Some(5),
                        "SA" => Some(6),
                        "SU" => Some(7),
                        _ => None,
                    };
                    if let Some(d) = mapped {
                        if !days.contains(&d) {
                            days.push(d);
                        }
                    }
                }
            }
            if days.is_empty() {
                days.push(1);
            }
            days.sort_unstable();
            let weekdays = days
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(",");
            (
                "weekly".into(),
                None,
                Some(format!("{hour:02}:{by_minute:02}")),
                Some(weekdays),
                None,
            )
        }
        _ => ("daily".into(), None, Some("09:00".into()), None, None),
    }
}

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

fn codex_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|h| PathBuf::from(h).join(".codex"))
        })?;
    let primary = home.join("sqlite").join("codex.db");
    if primary.is_file() {
        return Some(primary);
    }
    let dev = home.join("sqlite").join("codex-dev.db");
    if dev.is_file() {
        return Some(dev);
    }
    None
}

pub fn scan_codex_in(db_path: &Path, store: &Store) -> Result<CodexScanResult, String> {
    if !db_path.is_file() {
        return Ok(CodexScanResult {
            found: false,
            db_path: None,
            items: Vec::new(),
        });
    }
    let imported_ids: HashSet<String> = store
        .connect()?
        .prepare("SELECT source_external_id FROM scheduled_tasks WHERE source='codex_import' AND source_external_id IS NOT NULL")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("无法只读打开 Codex 数据库：{e}"))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(2));

    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='automations')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_table {
        return Ok(CodexScanResult {
            found: true,
            db_path: Some(db_path.to_string_lossy().into_owned()),
            items: Vec::new(),
        });
    }

    let mut stmt = conn
        .prepare("SELECT id,name,prompt,status,rrule,cwds,model,reasoning_effort,last_run_at,next_run_at FROM automations ORDER BY updated_at DESC")
        .map_err(|e| format!("Codex 定时任务表结构不兼容：{e}"))?;
    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let prompt: String = row.get(2)?;
            let status: String = row.get(3)?;
            let rrule: String = row.get(4)?;
            let cwds_raw: String = row.get(5)?;
            let model: Option<String> = row.get(6)?;
            let reasoning_effort: Option<String> = row.get(7)?;
            let last_run_at: Option<i64> = row.get(8)?;
            let next_run_at: Option<i64> = row.get(9)?;
            Ok((
                id,
                name,
                prompt,
                status,
                rrule,
                cwds_raw,
                model,
                reasoning_effort,
                last_run_at,
                next_run_at,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for entry in rows.flatten() {
        let (
            id,
            name,
            prompt,
            status,
            rrule,
            cwds_raw,
            model,
            reasoning_effort,
            last_run_at,
            next_run_at,
        ) = entry;
        let cwds: Vec<String> = serde_json::from_str(&cwds_raw).unwrap_or_default();
        let (schedule_kind, interval_minutes, time_of_day, weekdays, cron_expr) =
            parse_codex_rrule(&rrule);
        // Codex timestamps may be ms or seconds; normalize to seconds
        let norm_ts = |v: Option<i64>| v.map(|t| if t > 10_000_000_000 { t / 1000 } else { t });
        items.push(CodexAutomationCandidate {
            already_imported: imported_ids.contains(&id),
            id,
            name,
            prompt,
            status,
            rrule,
            cwds,
            model,
            reasoning_effort,
            last_run_at: norm_ts(last_run_at),
            next_run_at: norm_ts(next_run_at),
            schedule_kind,
            interval_minutes,
            time_of_day,
            weekdays,
            cron_expr,
        });
    }

    Ok(CodexScanResult {
        found: true,
        db_path: Some(db_path.to_string_lossy().into_owned()),
        items,
    })
}

pub fn import_codex_in(
    db_path: &Path,
    store: &Store,
    ids: &[String],
    target_agent_id: Option<&str>,
    default_model: Option<&str>,
    enable_imported: bool,
) -> Result<Vec<ScheduledTask>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let target_agent = target_agent_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("codex");
    if target_agent != "codex" {
        agents::ensure_enabled(store, target_agent)?;
    }
    let is_codex_target = target_agent == "codex";
    let scan = scan_codex_in(db_path, store)?;
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let projects = store.list_projects()?;
    let mut created = Vec::new();
    let now = now_epoch();

    for item in scan.items {
        if !selected.contains(item.id.as_str()) {
            continue;
        }
        // Match first cwd to an existing AgentCrew project, or add it if it exists on disk
        let mut target_type = "agent".to_owned();
        let mut project_id: Option<String> = None;
        if let Some(first_cwd) = item.cwds.first().map(String::as_str).filter(|s| !s.is_empty()) {
            let target_canon = std::fs::canonicalize(first_cwd).ok();
            let matched = projects.iter().find(|p| {
                if let (Some(a), Ok(b)) = (&target_canon, std::fs::canonicalize(&p.primary_root)) {
                    a == &b
                } else {
                    p.primary_root == first_cwd
                }
            });
            if let Some(proj) = matched {
                target_type = "project".into();
                project_id = Some(proj.id.clone());
            } else if Path::new(first_cwd).is_dir() {
                if let Ok(added) = store.add_project(first_cwd) {
                    target_type = "project".into();
                    project_id = Some(added.id);
                }
            }
        }

        let model = if is_codex_target {
            item.model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| default_model.map(str::trim).filter(|s| !s.is_empty()))
                .unwrap_or("")
                .to_owned()
        } else {
            default_model
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_owned()
        };
        let reasoning_effort = if is_codex_target {
            item.reasoning_effort.clone()
        } else {
            None
        };
        let enabled = enable_imported && item.status.eq_ignore_ascii_case("ACTIVE");
        let next_run_at = if enabled {
            compute_next_run(
                &item.schedule_kind,
                item.interval_minutes,
                item.time_of_day.as_deref(),
                item.weekdays.as_deref(),
                item.cron_expr.as_deref(),
                now,
            )
            .ok()
        } else {
            None
        };
        let new_id = format!("sched-{}", uuid::Uuid::new_v4());
        let name: String = item.name.trim().chars().take(60).collect();
        let name = if name.is_empty() {
            "Codex 导入任务".to_owned()
        } else {
            name
        };

        store
            .connect()?
            .execute(
                "INSERT INTO scheduled_tasks(id,name,prompt,enabled,schedule_kind,interval_minutes,time_of_day,weekdays,cron_expr,target_type,agent_id,project_id,group_id,model,reasoning_effort,source,source_external_id,next_run_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,'codex_import',?15,?16)",
                params![
                    new_id,
                    name,
                    item.prompt,
                    i64::from(enabled),
                    item.schedule_kind,
                    item.interval_minutes,
                    item.time_of_day,
                    item.weekdays,
                    item.cron_expr,
                    target_type,
                    target_agent,
                    project_id,
                    model,
                    reasoning_effort,
                    item.id,
                    next_run_at,
                ],
            )
            .map_err(|e| e.to_string())?;
        created.push(get(store, &new_id)?);
    }

    if !created.is_empty() {
        crate::events::emit_schedules_sync();
    }
    Ok(created)
}

#[tauri::command]
pub fn schedule_list(store: tauri::State<'_, Store>) -> Result<Vec<ScheduledTask>, String> {
    list(&store)
}

#[tauri::command]
pub fn schedule_save(
    store: tauri::State<'_, Store>,
    input: ScheduledTaskInput,
) -> Result<ScheduledTask, String> {
    save(&store, input)
}

#[tauri::command]
pub fn schedule_toggle(
    store: tauri::State<'_, Store>,
    id: String,
    enabled: bool,
) -> Result<ScheduledTask, String> {
    toggle(&store, &id, enabled)
}

#[tauri::command]
pub fn schedule_delete(store: tauri::State<'_, Store>, id: String) -> Result<(), String> {
    delete(&store, &id)
}

#[tauri::command]
pub async fn schedule_run_now(
    app: tauri::AppHandle,
    id: String,
) -> Result<ScheduledTaskRun, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let task = get(&store, &id)?;
        // Validate provider availability synchronously before returning
        if matches!(task.target_type.as_str(), "agent" | "project") {
            let agent_id = task.agent_id.as_deref().ok_or("任务未指定执行 Agent。")?;
            let kind = agents::ensure_enabled(&store, agent_id)?;
            providers::resolve(&kind)?;
        }
        Ok(dispatch_task(&app, &store, &task, "manual"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn schedule_runs(
    store: tauri::State<'_, Store>,
    task_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<ScheduledTaskRun>, String> {
    list_runs(&store, task_id.as_deref(), limit.unwrap_or(50))
}

#[tauri::command]
pub fn schedule_codex_scan(store: tauri::State<'_, Store>) -> Result<CodexScanResult, String> {
    let Some(path) = codex_db_path() else {
        return Ok(CodexScanResult {
            found: false,
            db_path: None,
            items: Vec::new(),
        });
    };
    scan_codex_in(&path, &store)
}

#[tauri::command]
pub fn schedule_codex_import(
    store: tauri::State<'_, Store>,
    ids: Vec<String>,
    target_agent_id: Option<String>,
    default_model: Option<String>,
    enable_imported: Option<bool>,
) -> Result<Vec<ScheduledTask>, String> {
    let path = codex_db_path().ok_or("未在本机找到 Codex 数据库（~/.codex/sqlite/codex.db）。")?;
    import_codex_in(
        &path,
        &store,
        &ids,
        target_agent_id.as_deref(),
        default_model.as_deref(),
        enable_imported.unwrap_or(false),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_and_daily_and_weekly_and_cron_compute_future_times() {
        let now = 1_750_000_000;
        let next_interval = compute_next_run("interval", Some(30), None, None, None, now).unwrap();
        assert_eq!(next_interval, now + 1800);

        let next_daily = compute_next_run("daily", None, Some("09:30"), None, None, now).unwrap();
        assert!(next_daily > now);
        assert!(next_daily <= now + 86_400 + 3600);
        let dt = epoch_to_local(next_daily).unwrap();
        assert_eq!((dt.hour, dt.minute), (9, 30));

        let next_weekly =
            compute_next_run("weekly", None, Some("10:15"), Some("1,3,5"), None, now).unwrap();
        assert!(next_weekly > now);
        let wdt = epoch_to_local(next_weekly).unwrap();
        assert_eq!((wdt.hour, wdt.minute), (10, 15));
        assert!([1, 3, 5].contains(&wdt.weekday));

        let next_cron =
            compute_next_run("cron", None, None, None, Some("15 */2 * * 1-5"), now).unwrap();
        assert!(next_cron > now);
        let cdt = epoch_to_local(next_cron).unwrap();
        assert_eq!(cdt.minute, 15);
        assert_eq!(cdt.hour % 2, 0);
        assert!((1..=5).contains(&cdt.weekday));
    }

    #[test]
    fn parses_codex_rrule_variants() {
        let (k, m, t, w, _) = parse_codex_rrule("FREQ=HOURLY;INTERVAL=24;BYMINUTE=0");
        assert_eq!(k, "interval");
        assert_eq!(m, Some(1440));
        assert_eq!(t, None);
        assert_eq!(w, None);

        let (k, _, t, w, _) = parse_codex_rrule("FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=9;BYMINUTE=30");
        assert_eq!(k, "weekly");
        assert_eq!(t.as_deref(), Some("09:30"));
        assert_eq!(w.as_deref(), Some("1,3,5"));

        let (k, _, t, _, _) = parse_codex_rrule("FREQ=DAILY;BYHOUR=8;BYMINUTE=0");
        assert_eq!(k, "daily");
        assert_eq!(t.as_deref(), Some("08:00"));
    }

    #[test]
    fn scans_and_imports_from_codex_db() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("app.sqlite")).unwrap();
        let codex_db = dir.path().join("codex.db");
        let conn = Connection::open(&codex_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE automations(id TEXT PRIMARY KEY, name TEXT NOT NULL, prompt TEXT NOT NULL, status TEXT NOT NULL, next_run_at INTEGER, last_run_at INTEGER, cwds TEXT NOT NULL, rrule TEXT NOT NULL, model TEXT, reasoning_effort TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO automations VALUES('auto-1','每日代码巡检','检查 git diff','ACTIVE',1750003600000,NULL,'[]','FREQ=DAILY;BYHOUR=9;BYMINUTE=0','gpt-5.4','high',1750000000,1750000000)",
            [],
        )
        .unwrap();

        let scanned = scan_codex_in(&codex_db, &store).unwrap();
        assert!(scanned.found);
        assert_eq!(scanned.items.len(), 1);
        assert!(!scanned.items[0].already_imported);
        assert_eq!(scanned.items[0].schedule_kind, "daily");
        assert_eq!(scanned.items[0].time_of_day.as_deref(), Some("09:00"));

        let imported = import_codex_in(&codex_db, &store, &["auto-1".into()], None, None, false).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].name, "每日代码巡检");
        assert_eq!(imported[0].source, "codex_import");
        assert!(!imported[0].enabled);

        let scanned_again = scan_codex_in(&codex_db, &store).unwrap();
        assert!(scanned_again.items[0].already_imported);
    }

    #[test]
    fn resolves_isolated_sessions_for_new_and_dedicated_modes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("app.sqlite")).unwrap();
        let project = store.add_project(dir.path().to_str().unwrap()).unwrap();

        let new_task = save(
            &store,
            ScheduledTaskInput {
                id: None,
                name: "每天天气预报".into(),
                prompt: "查询天气".into(),
                enabled: true,
                schedule_kind: "daily".into(),
                interval_minutes: None,
                time_of_day: Some("08:00".into()),
                weekdays: None,
                cron_expr: None,
                target_type: "project".into(),
                agent_id: Some("codex".into()),
                project_id: Some(project.id.clone()),
                group_id: None,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                session_mode: None,
            },
        )
        .unwrap();
        assert_eq!(new_task.session_mode, "new");
        let run1 = resolve_task_conversation(&store, &new_task, 1_750_000_000).unwrap();
        let run2 = resolve_task_conversation(&store, &new_task, 1_750_003_600).unwrap();
        assert_ne!(run1, project.id);
        assert_ne!(run2, project.id);
        assert_ne!(run1, run2);

        let dedicated_task = save(
            &store,
            ScheduledTaskInput {
                id: None,
                name: "每日项目进度追踪".into(),
                prompt: "汇总进度".into(),
                enabled: true,
                schedule_kind: "daily".into(),
                interval_minutes: None,
                time_of_day: Some("18:00".into()),
                weekdays: None,
                cron_expr: None,
                target_type: "project".into(),
                agent_id: Some("codex".into()),
                project_id: Some(project.id.clone()),
                group_id: None,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                session_mode: Some("dedicated".into()),
            },
        )
        .unwrap();
        assert_eq!(dedicated_task.session_mode, "dedicated");
        let d1 = resolve_task_conversation(&store, &dedicated_task, 1_750_000_000).unwrap();
        assert_ne!(d1, project.id);

        let reloaded = get(&store, &dedicated_task.id).unwrap();
        assert_eq!(reloaded.session_id.as_deref(), Some(d1.as_str()));
        let d2 = resolve_task_conversation(&store, &reloaded, 1_750_003_600).unwrap();
        assert_eq!(d1, d2);

        let sessions = store.project_sessions(&project.id).unwrap();
        let default_session = sessions.iter().find(|s| s.id == project.id).unwrap();
        let dedicated_session = sessions.iter().find(|s| s.id == d1).unwrap();
        assert!(!default_session.is_scheduled);
        assert!(dedicated_session.is_scheduled);

        // Archiving the dedicated session causes next run to create a fresh one
        store
            .update_project_session(&project.id, &d1, &dedicated_session.name, true)
            .unwrap();
        let d3 = resolve_task_conversation(&store, &reloaded, 1_750_007_200).unwrap();
        assert_ne!(d3, d1);
    }
}
