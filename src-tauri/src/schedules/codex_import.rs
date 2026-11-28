use super::*;

pub(super) fn codex_db_path() -> Option<PathBuf> {
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
