CREATE TABLE group_task_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES group_tasks(id),
    message_id TEXT NOT NULL,
    target_agent TEXT NOT NULL,
    model TEXT NOT NULL,
    text TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(task_id,message_id,target_agent)
);
CREATE INDEX group_inputs_pending ON group_task_inputs(task_id,status,id);
PRAGMA user_version=8;
