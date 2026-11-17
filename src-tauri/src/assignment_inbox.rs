//! 新指派收件箱：与完整同步同事务提交，系统通知领取后不自动重放。
use crate::{data::text, storage::Store};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) fn reconcile(
    tx: &Transaction<'_>,
    connection: &str,
    account: &str,
    kind: &str,
    values: &[Value],
) -> Result<(), String> {
    if account.is_empty() {
        return Ok(());
    }
    let initialized = tx
        .query_row(
            "SELECT 1 FROM assignment_scopes WHERE connection_id=?1 AND account=?2 AND kind=?3",
            params![connection, account, kind],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    let mut statement = tx.prepare("SELECT remote_id FROM assignment_members WHERE connection_id=?1 AND account=?2 AND kind=?3").map_err(|e|e.to_string())?;
    let old = statement
        .query_map(params![connection, account, kind], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(statement);
    let now = chrono::Utc::now().timestamp_millis();
    // 账号切换后，旧账号未读不继续占用当前账号角标。
    tx.execute("UPDATE assignment_notifications SET read_at=?1 WHERE connection_id=?2 AND account<>?3 AND read_at IS NULL",params![now,connection,account]).map_err(|e|e.to_string())?;
    tx.execute(
        "INSERT OR IGNORE INTO assignment_scopes VALUES(?1,?2,?3)",
        params![connection, account, kind],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM assignment_members WHERE connection_id=?1 AND account=?2 AND kind=?3",
        params![connection, account, kind],
    )
    .map_err(|e| e.to_string())?;
    let mut active = HashSet::new();
    for value in values {
        let owned = if kind == "bug" {
            text(value, "assignedTo") == account
        } else {
            text(value, "remoteExecutionOwnerAccount") == account
        };
        if !owned {
            continue;
        }
        let remote = text(value, "remoteId");
        if remote.is_empty() || !active.insert(remote.to_owned()) {
            continue;
        }
        tx.execute(
            "INSERT INTO assignment_members VALUES(?1,?2,?3,?4)",
            params![connection, account, kind, remote],
        )
        .map_err(|e| e.to_string())?;
        // 同一远端实体可能在本地删除后重新导入，修复既有提醒的详情链接。
        tx.execute("UPDATE assignment_notifications SET entity_id=?1,title=?2 WHERE connection_id=?3 AND account=?4 AND kind=?5 AND remote_id=?6 AND read_at IS NULL", params![text(value,"id"),text(value,"title"),connection,account,kind,remote]).map_err(|e|e.to_string())?;
        if initialized
            && !old.contains(remote)
            && !matches!(text(value, "status"), "done" | "closed")
        {
            tx.execute("INSERT INTO assignment_notifications(id,connection_id,account,kind,remote_id,entity_id,title,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![uuid::Uuid::now_v7().to_string(),connection,account,kind,remote,text(value,"id"),text(value,"title"),now]).map_err(|e|e.to_string())?;
        }
    }
    let mut stmt = tx.prepare("SELECT id,remote_id FROM assignment_notifications WHERE connection_id=?1 AND account=?2 AND kind=?3 AND read_at IS NULL").map_err(|e|e.to_string())?;
    let stale = stmt
        .query_map(params![connection, account, kind], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for (id, remote) in stale {
        if !active.contains(&remote) {
            tx.execute(
                "UPDATE assignment_notifications SET read_at=?1 WHERE id=?2",
                params![now, id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn list(store: &Store) -> Result<Vec<Value>, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id,kind,entity_id,title,created_at,read_at,connection_id FROM assignment_notifications WHERE read_at IS NULL ORDER BY created_at,id").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([], |r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"entityId":r.get::<_,String>(2)?,"title":r.get::<_,String>(3)?,"createdAt":r.get::<_,i64>(4)?,"readAt":r.get::<_,Option<i64>>(5)?,"connectionId":r.get::<_,String>(6)?}))).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
pub fn mark_read(store: &Store, id: Option<&str>) -> Result<(), String> {
    store.conn.lock().map_err(|e|e.to_string())?.execute("UPDATE assignment_notifications SET read_at=?1 WHERE read_at IS NULL AND (?2 IS NULL OR id=?2)",params![chrono::Utc::now().timestamp_millis(),id]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn mark_entity_read(store: &Store, kind: &str, entity_id: &str) -> Result<(), String> {
    if !matches!(kind, "task" | "bug") {
        return Err("未知指派类型".into());
    }
    store.conn.lock().map_err(|e|e.to_string())?.execute("UPDATE assignment_notifications SET read_at=?1 WHERE read_at IS NULL AND kind=?2 AND entity_id=?3",params![chrono::Utc::now().timestamp_millis(),kind,entity_id]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn claim_pending(store: &Store) -> Result<Vec<Value>, String> {
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut stmt=tx.prepare("SELECT id,kind,entity_id,title,created_at,connection_id FROM assignment_notifications WHERE read_at IS NULL AND attempted_at IS NULL ORDER BY created_at,id").map_err(|e|e.to_string())?;
    let values=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"entityId":r.get::<_,String>(2)?,"title":r.get::<_,String>(3)?,"createdAt":r.get::<_,i64>(4)?,"connectionId":r.get::<_,String>(5)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    drop(stmt);
    tx.execute("UPDATE assignment_notifications SET attempted_at=?1 WHERE read_at IS NULL AND attempted_at IS NULL",[chrono::Utc::now().timestamp_millis()]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE connections(id TEXT PRIMARY KEY); INSERT INTO connections VALUES('c');").unwrap();
        conn.execute_batch(include_str!(
            "../migrations/012_assignment_notifications.sql"
        ))
        .unwrap();
        conn
    }
    fn item(id: &str, account: &str) -> Value {
        json!({"id":format!("local-{id}"),"remoteId":id,"title":"新指派","assignedTo":account,"status":"active"})
    }
    fn sync(conn: &mut rusqlite::Connection, account: &str, values: &[Value]) {
        let tx = conn.transaction().unwrap();
        reconcile(&tx, "c", account, "bug", values).unwrap();
        tx.commit().unwrap();
    }
    fn count(conn: &rusqlite::Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM assignment_notifications WHERE read_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }
    #[test]
    fn baseline_repeat_edits_reassignment_and_identity_are_isolated() {
        let mut conn = database();
        sync(&mut conn, "me", &[item("1", "me")]);
        assert_eq!(count(&conn), 0);
        sync(&mut conn, "me", &[item("1", "me"), item("2", "me")]);
        assert_eq!(count(&conn), 1);
        let mut edited = item("2", "me");
        edited["title"] = json!("修改标题");
        sync(&mut conn, "me", &[item("1", "me"), edited]);
        assert_eq!(count(&conn), 1);
        sync(&mut conn, "me", &[]);
        assert_eq!(count(&conn), 0);
        sync(&mut conn, "me", &[item("2", "me")]);
        assert_eq!(count(&conn), 1);
        sync(&mut conn, "other", &[item("2", "other")]);
        assert_eq!(count(&conn), 0);
        sync(
            &mut conn,
            "other",
            &[item("2", "other"), item("3", "other")],
        );
        assert_eq!(count(&conn), 1);
        let tx = conn.transaction().unwrap();
        reconcile(&tx, "c", "me", "task", &[]).unwrap();
        tx.commit().unwrap();
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn failed_transaction_rolls_back_members_and_unread_and_connection_cascades() {
        let mut conn = database();
        sync(&mut conn, "me", &[]);
        {
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "bug", &[item("1", "me")]).unwrap();
            tx.rollback().unwrap();
        }
        assert_eq!(count(&conn), 0);
        sync(&mut conn, "me", &[item("1", "me")]);
        assert_eq!(count(&conn), 1);
        conn.execute("DELETE FROM connections WHERE id='c'", [])
            .unwrap();
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn restart_preserves_baseline_unread_and_delivery_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let memory = database();
        memory
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        sync(&mut conn, "me", &[]);
        sync(&mut conn, "me", &[item("1", "me")]);
        conn.execute("UPDATE assignment_notifications SET attempted_at=1", [])
            .unwrap();
        drop(conn);
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        sync(&mut conn, "me", &[item("1", "me")]);
        assert_eq!(count(&conn), 1);
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM assignment_notifications WHERE attempted_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
    }
    #[test]
    fn locally_created_execution_is_silently_added_to_existing_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        store
            .save("connection", json!({"id":"c","name":"fixture"}))
            .unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "task", &[]).unwrap();
            tx.commit().unwrap();
        }
        let created=store.save("task",json!({"id":"mine","title":"自己创建","source":"zentao","connectionId":"c","remoteId":"1","remoteType":"execution","remoteExecutionOwnerAccount":"me"})).unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "task", &[created]).unwrap();
            tx.commit().unwrap();
            assert_eq!(count(&conn), 0);
        }
    }
    #[test]
    fn delivery_claim_and_read_operations_keep_unread_until_opened() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        store
            .save("connection", json!({"id":"c","name":"fixture"}))
            .unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "bug", &[]).unwrap();
            tx.commit().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "bug", &[item("1", "me"), item("2", "me")]).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(list(&store).unwrap().len(), 2);
        assert_eq!(claim_pending(&store).unwrap().len(), 2);
        assert!(claim_pending(&store).unwrap().is_empty());
        assert_eq!(list(&store).unwrap().len(), 2);
        mark_entity_read(&store, "bug", "local-1").unwrap();
        assert_eq!(list(&store).unwrap().len(), 1);
        let id = list(&store).unwrap()[0]["id"].as_str().unwrap().to_owned();
        mark_read(&store, Some(&id)).unwrap();
        assert!(list(&store).unwrap().is_empty());
        mark_read(&store, None).unwrap();
    }
    #[test]
    fn deletion_and_reimport_do_not_leave_broken_unread_links() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        store
            .save("connection", json!({"id":"c","name":"fixture"}))
            .unwrap();
        let task=store.save("task",json!({"id":"old","title":"执行","source":"zentao","connectionId":"c","remoteId":"1","remoteExecutionOwnerAccount":"me"})).unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "task", &[]).unwrap();
            reconcile(&tx, "c", "me", "task", &[task.clone()]).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(list(&store).unwrap().len(), 1);
        store
            .delete("task", "old", task["revision"].as_i64())
            .unwrap();
        assert!(list(&store).unwrap().is_empty());
        let mut imported = task;
        imported["id"] = json!("new");
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "task", &[imported]).unwrap();
            tx.commit().unwrap();
        }
        assert!(list(&store).unwrap().is_empty());
    }
    #[test]
    fn explicit_identity_reset_is_atomic_and_ordinary_save_preserves_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"fixture"}))
            .unwrap();
        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            reconcile(&tx, "c", "me", "bug", &[]).unwrap();
            reconcile(&tx, "c", "me", "bug", &[item("1", "me")]).unwrap();
            tx.commit().unwrap();
        }
        let saved = store.save("connection", connection.clone()).unwrap();
        assert_eq!(list(&store).unwrap().len(), 1);
        assert!(store
            .save_connection_with_assignment_reset(connection)
            .is_err());
        assert_eq!(list(&store).unwrap().len(), 1);
        store.save_connection_with_assignment_reset(saved).unwrap();
        assert!(list(&store).unwrap().is_empty());
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM assignment_members", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn schema11_upgrades_without_losing_existing_business_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        let store = Store::open_at(root.clone()).unwrap();
        let generation = store.generation.clone();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO settings(key,value) VALUES('assignment-fixture','123')",
                [],
            )
            .unwrap();
            conn.execute_batch("DROP TABLE local_bugs; DROP TABLE assignment_notifications; DROP TABLE assignment_members; DROP TABLE assignment_scopes; DELETE FROM schema_migrations WHERE version>=12; PRAGMA user_version=11;").unwrap();
        }
        drop(store);
        let store = Store::open_at(root).unwrap();
        assert_eq!(store.generation, generation);
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            crate::storage::SCHEMA_VERSION
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='assignment-fixture'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "123"
        );
        assert_eq!(count(&conn), 0);
    }
    #[test]
    fn completed_and_non_owner_items_do_not_notify() {
        let mut conn = database();
        sync(&mut conn, "me", &[]);
        let mut completed = item("1", "me");
        completed["status"] = json!("closed");
        sync(&mut conn, "me", &[completed, item("2", "other")]);
        assert_eq!(count(&conn), 0);
    }
}
