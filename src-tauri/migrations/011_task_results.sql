ALTER TABLE group_tasks ADD COLUMN workspace_path TEXT;
ALTER TABLE group_tasks ADD COLUMN pending_source_agent TEXT NOT NULL DEFAULT 'user';
ALTER TABLE group_tasks ADD COLUMN pending_source_message TEXT;
CREATE TABLE group_task_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES group_tasks(id),
    ordinal INTEGER NOT NULL,
    agent TEXT NOT NULL,
    source_agent TEXT NOT NULL,
    source_message TEXT,
    instruction TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    sequence_start INTEGER NOT NULL,
    sequence_end INTEGER,
    before_snapshot TEXT NOT NULL,
    after_snapshot TEXT,
    messages TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at TEXT,
    UNIQUE(task_id,ordinal)
);
CREATE INDEX group_task_steps_task ON group_task_steps(task_id,ordinal);
PRAGMA user_version=11;
