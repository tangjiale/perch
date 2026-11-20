CREATE TABLE IF NOT EXISTS password_vault (
    id TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('application', 'server')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    encrypted TEXT NOT NULL CHECK (json_valid(encrypted))
);

CREATE INDEX IF NOT EXISTS password_vault_by_kind ON password_vault(kind);
