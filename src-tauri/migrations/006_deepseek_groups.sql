CREATE TABLE deepseek_sessions (
    conversation_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL UNIQUE,
    model TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    workspace_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    error TEXT
);
INSERT INTO deepseek_sessions
SELECT 'deepseek-personal',thread_id,model,runtime_version,workspace_path,status,error FROM deepseek_session;
DROP TABLE deepseek_session;
ALTER TABLE codex_messages ADD COLUMN agent TEXT NOT NULL DEFAULT 'codex';
PRAGMA user_version=6;
