CREATE TABLE IF NOT EXISTS app_credentials (
    app_id TEXT NOT NULL PRIMARY KEY REFERENCES apps(id) ON DELETE CASCADE,
    encrypted TEXT NOT NULL CHECK (json_valid(encrypted))
);
