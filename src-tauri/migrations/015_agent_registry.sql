CREATE TABLE agents (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    provider_kind TEXT NOT NULL,
    runtime_kind TEXT NOT NULL,
    registration_state TEXT NOT NULL DEFAULT 'pending',
    availability TEXT NOT NULL DEFAULT 'checking',
    enabled INTEGER NOT NULL DEFAULT 1,
    source TEXT NOT NULL DEFAULT 'pairing',
    runtime_version TEXT,
    executable_path TEXT,
    capabilities TEXT NOT NULL DEFAULT '{}',
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO agents(id,display_name,provider_kind,runtime_kind,registration_state,source)
VALUES('codex','Codex','codex','codex-app-server','active','legacy'),
      ('deepseek','DeepSeek','deepseek','deepseek-harness-sdk','active','legacy');

CREATE TABLE pairing_invitations (
    id TEXT PRIMARY KEY NOT NULL,
    instance_id TEXT NOT NULL,
    expected_provider TEXT NOT NULL,
    display_name TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    failures INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE pairing_claims (
    id TEXT PRIMARY KEY NOT NULL,
    invitation_id TEXT NOT NULL REFERENCES pairing_invitations(id),
    request_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    agent_id TEXT NOT NULL REFERENCES agents(id),
    token_hash TEXT NOT NULL,
    token_expiry INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'validating',
    reason_code TEXT,
    reason TEXT,
    UNIQUE(invitation_id,request_id)
);

-- Each new identity gets a private provider binding. Legacy bindings keep their IDs.
CREATE TABLE agent_sessions (
    agent_id TEXT NOT NULL REFERENCES agents(id),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    runtime_conversation_id TEXT NOT NULL REFERENCES conversations(id),
    workspace_path TEXT NOT NULL,
    PRIMARY KEY(agent_id,conversation_id),
    UNIQUE(runtime_conversation_id)
);
CREATE TRIGGER agent_message_insert AFTER INSERT ON codex_messages
WHEN EXISTS(SELECT 1 FROM agent_sessions WHERE runtime_conversation_id=NEW.conversation_id)
BEGIN
    INSERT INTO codex_messages(conversation_id,item_id,role,text,created_at,agent)
    SELECT conversation_id,NEW.item_id,NEW.role,NEW.text,NEW.created_at,agent_id
    FROM agent_sessions WHERE runtime_conversation_id=NEW.conversation_id
    ON CONFLICT(conversation_id,item_id) DO UPDATE SET text=excluded.text,role=excluded.role;
END;
CREATE TRIGGER agent_message_update AFTER UPDATE OF text,role ON codex_messages
WHEN EXISTS(SELECT 1 FROM agent_sessions WHERE runtime_conversation_id=NEW.conversation_id)
BEGIN
    UPDATE codex_messages SET text=NEW.text,role=NEW.role
    WHERE item_id=NEW.item_id AND conversation_id=(SELECT conversation_id FROM agent_sessions WHERE runtime_conversation_id=NEW.conversation_id);
END;

CREATE TABLE group_task_members_v15 (
    task_id TEXT NOT NULL REFERENCES group_tasks(id),
    agent TEXT NOT NULL REFERENCES agents(id),
    status TEXT NOT NULL DEFAULT 'idle',
    sequence_start INTEGER NOT NULL DEFAULT 0,
    thread_id TEXT,
    turn_id TEXT,
    pending_prompt TEXT NOT NULL DEFAULT '',
    source_agent TEXT NOT NULL DEFAULT 'user',
    source_message TEXT,
    error TEXT,
    model TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(task_id,agent)
);
INSERT INTO group_task_members_v15(task_id,agent,status,sequence_start,thread_id,turn_id,pending_prompt,source_agent,source_message,error,model)
SELECT m.*,CASE WHEN m.agent='codex' THEN t.codex_model ELSE t.deepseek_model END
FROM group_task_members m JOIN group_tasks t ON t.id=m.task_id;
DROP TABLE group_task_members;
ALTER TABLE group_task_members_v15 RENAME TO group_task_members;

ALTER TABLE group_tasks ADD COLUMN generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE group_task_inputs ADD COLUMN generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE group_task_inputs ADD COLUMN attempt_id TEXT;
ALTER TABLE group_task_inputs ADD COLUMN owner_instance TEXT;
ALTER TABLE group_task_inputs ADD COLUMN lease_until INTEGER;
ALTER TABLE group_task_inputs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE group_task_inputs ADD COLUMN next_attempt_at INTEGER NOT NULL DEFAULT 0;
CREATE TABLE agent_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    input_id INTEGER NOT NULL REFERENCES group_task_inputs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    phase TEXT NOT NULL,
    native_run_id TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at TEXT
);
CREATE TABLE app_controls (id INTEGER PRIMARY KEY CHECK(id=1),paused INTEGER NOT NULL DEFAULT 0,instance_id TEXT NOT NULL DEFAULT '');
INSERT INTO app_controls(id) VALUES(1);
-- Old selected inputs have no launch intent record: their effects cannot be inferred.
UPDATE group_tasks SET status='interrupted',error='升级前已领取的执行结果未知，请核对后重新指派。'
WHERE status IN ('running','paused','cancelling') AND EXISTS(SELECT 1 FROM group_task_inputs i WHERE i.task_id=group_tasks.id AND i.status='selected');
UPDATE group_task_inputs SET status='interrupted_unknown' WHERE status='selected';
PRAGMA user_version=15;
