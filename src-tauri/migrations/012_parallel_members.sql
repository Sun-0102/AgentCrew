CREATE TABLE group_task_members (
    task_id TEXT NOT NULL REFERENCES group_tasks(id),
    agent TEXT NOT NULL CHECK(agent IN ('codex','deepseek')),
    status TEXT NOT NULL DEFAULT 'idle',
    sequence_start INTEGER NOT NULL DEFAULT 0,
    thread_id TEXT,
    turn_id TEXT,
    pending_prompt TEXT NOT NULL DEFAULT '',
    source_agent TEXT NOT NULL DEFAULT 'user',
    source_message TEXT,
    error TEXT,
    PRIMARY KEY(task_id,agent)
);
ALTER TABLE group_task_inputs ADD COLUMN source_agent TEXT NOT NULL DEFAULT 'user';
INSERT INTO group_task_members(task_id,agent,status)
SELECT id,'codex','idle' FROM group_tasks
UNION ALL SELECT id,'deepseek','idle' FROM group_tasks;
PRAGMA user_version=12;
