//! 原生定时器独立于 WebView；同一连接整轮持有同步锁，失败也从完成时重新计时。
use super::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) fn interval_minutes(value: Option<&Value>) -> Result<u64, String> {
    match value {
        None => Ok(5),
        Some(value) => match value.as_u64() {
            Some(minutes @ (0 | 1 | 5 | 10 | 15 | 30 | 60)) => Ok(minutes),
            _ => Err("自动同步频率请选择关闭、1、5、10、15、30 或 60 分钟".into()),
        },
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    workspace: String,
    generation: PathBuf,
    connection: String,
}

fn due(last: Option<&Instant>, now: Instant, minutes: u64) -> bool {
    minutes > 0
        && last.is_none_or(|last| now.duration_since(*last) >= Duration::from_secs(minutes * 60))
}

fn connections(state: &AppState) -> Result<Vec<(Key, u64)>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    Ok(snapshot["connections"]
        .as_array()
        .ok_or("连接列表无效")?
        .iter()
        .filter(|connection| connection["enabled"] != false && connection["hasCredential"] == true)
        .filter_map(|connection| {
            let minutes = interval_minutes(connection.get("syncIntervalMinutes")).ok()?;
            (minutes > 0).then(|| {
                (
                    Key {
                        workspace: store.workspace_id.clone(),
                        generation: store.generation.clone(),
                        connection: text(connection, "id").into(),
                    },
                    minutes,
                )
            })
        })
        .collect())
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut completed: HashMap<Key, Instant> = HashMap::new();
        // 避免启动时与界面初始化争用资源；首轮随后自动同步。
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            let state = app.state::<AppState>();
            if let Ok(connections) = connections(&state) {
                completed.retain(|key, _| connections.iter().any(|(current, _)| current == key));
                for (key, minutes) in connections {
                    if !due(completed.get(&key), Instant::now(), minutes) {
                        continue;
                    }
                    let Ok(guard) = SyncGuard::acquire(&key.workspace, &key.connection) else {
                        continue;
                    };
                    let expected = Some((key.workspace.as_str(), key.generation.as_path()));
                    let tasks =
                        super::sync_inner(&state, key.connection.clone(), expected, Some(&guard))
                            .await;
                    if tasks.is_ok() {
                        let _ = app.emit("zentao-updated", json!({"workspaceId":key.workspace}));
                    }
                    // 两类读取独立完成，项目权限或网络错误不能永久阻止 BUG 缓存更新。
                    let bugs = super::bugs::sync_inner(
                        &state,
                        key.connection.clone(),
                        expected,
                        Some(&guard),
                    )
                    .await;
                    if bugs.is_ok() {
                        let _ = app.emit("zentao-updated", json!({"workspaceId":key.workspace}));
                    }
                    // 不输出原始错误或凭据，也不重放写操作；失败等待完整配置周期。
                    completed.insert(key, Instant::now());
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_sync_frequency_validates_defaults_and_invalid_values() {
        assert_eq!(interval_minutes(None).unwrap(), 5);
        for minutes in [0, 1, 5, 10, 15, 30, 60] {
            assert_eq!(interval_minutes(Some(&json!(minutes))).unwrap(), minutes);
        }
        for value in [json!(-1), json!(2), json!(1.5), json!("5"), Value::Null] {
            assert!(interval_minutes(Some(&value)).is_err());
        }
    }
    #[test]
    fn automatic_sync_waits_full_period_after_success_or_failure_and_respects_disable() {
        let now = Instant::now();
        assert!(due(None, now, 5));
        assert!(!due(None, now, 0));
        assert!(!due(Some(&now), now + Duration::from_secs(299), 5));
        assert!(due(Some(&now), now + Duration::from_secs(300), 5));
        assert!(due(Some(&now), now + Duration::from_secs(60), 1));
    }
    #[tokio::test]
    async fn automatic_sync_rejects_stale_generation_before_reading_credentials() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState {
            store: Mutex::new(crate::storage::Store::open_at(root.path().to_path_buf()).unwrap()),
        };
        let workspace = state.store.lock().unwrap().workspace_id.clone();
        let expected = Some((workspace.as_str(), std::path::Path::new("stale-generation")));
        for result in [
            super::super::sync_inner(&state, "absent".into(), expected, None).await,
            super::super::bugs::sync_inner(&state, "absent".into(), expected, None).await,
        ] {
            assert_eq!(result.unwrap_err(), "工作空间已切换，取消本次自动同步");
        }
    }

    #[test]
    fn automatic_sync_generation_isolates_schedule_and_guard_blocks_manual_overlap() {
        let key = Key {
            workspace: "scheduler-test".into(),
            generation: "a".into(),
            connection: "connection".into(),
        };
        let mut completed = HashMap::new();
        completed.insert(key.clone(), Instant::now());
        let changed = Key {
            generation: "b".into(),
            ..key.clone()
        };
        assert!(!completed.contains_key(&changed));
        let guard = SyncGuard::acquire(&key.workspace, &key.connection).unwrap();
        assert!(SyncGuard::acquire(&key.workspace, &key.connection).is_err());
        drop(guard);
        assert!(SyncGuard::acquire(&key.workspace, &key.connection).is_ok());
    }
}
