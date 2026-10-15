CREATE TABLE task_reminder_deliveries (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    deadline_at_ms INTEGER NOT NULL,
    advance_minutes INTEGER NOT NULL CHECK(advance_minutes IN (0,15,60,1440)),
    attempted_at_ms INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('attempted','submitted','failed')),
    PRIMARY KEY(task_id, deadline_at_ms, advance_minutes)
);
