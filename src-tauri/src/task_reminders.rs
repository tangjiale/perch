use crate::AppState;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::time::Duration;
use tauri::{plugin::PermissionState, AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const CHECK_INTERVAL: Duration = Duration::from_secs(30);
const CATCH_UP_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug)]
struct Reminder {
    task_id: String,
    title: String,
    deadline_at_ms: i64,
    advance_minutes: i64,
    deadline_label: String,
}

fn candidate(task: &Value, now_ms: i64) -> Option<Reminder> {
    if !matches!(task["status"].as_str(), Some("todo" | "doing"))
        || task["reminder"]["enabled"] != true
    {
        return None;
    }
    let advance_minutes = task["reminder"]["advanceMinutes"].as_i64()?;
    if !matches!(advance_minutes, 0 | 15 | 60 | 1440) {
        return None;
    }
    let schedule = &task["schedule"];
    let end = schedule["end"].as_str()?;
    let tz = schedule["timezone"]
        .as_str()?
        .parse::<chrono_tz::Tz>()
        .ok()?;
    let deadline = match schedule["kind"].as_str()? {
        "timed" => DateTime::parse_from_rfc3339(end).ok()?.with_timezone(&Utc),
        "all_day" => {
            let midnight = NaiveDate::parse_from_str(end, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)?;
            // 全天结束为排他边界；夏令时重复时取较早边界，不存在的时间不猜测。
            tz.from_local_datetime(&midnight)
                .earliest()?
                .with_timezone(&Utc)
        }
        _ => return None,
    };
    let deadline_at_ms = deadline.timestamp_millis();
    let trigger_at_ms = deadline_at_ms.checked_sub(advance_minutes * 60_000)?;
    let elapsed = now_ms.checked_sub(trigger_at_ms)?;
    if !(0..=CATCH_UP_MS).contains(&elapsed) {
        return None;
    }
    Some(Reminder {
        task_id: task["id"].as_str()?.to_owned(),
        title: task["title"].as_str()?.to_owned(),
        deadline_at_ms,
        advance_minutes,
        deadline_label: deadline
            .with_timezone(&tz)
            .format("%m月%d日 %H:%M")
            .to_string(),
    })
}

fn notification_body(reminder: &Reminder, now_ms: i64) -> String {
    if now_ms >= reminder.deadline_at_ms {
        format!(
            "{}\n任务已到期（{}）",
            reminder.title, reminder.deadline_label
        )
    } else {
        format!("{}\n即将到期：{}", reminder.title, reminder.deadline_label)
    }
}

fn dispatch(
    conn: &mut Connection,
    now_ms: i64,
    allowed: bool,
    mut send: impl FnMut(&Reminder) -> Result<(), String>,
) -> Result<usize, String> {
    if !allowed {
        return Ok(0);
    }
    // 仅读取启用提醒的未完成任务，不扫描邮件、凭据或完整工作空间快照。
    let candidates = {
        let mut statement = conn.prepare(
            "SELECT data FROM tasks WHERE status IN ('todo','doing') AND json_extract(data,'$.reminder.enabled')=1",
        ).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut candidates = Vec::new();
        for row in rows {
            let task: Value = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if let Some(reminder) = candidate(&task, now_ms) {
                candidates.push(reminder);
            }
        }
        candidates
    };
    let mut submitted = 0;
    let mut failed = false;
    for reminder in candidates {
        // 先持久记录发送意图；系统通知结果未知或进程中断时不自动重发。
        let claimed = conn.execute(
            "INSERT INTO task_reminder_deliveries(task_id,deadline_at_ms,advance_minutes,attempted_at_ms,state) VALUES(?1,?2,?3,?4,'attempted') ON CONFLICT DO NOTHING",
            params![reminder.task_id, reminder.deadline_at_ms, reminder.advance_minutes, now_ms],
        ).map_err(|e| e.to_string())?;
        if claimed == 0 {
            continue;
        }
        let success = send(&reminder).is_ok();
        submitted += usize::from(success);
        failed |= !success;
        conn.execute(
            "UPDATE task_reminder_deliveries SET state=?4 WHERE task_id=?1 AND deadline_at_ms=?2 AND advance_minutes=?3",
            params![reminder.task_id, reminder.deadline_at_ms, reminder.advance_minutes, if success { "submitted" } else { "failed" }],
        ).map_err(|e| e.to_string())?;
    }
    if failed {
        Err("任务通知发送失败，请检查系统通知设置；为避免重复，未自动重发".into())
    } else {
        Ok(submitted)
    }
}

