CREATE TABLE speed_preferences (
    conversation_id TEXT NOT NULL,
    model TEXT NOT NULL,
    service_tier TEXT NOT NULL,
    PRIMARY KEY(conversation_id,model)
);
PRAGMA user_version=10;
