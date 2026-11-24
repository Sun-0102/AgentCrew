CREATE TABLE usage_records (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    provider_kind TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    conversation_id TEXT NOT NULL DEFAULT '',
    scope_kind TEXT NOT NULL DEFAULT 'private',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    status TEXT NOT NULL DEFAULT 'completed',
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX usage_records_created ON usage_records(created_at);
CREATE INDEX usage_records_provider ON usage_records(provider_kind, created_at);
PRAGMA user_version=18;
