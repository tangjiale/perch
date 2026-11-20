CREATE TABLE shared_table_groups (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK(name <> ''),
    revision INTEGER NOT NULL CHECK(revision > 0)
);

ALTER TABLE shared_tables ADD COLUMN group_id TEXT REFERENCES shared_table_groups(id);

-- schema 14 保存时已清理名称首尾空白，直接按存量名称建立分组。
INSERT INTO shared_table_groups(id, name, revision)
SELECT lower(hex(randomblob(16))), name, 1
FROM (SELECT DISTINCT group_name AS name FROM shared_tables WHERE group_name <> '');

UPDATE shared_tables
SET group_id = (SELECT id FROM shared_table_groups WHERE name = shared_tables.group_name)
WHERE group_name <> '';

ALTER TABLE shared_tables DROP COLUMN group_name;
CREATE INDEX shared_tables_group_id ON shared_tables(group_id);
