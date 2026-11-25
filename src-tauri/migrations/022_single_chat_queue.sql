CREATE TABLE single_chat_queue (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL,
    model TEXT NOT NULL,
    text TEXT NOT NULL,
    reasoning_effort TEXT,
    service_tier TEXT,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','paused')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX single_chat_queue_conv ON single_chat_queue(conversation_id, rowid);
PRAGMA user_version = 22;
