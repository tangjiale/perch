-- 重建任务表以更新 CHECK；先复制提醒记录，避免删除旧表触发级联删除。
CREATE TABLE tasks_v8 (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE RESTRICT,
    source TEXT NOT NULL CHECK(source IN ('local','zentao')),
    status TEXT NOT NULL CHECK(status IN ('todo','doing','suspended','done')),
    priority TEXT NOT NULL CHECK(priority IN ('low','normal','high')),
    start_at_ms INTEGER,
    end_at_ms INTEGER,
    schedule_date TEXT,
    timezone TEXT NOT NULL,
    revision INTEGER NOT NULL,
    data TEXT NOT NULL CHECK(json_valid(data)),
    CHECK(end_at_ms IS NULL OR (start_at_ms IS NOT NULL AND end_at_ms > start_at_ms))
);
INSERT INTO tasks_v8
SELECT id,title,project_id,source,CASE WHEN status='closed' THEN 'done' ELSE status END,
       priority,start_at_ms,end_at_ms,schedule_date,timezone,revision,
       json_remove(json_set(data,
           '$.status',CASE WHEN status='closed' THEN 'done' ELSE status END,
           '$.completedAt',CASE WHEN status IN ('closed','done') THEN
               CASE WHEN json_type(data,'$.completedAt')='integer' THEN json_extract(data,'$.completedAt')
                    WHEN json_type(data,'$.closedAt')='integer' THEN json_extract(data,'$.closedAt')
                    ELSE NULL END
               ELSE NULL END), '$.closedAt')
FROM tasks;
CREATE TABLE task_reminder_deliveries_v8 (
    task_id TEXT NOT NULL REFERENCES tasks_v8(id) ON DELETE CASCADE,
    deadline_at_ms INTEGER NOT NULL,
    advance_minutes INTEGER NOT NULL CHECK(advance_minutes IN (0,15,60,1440)),
    attempted_at_ms INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('attempted','submitted','failed')),
    PRIMARY KEY(task_id,deadline_at_ms,advance_minutes)
);
INSERT INTO task_reminder_deliveries_v8 SELECT * FROM task_reminder_deliveries;
DROP TABLE task_reminder_deliveries;
DROP TABLE tasks;
ALTER TABLE tasks_v8 RENAME TO tasks;
ALTER TABLE task_reminder_deliveries_v8 RENAME TO task_reminder_deliveries;
CREATE INDEX tasks_project_status ON tasks(project_id,status);
CREATE INDEX tasks_schedule ON tasks(schedule_date,start_at_ms,end_at_ms);
