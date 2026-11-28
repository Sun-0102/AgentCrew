CREATE TABLE IF NOT EXISTS delegated_tasks (
    id TEXT PRIMARY KEY,
    parent_conversation_id TEXT NOT NULL,
    parent_message_id TEXT,
    lead_agent TEXT NOT NULL,
    lead_model TEXT NOT NULL,
    worker_agent TEXT NOT NULL,
    worker_model TEXT NOT NULL,
    worker_conversation_id TEXT NOT NULL,
    instruction TEXT NOT NULL,
    return_to_sender INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,
    hop_count INTEGER NOT NULL DEFAULT 1,
    sequence_start INTEGER NOT NULL DEFAULT 0,
    summary TEXT,
    error TEXT,
    receipt_dispatched INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_delegated_tasks_parent
    ON delegated_tasks(parent_conversation_id, created_at);

CREATE INDEX IF NOT EXISTS idx_delegated_tasks_status
    ON delegated_tasks(status);

CREATE TABLE IF NOT EXISTS context_checkpoints (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    up_to_sequence INTEGER NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_context_checkpoints_conv
    ON context_checkpoints(conversation_id, up_to_sequence DESC);

CREATE TABLE IF NOT EXISTS context_governance_state (
    conversation_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    last_compacted_sequence INTEGER NOT NULL DEFAULT 0,
    last_compacted_turn INTEGER NOT NULL DEFAULT 0,
    last_compacted_at TEXT,
    PRIMARY KEY (conversation_id, agent_id)
);

ALTER TABLE app_controls ADD COLUMN auto_compact_context INTEGER NOT NULL DEFAULT 1;

PRAGMA user_version = 29;
