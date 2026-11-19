CREATE TABLE conversations (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT UNIQUE REFERENCES projects(id),
    workspace_path TEXT NOT NULL
);
INSERT INTO conversations(id, project_id, workspace_path)
SELECT id, id, primary_root FROM projects;

CREATE TABLE codex_sessions_new (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id),
    thread_id TEXT NOT NULL UNIQUE,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    provider TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    turn_id TEXT,
    error TEXT
);
INSERT INTO codex_sessions_new
SELECT project_id, thread_id, model, runtime_version, provider, status, turn_id, error FROM codex_sessions;

CREATE TABLE codex_messages_new (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    item_id TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    UNIQUE(conversation_id, item_id)
);
INSERT INTO codex_messages_new SELECT sequence, project_id, item_id, role, text FROM codex_messages;
DROP TABLE codex_messages;
DROP TABLE codex_sessions;
ALTER TABLE codex_sessions_new RENAME TO codex_sessions;
ALTER TABLE codex_messages_new RENAME TO codex_messages;
PRAGMA user_version = 3;
