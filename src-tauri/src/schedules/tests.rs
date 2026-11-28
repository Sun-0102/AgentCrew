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
