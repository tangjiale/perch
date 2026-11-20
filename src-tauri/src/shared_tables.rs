use crate::{storage::Store, AppState};
use chrono::{SecondsFormat, Utc};
use rusqlite::ffi::{SQLITE_CONSTRAINT_FOREIGNKEY, SQLITE_CONSTRAINT_UNIQUE};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::MutexGuard;
use tauri::State;
use url::Url;

const CONFLICT: &str = "此共享链接已被修改或删除，请刷新后重试。";
const GROUP_CONFLICT: &str = "此分组已被修改或删除，请刷新后重试。";
const GROUP_MISSING: &str = "所选分组已不存在，请刷新后重新选择。";
const GROUP_DUPLICATE: &str = "已有同名分组，请使用其他名称。";
const GROUP_IN_USE: &str = "该分组中仍有共享链接，请先将链接移出分组。";

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedTable {
    id: String,
    name: String,
    creator: String,
    group_id: Option<String>,
    url: String,
    created_at: String,
    updated_at: String,
    revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedTableInput {
    name: String,
    creator: String,
    group_id: Option<String>,
    url: String,
    #[serde(default, deserialize_with = "present")]
    id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    revision: Option<i64>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct SharedTableGroup {
    id: String,
    name: String,
    revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedTableGroupInput {
    name: String,
    #[serde(default, deserialize_with = "present")]
    id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    revision: Option<i64>,
}

// 新建时字段必须缺省，显式 null 不能被误当作新建请求。
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn workspace_connection<'a>(
    store: &'a Store,
    workspace_id: &str,
) -> Result<MutexGuard<'a, Connection>, String> {
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开共享链接管理。".into());
    }
    store.conn.lock().map_err(|e| e.to_string())
}

fn read_table(row: &Row<'_>) -> rusqlite::Result<SharedTable> {
    Ok(SharedTable {
        id: row.get("id")?,
        name: row.get("name")?,
        creator: row.get("creator")?,
        group_id: row.get("group_id")?,
        url: row.get("url")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        revision: row.get("revision")?,
    })
}

fn list(conn: &Connection) -> Result<Vec<SharedTable>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM shared_tables ORDER BY created_at, id")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], read_table).map_err(|e| e.to_string())?;
    rows.map(|row| row.map_err(|e| e.to_string())).collect()
}

fn constraint_message(error: rusqlite::Error, code: i32, message: &str) -> String {
    if error
        .sqlite_error()
        .is_some_and(|cause| cause.extended_code == code)
    {
        return message.into();
    }
    error.to_string()
}

fn validate_url(url: &str) -> Result<(), String> {
    let invalid = "请输入完整的 HTTP(S) 网页地址。";
    let (scheme, rest) = url.split_once("://").ok_or(invalid)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.is_empty()
        || authority.contains('@')
        || url.contains('\\')
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
        || !Url::parse(url).is_ok_and(|parsed| parsed.has_host())
    {
        return Err(invalid.into());
    }
    Ok(())
}

fn save(conn: &mut Connection, input: SharedTableInput) -> Result<SharedTable, String> {
    let name = input.name.trim();
    let creator = input.creator.trim();
    let url = input.url.trim();
    if name.is_empty() || creator.is_empty() {
        return Err("链接名称和创建人不能为空。".into());
    }
    validate_url(url)?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let result = match (input.id, input.revision) {
        (None, None) => tx.query_row(
            "INSERT INTO shared_tables(id, name, creator, group_id, url, created_at, updated_at, revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1) RETURNING *",
            params![uuid::Uuid::now_v7().to_string(), name, creator, input.group_id, url, now],
            read_table,
        ),
        (Some(id), Some(revision)) if !id.is_empty() && revision > 0 => {
            let next = revision.checked_add(1).ok_or(CONFLICT)?;
            tx.query_row(
                "UPDATE shared_tables SET name=?1, creator=?2, group_id=?3, url=?4, updated_at=?5, revision=?6
                 WHERE id=?7 AND revision=?8 RETURNING *",
                params![name, creator, input.group_id, url, now, next, id, revision],
                read_table,
            )
        }
        _ => return Err("修改共享链接时必须同时提供有效的 ID 和版本。".into()),
    };
    let table = result
        .optional()
        .map_err(|e| constraint_message(e, SQLITE_CONSTRAINT_FOREIGNKEY, GROUP_MISSING))?
        .ok_or(CONFLICT)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(table)
}

