CREATE TABLE codex_sessions (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id),
    thread_id TEXT NOT NULL UNIQUE,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    provider TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    turn_id TEXT,
    error TEXT
);
CREATE TABLE codex_messages (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL REFERENCES projects(id),
    item_id TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    UNIQUE(project_id, item_id)
);
PRAGMA user_version = 2;
