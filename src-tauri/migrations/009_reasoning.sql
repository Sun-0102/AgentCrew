CREATE TABLE reasoning_preferences (
    conversation_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    PRIMARY KEY(conversation_id,agent,model)
);
PRAGMA user_version=9;
