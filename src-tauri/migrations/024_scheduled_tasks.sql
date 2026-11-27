CREATE TABLE scheduled_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    schedule_kind TEXT NOT NULL CHECK(schedule_kind IN ('interval','daily','weekly','cron')),
    interval_minutes INTEGER,
    time_of_day TEXT,
    weekdays TEXT,
    cron_expr TEXT,
    target_type TEXT NOT NULL CHECK(target_type IN ('agent','project','group')),
    agent_id TEXT,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    group_id TEXT REFERENCES chat_groups(conversation_id) ON DELETE SET NULL,
    model TEXT NOT NULL DEFAULT '',
    reasoning_effort TEXT,
    service_tier TEXT,
    source TEXT NOT NULL DEFAULT 'agentcrew',
    source_external_id TEXT,
    next_run_at INTEGER,
    last_run_at INTEGER,
    last_status TEXT,
    last_error TEXT,
    run_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX scheduled_tasks_due_idx ON scheduled_tasks(enabled, next_run_at);
CREATE INDEX scheduled_tasks_source_idx ON scheduled_tasks(source, source_external_id);

CREATE TABLE scheduled_task_runs (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES scheduled_tasks(id) ON DELETE CASCADE,
    trigger_kind TEXT NOT NULL CHECK(trigger_kind IN ('scheduled','manual')),
    status TEXT NOT NULL CHECK(status IN ('dispatched','queued','failed','skipped')),
    conversation_id TEXT,
    target_summary TEXT NOT NULL DEFAULT '',
    error TEXT,
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);

CREATE INDEX scheduled_task_runs_task_idx ON scheduled_task_runs(task_id, started_at DESC);

PRAGMA user_version = 24;
