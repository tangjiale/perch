-- 每个连接、真实账号及工作项类型分别维护完整同步基线。
CREATE TABLE IF NOT EXISTS assignment_scopes (
 connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
 account TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('task','bug')),
 PRIMARY KEY(connection_id,account,kind)
);
CREATE TABLE IF NOT EXISTS assignment_members (
 connection_id TEXT NOT NULL, account TEXT NOT NULL, kind TEXT NOT NULL,
 remote_id TEXT NOT NULL,
 PRIMARY KEY(connection_id,account,kind,remote_id),
 FOREIGN KEY(connection_id,account,kind) REFERENCES assignment_scopes(connection_id,account,kind) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS assignment_notifications (
 id TEXT PRIMARY KEY, connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
 account TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('task','bug')),
 remote_id TEXT NOT NULL, entity_id TEXT NOT NULL, title TEXT NOT NULL,
 created_at INTEGER NOT NULL, read_at INTEGER, attempted_at INTEGER
);
CREATE INDEX IF NOT EXISTS assignment_unread ON assignment_notifications(read_at,created_at);
