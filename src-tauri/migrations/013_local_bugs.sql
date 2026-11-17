CREATE TABLE local_bugs (
  id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL CHECK(revision > 0),
  data TEXT NOT NULL
);
