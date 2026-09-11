CREATE TABLE bugs (
  id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL,
  title TEXT NOT NULL,
  connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
  remote_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('active','resolved','closed')),
  data TEXT NOT NULL,
  UNIQUE(connection_id, remote_id)
);
CREATE INDEX bugs_status ON bugs(status);
