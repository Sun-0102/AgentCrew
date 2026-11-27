ALTER TABLE scheduled_tasks ADD COLUMN session_mode TEXT NOT NULL DEFAULT 'new' CHECK(session_mode IN ('new','dedicated'));
ALTER TABLE scheduled_tasks ADD COLUMN session_id TEXT;

CREATE INDEX scheduled_task_runs_conv_idx ON scheduled_task_runs(conversation_id);

PRAGMA user_version = 25;