fn delete(conn: &Connection, id: &str, revision: i64) -> Result<(), String> {
    let changed = conn
        .execute(
            "DELETE FROM shared_tables WHERE id=?1 AND revision=?2",
            params![id, revision],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err(CONFLICT.into());
    }
    Ok(())
}

fn read_group(row: &Row<'_>) -> rusqlite::Result<SharedTableGroup> {
    Ok(SharedTableGroup {
        id: row.get("id")?,
        name: row.get("name")?,
        revision: row.get("revision")?,
    })
}

fn list_groups(conn: &Connection) -> Result<Vec<SharedTableGroup>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, revision FROM shared_table_groups ORDER BY name, id")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], read_group).map_err(|e| e.to_string())?;
    rows.map(|row| row.map_err(|e| e.to_string())).collect()
}

fn save_group(
    conn: &mut Connection,
    input: SharedTableGroupInput,
) -> Result<SharedTableGroup, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("分组名称不能为空。".into());
    }

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let result = match (input.id, input.revision) {
        (None, None) => tx.query_row(
            "INSERT INTO shared_table_groups(id, name, revision) VALUES (?1, ?2, 1)
             RETURNING id, name, revision",
            params![uuid::Uuid::now_v7().to_string(), name],
            read_group,
        ),
        (Some(id), Some(revision)) if !id.is_empty() && revision > 0 => {
            let next = revision.checked_add(1).ok_or(GROUP_CONFLICT)?;
            tx.query_row(
                "UPDATE shared_table_groups SET name=?1, revision=?2 WHERE id=?3 AND revision=?4
                 RETURNING id, name, revision",
                params![name, next, id, revision],
                read_group,
            )
        }
        _ => return Err("修改分组时必须同时提供有效的 ID 和版本。".into()),
    };
    let group = result
        .optional()
        .map_err(|e| constraint_message(e, SQLITE_CONSTRAINT_UNIQUE, GROUP_DUPLICATE))?
        .ok_or(GROUP_CONFLICT)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(group)
}

