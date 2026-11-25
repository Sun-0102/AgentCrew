CREATE TABLE claude_sessions (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id),
    thread_id TEXT NOT NULL UNIQUE,
    turn_id TEXT,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    workspace_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    error TEXT
);
PRAGMA user_version=19;
