CREATE TABLE group_message_summaries (
    sequence INTEGER PRIMARY KEY REFERENCES codex_messages(sequence) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    excerpt TEXT NOT NULL
);
CREATE INDEX group_message_summaries_conversation ON group_message_summaries(conversation_id, sequence);
PRAGMA user_version=13;
