CREATE TABLE provider_credentials(
    provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
    secret TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
