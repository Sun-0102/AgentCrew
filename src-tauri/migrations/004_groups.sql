CREATE TABLE chat_groups (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id),
    name TEXT NOT NULL,
    members TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
ALTER TABLE codex_messages ADD COLUMN created_at TEXT;
CREATE TRIGGER codex_message_timestamp AFTER INSERT ON codex_messages
WHEN NEW.created_at IS NULL BEGIN
    UPDATE codex_messages SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE sequence=NEW.sequence;
END;
PRAGMA user_version = 4;
