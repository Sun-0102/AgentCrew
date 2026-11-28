CREATE TABLE model_defaults (
    scope TEXT NOT NULL CHECK(scope IN ('private', 'group')),
    agent_id TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    reasoning_effort TEXT,
    service_tier TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY(scope, agent_id)
);

ALTER TABLE app_controls ADD COLUMN group_follow_private_defaults INTEGER NOT NULL DEFAULT 1;

PRAGMA user_version = 26;
