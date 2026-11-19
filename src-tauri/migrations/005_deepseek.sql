CREATE TABLE deepseek_session (
    id INTEGER PRIMARY KEY CHECK(id=1),
    thread_id TEXT NOT NULL UNIQUE,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    workspace_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    error TEXT
);
CREATE TABLE deepseek_messages (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
PRAGMA user_version=5;
