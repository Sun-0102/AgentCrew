CREATE TABLE group_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES chat_groups(conversation_id),
    objective TEXT NOT NULL,
    status TEXT NOT NULL,
    current_agent TEXT NOT NULL,
    codex_model TEXT NOT NULL,
    deepseek_model TEXT NOT NULL,
    turn_count INTEGER NOT NULL DEFAULT 0,
    max_turns INTEGER NOT NULL DEFAULT 7,
    sequence_start INTEGER NOT NULL DEFAULT 0,
    thread_id TEXT,
    turn_id TEXT,
    pending_prompt TEXT NOT NULL,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX group_tasks_conversation ON group_tasks(conversation_id,created_at);
CREATE UNIQUE INDEX one_active_group_task ON group_tasks((1))
WHERE status IN ('running','paused','cancelling');
PRAGMA user_version=7;
