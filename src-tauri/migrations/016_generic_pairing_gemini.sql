-- Keep existing invitations bound to their original provider and protocol.
ALTER TABLE pairing_invitations ADD COLUMN protocol_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE pairing_claims ADD COLUMN reported_provider_kind TEXT;
ALTER TABLE pairing_claims ADD COLUMN reported_runtime_kind TEXT;
ALTER TABLE pairing_claims ADD COLUMN verified_provider_kind TEXT;
ALTER TABLE pairing_claims ADD COLUMN verified_runtime_kind TEXT;
ALTER TABLE pairing_claims ADD COLUMN verified_runtime_version TEXT;
UPDATE pairing_claims SET reported_provider_kind=(SELECT provider_kind FROM agents WHERE id=agent_id),
    reported_runtime_kind=(SELECT runtime_kind FROM agents WHERE id=agent_id);
UPDATE pairing_claims SET verified_provider_kind=reported_provider_kind,
    verified_runtime_kind=reported_runtime_kind,
    verified_runtime_version=(SELECT runtime_version FROM agents WHERE id=agent_id)
WHERE state='active';

CREATE TABLE gemini_sessions (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id),
    thread_id TEXT NOT NULL UNIQUE,
    turn_id TEXT,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    workspace_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    error TEXT
);
PRAGMA user_version=16;