fn delete_group(conn: &Connection, id: &str, revision: i64) -> Result<(), String> {
    let changed = conn
        .execute(
            "DELETE FROM shared_table_groups WHERE id=?1 AND revision=?2",
            params![id, revision],
        )
        .map_err(|e| constraint_message(e, SQLITE_CONSTRAINT_FOREIGNKEY, GROUP_IN_USE))?;
    if changed != 1 {
        return Err(GROUP_CONFLICT.into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_shared_tables(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<SharedTable>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = workspace_connection(&store, &workspace_id)?;
    list(&conn)
}

#[tauri::command]
pub fn save_shared_table(
    state: State<'_, AppState>,
    workspace_id: String,
    input: SharedTableInput,
) -> Result<SharedTable, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut conn = workspace_connection(&store, &workspace_id)?;
    save(&mut conn, input)
}

#[tauri::command]
pub fn delete_shared_table(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    revision: i64,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = workspace_connection(&store, &workspace_id)?;
    delete(&conn, &id, revision)
}

#[tauri::command]
pub fn list_shared_table_groups(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<SharedTableGroup>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = workspace_connection(&store, &workspace_id)?;
    list_groups(&conn)
}

#[tauri::command]
pub fn save_shared_table_group(
    state: State<'_, AppState>,
    workspace_id: String,
    input: SharedTableGroupInput,
) -> Result<SharedTableGroup, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut conn = workspace_connection(&store, &workspace_id)?;
    save_group(&mut conn, input)
}

#[tauri::command]
pub fn delete_shared_table_group(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    revision: i64,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = workspace_connection(&store, &workspace_id)?;
    delete_group(&conn, &id, revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn input() -> Value {
        json!({"name":" 周报 ","creator":" 小林 ","url":" https://example.test/sheet?id=2#tab "})
    }

    fn submit(conn: &mut Connection, value: Value) -> Result<SharedTable, String> {
        save(
            conn,
            serde_json::from_value(value).map_err(|e| e.to_string())?,
        )
    }

    fn submit_group(conn: &mut Connection, value: Value) -> Result<SharedTableGroup, String> {
        save_group(
            conn,
            serde_json::from_value(value).map_err(|e| e.to_string())?,
        )
    }

    #[test]
    fn crud_requires_current_revision_and_survives_backup_restore() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("workspace");
        let store = Store::open_at(root.clone()).unwrap();
        assert!(workspace_connection(&store, "a-different-workspace").is_err());
        let (updated, group) = {
            let mut conn = workspace_connection(&store, &store.workspace_id).unwrap();
            assert!(list(&conn).unwrap().is_empty());
            let created = submit(&mut conn, input()).unwrap();
            let timestamp = &created.created_at;
            chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
            assert_eq!(
                serde_json::to_value(&created).unwrap(),
                json!({
                    "id":created.id,"name":"周报","creator":"小林","groupId":null,
                    "url":"https://example.test/sheet?id=2#tab","revision":1,
                    "createdAt":timestamp,"updatedAt":timestamp,
                })
            );
            let mut edit = input();
            edit["id"] = json!(created.id);
            edit["revision"] = json!(created.revision);
            edit["name"] = json!("新周报");
            let group = submit_group(&mut conn, json!({"name":"市场"})).unwrap();
            edit["groupId"] = json!(group.id);
            let updated = submit(&mut conn, edit.clone()).unwrap();
            assert_eq!(updated.name, "新周报");
            assert_eq!(updated.group_id.as_deref(), Some(group.id.as_str()));
            assert_eq!(updated.revision, 2);
            assert_eq!(updated.created_at, created.created_at);
            chrono::DateTime::parse_from_rfc3339(&updated.updated_at).unwrap();
            assert_eq!(submit(&mut conn, edit).unwrap_err(), CONFLICT);
            assert_eq!(
                delete(&conn, &created.id, created.revision).unwrap_err(),
                CONFLICT
            );
            assert_eq!(list(&conn).unwrap(), std::slice::from_ref(&updated));
            (updated, group)
        };

        let archive = sandbox.path().join("shared-tables.zip");
        store.backup(&archive).unwrap();
        drop(store);
        let reopened = Store::open_at(root).unwrap();
        assert_eq!(
            list(&reopened.conn.lock().unwrap()).unwrap(),
            std::slice::from_ref(&updated)
        );
        let restored = Store::restore_to(&archive, &sandbox.path().join("restored")).unwrap();
        let mut restored = restored.conn.lock().unwrap();
        assert_eq!(list(&restored).unwrap(), std::slice::from_ref(&updated));
        assert_eq!(list_groups(&restored).unwrap(), [group]);
        delete(&restored, &updated.id, updated.revision).unwrap();
        assert!(list(&restored).unwrap().is_empty());
        let mut edit = input();
        edit["id"] = json!(updated.id);
        edit["revision"] = json!(updated.revision);
        assert_eq!(submit(&mut restored, edit).unwrap_err(), CONFLICT);
    }

    #[test]
    fn rejects_invalid_fields_and_incomplete_update_identity() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut conn = store.conn.lock().unwrap();
        for field in ["name", "creator", "url"] {
            let mut invalid = input();
            invalid[field] = json!(" \n ");
            assert!(submit(&mut conn, invalid).is_err(), "{field}");
        }
        for (field, value) in [
            ("id", json!("example")),
            ("revision", json!(1)),
            ("id", Value::Null),
            ("revision", Value::Null),
            ("group", json!("旧分组字段")),
        ] {
            let mut invalid = input();
            invalid[field] = value;
            assert!(submit(&mut conn, invalid).is_err(), "{field}");
        }
        for url in [
            "example.test/sheet",
            "//example.test/sheet",
            "http:example.test/sheet",
            "https:///example.test/sheet",
            "https://",
            "https://?x=1",
            "https://user:pass@example.test/",
            "https://example.test/a b",
            "https://example.test\\sheet",
            "https://exa\nmple.test/",
            "https://example.test:65536/",
            "file:///tmp/sheet",
            "javascript:alert(1)",
        ] {
            let mut invalid = input();
            invalid["url"] = json!(url);
            assert!(submit(&mut conn, invalid).is_err(), "{url:?}");
        }
        assert!(list(&conn).unwrap().is_empty());
        for url in [
            "http://localhost:8080/sheet",
            "https://[::1]/a%20b",
            "https://表格.example/页面?tab=1#sheet",
        ] {
            let mut valid = input();
            valid["url"] = json!(url);
            let saved = submit(&mut conn, valid).unwrap();
            assert_eq!(saved.url, url);
            assert_eq!(saved.group_id, None);
        }
    }

    #[test]
    fn group_crud_checks_duplicates_revisions_and_link_references() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert!(workspace_connection(&store, "other-workspace").is_err());
        let mut conn = workspace_connection(&store, &store.workspace_id).unwrap();
        assert!(list_groups(&conn).unwrap().is_empty());
        for invalid in [
            json!({"name":" \t　"}),
            json!({"name":"财务","revision":1}),
            json!({"name":"财务","id":"missing"}),
            json!({"name":"财务","id":null,"revision":null}),
        ] {
            assert!(submit_group(&mut conn, invalid).is_err());
        }
        let group = submit_group(&mut conn, json!({"name":" 财务　"})).unwrap();
        assert_eq!(
            serde_json::to_value(&group).unwrap(),
            json!({"id":group.id,"name":"财务","revision":1})
        );
        assert_eq!(
            submit_group(&mut conn, json!({"name":" 财务 "})).unwrap_err(),
            GROUP_DUPLICATE
        );
        let other = submit_group(&mut conn, json!({"name":"归档"})).unwrap();

        let mut link = input();
        link["groupId"] = json!(group.id);
        let linked = submit(&mut conn, link.clone()).unwrap();
        assert_eq!(linked.group_id.as_deref(), Some(group.id.as_str()));
        assert_eq!(
            delete_group(&conn, &group.id, group.revision).unwrap_err(),
            GROUP_IN_USE
        );
        assert_eq!(
            submit_group(
                &mut conn,
                json!({"id":group.id,"revision":1,"name":other.name})
            )
            .unwrap_err(),
            GROUP_DUPLICATE
        );
        let rename = json!({"id":group.id,"revision":1,"name":" 收支 "});
        let renamed = submit_group(&mut conn, rename.clone()).unwrap();
        assert_eq!(renamed.id, group.id);
        assert_eq!(renamed.name, "收支");
        assert_eq!(renamed.revision, 2);
        assert_eq!(submit_group(&mut conn, rename).unwrap_err(), GROUP_CONFLICT);
        assert_eq!(
            delete_group(&conn, &group.id, group.revision).unwrap_err(),
            GROUP_CONFLICT
        );
        assert_eq!(list(&conn).unwrap(), std::slice::from_ref(&linked));

        link["id"] = json!(linked.id);
        link["revision"] = json!(linked.revision);
        link["groupId"] = json!("foreign-or-missing-group");
        assert_eq!(submit(&mut conn, link.clone()).unwrap_err(), GROUP_MISSING);
        assert_eq!(list(&conn).unwrap(), std::slice::from_ref(&linked));
        link["groupId"] = Value::Null;
        let unlinked = submit(&mut conn, link.clone()).unwrap();
        assert_eq!(unlinked.group_id, None);
        delete_group(&conn, &renamed.id, renamed.revision).unwrap();
        assert_eq!(
            delete_group(&conn, &renamed.id, renamed.revision).unwrap_err(),
            GROUP_CONFLICT
        );
        link["revision"] = json!(unlinked.revision);
        link["groupId"] = json!(renamed.id);
        assert_eq!(submit(&mut conn, link).unwrap_err(), GROUP_MISSING);
        assert_eq!(list_groups(&conn).unwrap(), [other]);
    }

    #[test]
    fn schema_13_upgrade_is_atomic_and_preserves_existing_data() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let project = store.save("project", json!({"name":"原有项目"})).unwrap();
        let db_path = store.generation.join("workbench.sqlite3");
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "DROP TABLE shared_tables; DROP TABLE shared_table_groups; PRAGMA user_version=13;",
            )
            .unwrap();
        drop(store);

        // 旧库残留版本记录会强制迁移失败，已执行的建表必须一起回滚。
        assert!(Store::open_at(root.path().to_path_buf()).is_err());
        let conn = Connection::open(db_path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            13
        );
        assert!(!conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='shared_tables')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap());
        conn.execute("DELETE FROM schema_migrations WHERE version>=14", [])
            .unwrap();
        drop(conn);

        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(store.snapshot().unwrap()["projects"], json!([project]));
        let mut conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
        assert!(conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=14)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap());
        assert!(list(&conn).unwrap().is_empty());
        submit(&mut conn, input()).unwrap();
    }

    #[test]
    fn schema_14_migrates_distinct_groups_and_preserves_link_metadata_atomically() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let db_path = store.generation.join("workbench.sqlite3");
        let created_at = "2026-01-10T02:03:04.000Z";
        let updated_at = "2026-02-01T05:06:07.890Z";
        {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch("DROP TABLE shared_tables; DROP TABLE shared_table_groups;")
                .unwrap();
            conn.execute_batch(include_str!("../migrations/014_shared_tables.sql"))
                .unwrap();
            conn.pragma_update(None, "user_version", 14).unwrap();
            for (id, group) in [("a", "财务"), ("b", "财务"), ("c", ""), ("d", "归档")] {
                conn.execute(
                    "INSERT INTO shared_tables(id, name, creator, group_name, url, created_at, updated_at, revision)
                     VALUES (?1, '旧链接', '创建人', ?2, 'https://example.test/#legacy', ?3, ?4, 7)",
                    params![id, group, created_at, updated_at],
                ).unwrap();
            }
        }
        drop(store);

        // 保留版本 15 的记录，制造迁移末尾的冲突以核验回滚。
        assert!(Store::open_at(root.path().to_path_buf()).is_err());
        let conn = Connection::open(db_path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        assert_eq!(
            conn.query_row(
                "SELECT group_name FROM shared_tables WHERE id='a'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "财务"
        );
        assert!(!conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='shared_table_groups')",
                [],
                |r| r.get::<_, bool>(0),
            )
            .unwrap());
        conn.execute("DELETE FROM schema_migrations WHERE version=15", [])
            .unwrap();
        drop(conn);

        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let conn = store.conn.lock().unwrap();
        let groups = list_groups(&conn).unwrap();
        assert_eq!(groups.len(), 2);
        let group_id = |name| {
            groups
                .iter()
                .find(|group| group.name == name)
                .unwrap()
                .id
                .clone()
        };
        let group_ids = [
            Some(group_id("财务")),
            Some(group_id("财务")),
            None,
            Some(group_id("归档")),
        ];
        assert!(groups.iter().all(|group| group.revision == 1));
        let expected = ["a", "b", "c", "d"]
            .into_iter()
            .zip(group_ids)
            .map(|(id, group_id)| SharedTable {
                id: id.into(),
                name: "旧链接".into(),
                creator: "创建人".into(),
                group_id,
                url: "https://example.test/#legacy".into(),
                created_at: created_at.into(),
                updated_at: updated_at.into(),
                revision: 7,
            })
            .collect::<Vec<_>>();
        assert_eq!(list(&conn).unwrap(), expected);
        assert!(conn
            .prepare("SELECT group_name FROM shared_tables")
            .is_err());
        assert_eq!(
            delete_group(&conn, &group_id("财务"), 1).unwrap_err(),
            GROUP_IN_USE
        );
    }
}
