CREATE TABLE IF NOT EXISTS shared_tables (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) > 0),
    creator TEXT NOT NULL CHECK(length(creator) > 0),
    group_name TEXT NOT NULL,
    url TEXT NOT NULL CHECK(length(url) > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0)
);
