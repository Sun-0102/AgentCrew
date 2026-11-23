CREATE TABLE private_sessions (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id),
    agent_id TEXT NOT NULL,
    name TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0,1)),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK(is_default IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX private_sessions_agent ON private_sessions(agent_id, created_at);
PRAGMA user_version=17;