#[tauri::command]
pub fn task_reminder_permission(app: AppHandle) -> Result<bool, String> {
    let notification = app.notification();
    let permission = notification
        .permission_state()
        .map_err(|_| "无法读取系统通知权限")?;
    if permission == PermissionState::Granted {
        return Ok(true);
    }
    if permission == PermissionState::Denied {
        return Ok(false);
    }
    notification
        .request_permission()
        .map(|state| state == PermissionState::Granted)
        .map_err(|_| "无法申请系统通知权限，请检查系统设置".into())
}

fn check(app: &AppHandle) -> Result<(), String> {
    let allowed = app
        .notification()
        .permission_state()
        .map_err(|_| "无法读取系统通知权限")?
        == PermissionState::Granted;
    if !allowed {
        return Ok(());
    }
    // 持有工作空间锁直到通知提交，避免与目录切换、任务完成操作并发。
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "任务存储暂不可用")?;
    let mut conn = store.conn.lock().map_err(|_| "任务数据库暂不可用")?;
    let now_ms = Utc::now().timestamp_millis();
    dispatch(&mut conn, now_ms, allowed, |reminder| {
        app.notification()
            .builder()
            .title("栖点 · 任务到期提醒")
            .body(notification_body(reminder, now_ms))
            .show()
            .map_err(|_| "系统通知发送失败".into())
    })?;
    Ok(())
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let handle = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || check(&handle)).await;
            if !matches!(result, Ok(Ok(()))) {
                // 不记录通知正文或任务内容。
                eprintln!("任务到期提醒检查失败，请检查系统通知权限或本地存储");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;
    use serde_json::json;

    fn task() -> Value {
        json!({"id":"reminder-test","title":"交付报告","status":"todo","source":"local","priority":"normal","reminder":{"enabled":true,"advanceMinutes":15},"schedule":{"kind":"timed","timezone":"Asia/Shanghai","start":"2026-10-12T09:00:00+08:00","end":"2026-10-12T18:00:00+08:00"}})
    }

    fn now(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn respects_trigger_status_and_catch_up_window() {
        let mut task = task();
        assert!(candidate(&task, now("2026-10-12T17:44:59+08:00")).is_none());
        assert!(candidate(&task, now("2026-10-12T17:45:00+08:00")).is_some());
        assert!(candidate(&task, now("2026-10-13T17:45:01+08:00")).is_none());
        for status in ["done", "suspended"] {
            task["status"] = json!(status);
            assert!(candidate(&task, now("2026-10-12T18:00:00+08:00")).is_none());
        }
        task["status"] = json!("todo");
        task["reminder"]["enabled"] = json!(false);
        assert!(candidate(&task, now("2026-10-12T18:00:00+08:00")).is_none());
        task["reminder"]["enabled"] = json!(true);
        task["schedule"].as_object_mut().unwrap().remove("end");
        assert!(candidate(&task, now("2026-10-12T18:00:00+08:00")).is_none());
    }

    #[test]
    fn all_day_uses_exclusive_end_and_schedule_timezone() {
        let mut task = task();
        task["schedule"] = json!({"kind":"all_day","timezone":"America/New_York","start":"2026-10-12","end":"2026-10-13"});
        task["reminder"]["advanceMinutes"] = json!(0);
        assert!(candidate(&task, now("2026-10-13T03:59:59Z")).is_none());
        let reminder = candidate(&task, now("2026-10-13T04:00:00Z")).unwrap();
        assert_eq!(reminder.deadline_at_ms, now("2026-10-13T04:00:00Z"));
        assert_eq!(reminder.deadline_label, "10月13日 00:00");
        task["schedule"]["timezone"] = json!("invalid");
        assert!(candidate(&task, now("2026-10-13T04:00:00Z")).is_none());
    }

    #[test]
    fn persists_deduplication_through_reopen_and_allows_changed_deadline() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("workspace");
        let store = Store::open_at(root.clone()).unwrap();
        let saved = store.save("task", task()).unwrap();
        let instant = now("2026-10-12T18:00:00+08:00");
        let mut calls = 0;
        let mut send = |_: &Reminder| {
            calls += 1;
            Ok(())
        };
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, false, &mut send).unwrap(),
            0
        );
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, &mut send).unwrap(),
            1
        );
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, &mut send).unwrap(),
            0
        );
        drop(store);
        let store = Store::open_at(root).unwrap();
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, &mut send).unwrap(),
            0
        );
        let mut changed = saved;
        changed["schedule"]["end"] = json!("2026-10-12T18:15:00+08:00");
        let changed = store.save("task", changed).unwrap();
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, &mut send).unwrap(),
            1
        );
        assert_eq!(calls, 2);
        store
            .delete("task", "reminder-test", changed["revision"].as_i64())
            .unwrap();
        let records: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM task_reminder_deliveries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(records, 0);
    }

    #[test]
    fn unknown_or_failed_delivery_is_not_replayed() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().join("workspace")).unwrap();
        store.save("task", task()).unwrap();
        let instant = now("2026-10-12T18:00:00+08:00");
        let mut conn = store.conn.lock().unwrap();
        assert!(dispatch(&mut conn, instant, true, |_| Err("不可用".into())).is_err());
        assert_eq!(
            dispatch(&mut conn, instant, true, |_| panic!("失败通知不得重放")).unwrap(),
            0
        );
        conn.execute("UPDATE task_reminder_deliveries SET state='attempted'", [])
            .unwrap();
        assert_eq!(
            dispatch(&mut conn, instant, true, |_| panic!("未知结果不得重放")).unwrap(),
            0
        );
    }

    #[test]
    fn schema_upgrade_and_backup_restore_preserve_tasks_and_delivery_records() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("workspace");
        let store = Store::open_at(root.clone()).unwrap();
        let saved = store.save("task", task()).unwrap();
        {
            // 在临时库重现 schema 6；真实工作空间不参与验证。
            let conn = store.conn.lock().unwrap();
            conn.execute_batch("DROP TABLE local_bugs; DROP TABLE meeting_segments; DROP TABLE meeting_audio_chunks; DROP TABLE meetings; DROP TABLE prompts; DROP TABLE task_reminder_deliveries; DELETE FROM schema_migrations WHERE version>=7; PRAGMA user_version=6;").unwrap();
        }
        drop(store);
        let store = Store::open_at(root).unwrap();
        assert_eq!(store.snapshot().unwrap()["tasks"][0], saved);
        let instant = now("2026-10-12T18:00:00+08:00");
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, |_| Ok(())).unwrap(),
            1
        );
        let archive = sandbox.path().join("backup.zip");
        store.backup(&archive).unwrap();
        let restored = Store::restore_to(&archive, &sandbox.path().join("restored")).unwrap();
        assert_eq!(restored.snapshot().unwrap()["tasks"][0], saved);
        assert_eq!(
            dispatch(
                &mut restored.conn.lock().unwrap(),
                instant,
                true,
                |_| panic!("恢复已提交通知不得重发")
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn suspended_tasks_keep_preferences_and_delivery_history_without_notifications() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut value = task();
        value["status"] = json!("suspended");
        let saved = store.save("task", value).unwrap();
        let instant = now("2026-10-12T18:00:00+08:00");
        assert_eq!(
            dispatch(&mut store.conn.lock().unwrap(), instant, true, |_| panic!(
                "已挂起任务不主动提醒"
            ))
            .unwrap(),
            0
        );
        assert_eq!(
            store.snapshot().unwrap()["tasks"][0]["reminder"],
            saved["reminder"]
        );
    }
}
