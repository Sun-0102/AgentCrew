-- Keep a chat_groups row per execution conversation for existing task foreign keys.
ALTER TABLE chat_groups ADD COLUMN parent_group_id TEXT REFERENCES chat_groups(conversation_id);
CREATE TABLE group_sessions (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_groups(conversation_id),
    group_id TEXT NOT NULL REFERENCES chat_groups(conversation_id),
    name TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX group_sessions_group ON group_sessions(group_id,created_at);
INSERT INTO group_sessions(conversation_id,group_id,name,created_at)
SELECT conversation_id,conversation_id,'默认会话',created_at FROM chat_groups;
PRAGMA user_version=14;
