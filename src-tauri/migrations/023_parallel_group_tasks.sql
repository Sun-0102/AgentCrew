DROP INDEX IF EXISTS one_active_group_task;
CREATE UNIQUE INDEX IF NOT EXISTS one_active_group_task_per_conversation
ON group_tasks(conversation_id)
WHERE status IN ('running','paused','cancelling');
PRAGMA user_version=23;
