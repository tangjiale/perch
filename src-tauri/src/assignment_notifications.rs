//! 原生指派通知和图标角标。未读先落库，系统通知失败或关闭气泡均不清未读。
use crate::{assignment_inbox, data::text, storage::Store, AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Scope {
    workspace: String,
    generation: PathBuf,
}

impl Scope {
    fn from_store(store: &Store) -> Self {
        Self {
            workspace: store.workspace_id.clone(),
            generation: store.generation.clone(),
        }
    }
}

fn require_workspace(store: &Store, workspace_id: &str) -> Result<(), String> {
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开提醒".into());
    }
    Ok(())
}

fn generation_key(store: &Store) -> String {
    hex::encode(Sha256::digest(
        store.generation.to_string_lossy().as_bytes(),
    ))
}

fn require_generation(store: &Store, generation: &str) -> Result<(), String> {
    if generation_key(store) != generation {
        return Err("工作空间已恢复或迁移，请重新打开提醒".into());
    }
    Ok(())
}

fn inbox(workspace_id: &str, generation: &str, items: Vec<Value>) -> Value {
    let tasks = items.iter().filter(|item| item["kind"] == "task").count();
    json!({"workspaceId":workspace_id,"generation":generation,"unreadCount":items.len(),"taskCount":tasks,"bugCount":items.len()-tasks,"items":items})
}

#[tauri::command]
pub fn assignment_notification_list(
    state: State<AppState>,
    workspace_id: String,
) -> Result<Value, String> {
    let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
    require_workspace(&store, &workspace_id)?;
    Ok(inbox(
        &workspace_id,
        &generation_key(&store),
        assignment_inbox::list(&store)?,
    ))
}

fn update_badge(app: &AppHandle, count: usize) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    #[cfg(not(target_os = "windows"))]
    window
        .set_badge_count(if count == 0 {
            None
        } else {
            Some(count.min(i64::MAX as usize) as i64)
        })
        .map_err(|_| "无法更新应用图标角标")?;
    #[cfg(target_os = "windows")]
    {
        // Windows 使用任务栏叠加红点，精确未读数量显示在应用内。
        let icon = if count == 0 {
            None
        } else {
            let mut rgba = vec![0u8; 16 * 16 * 4];
            for y in 0..16 {
                for x in 0..16 {
                    if (x as i32 - 8).pow(2) + (y as i32 - 8).pow(2) <= 49 {
                        let offset = (y * 16 + x) * 4;
                        rgba[offset..offset + 4].copy_from_slice(&[235, 64, 69, 255]);
                    }
                }
            }
            Some(tauri::image::Image::new_owned(rgba, 16, 16))
        };
        window
            .set_overlay_icon(icon)
            .map_err(|_| "无法更新应用图标角标")?;
    }
    Ok(())
}

fn publish(app: &AppHandle, store: &Store) -> Result<(), String> {
    let values = assignment_inbox::list(store)?;
    // 角标暂时失败不能使已成功持久化的已读操作被报告为失败。
    if update_badge(app, values.len()).is_err() {
        eprintln!("指派提醒角标更新失败");
    }
    let _ = app.emit(
        "assignment-notifications-updated",
        json!({"workspaceId":store.workspace_id}),
    );
    Ok(())
}

#[tauri::command]
pub fn assignment_notification_read(
    app: AppHandle,
    state: State<AppState>,
    workspace_id: String,
    generation: String,
    id: String,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
    require_workspace(&store, &workspace_id)?;
    require_generation(&store, &generation)?;
    if id.is_empty() {
        return Err("缺少提醒标识".into());
    }
    assignment_inbox::mark_read(&store, Some(&id))?;
    publish(&app, &store)
}

#[tauri::command]
pub fn assignment_notification_read_entity(
    app: AppHandle,
    state: State<AppState>,
    workspace_id: String,
    generation: String,
    kind: String,
    entity_id: String,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
    require_workspace(&store, &workspace_id)?;
    require_generation(&store, &generation)?;
    if !matches!(kind.as_str(), "task" | "bug") || entity_id.is_empty() {
        return Err("无效的提醒关联对象".into());
    }
    assignment_inbox::mark_entity_read(&store, &kind, &entity_id)?;
    publish(&app, &store)
}

fn current_item(store: &Store, scope: &Scope, id: &str) -> Result<Option<Value>, String> {
    if Scope::from_store(store) != *scope {
        return Ok(None);
    }
    Ok(assignment_inbox::list(store)?
        .into_iter()
        .find(|value| text(value, "id") == id))
}

fn delivery_current(app: &AppHandle, scope: &Scope, item: &Value) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
    Ok(current_item(&store, scope, text(item, "id"))?.is_some())
}

fn open_notification(app: &AppHandle, scope: &Scope, item: &Value) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
    let Some(item) = current_item(&store, scope, text(item, "id"))? else {
        return Ok(());
    };
    let id = text(&item, "id");
    assignment_inbox::mark_read(&store, Some(id))?;
    publish(app, &store)?;
    crate::tray::show_main(app);
    app.emit("assignment-notification-open", json!({"workspaceId":scope.workspace,"id":id,"kind":item["kind"],"entityId":item["entityId"]})).map_err(|_| "无法打开指派详情".into())
}

fn title(item: &Value) -> &'static str {
    if item["kind"] == "bug" {
        "栖点 · 新 BUG 指派给你"
    } else {
        "栖点 · 新任务指派给你"
    }
}

#[cfg(target_os = "macos")]
async fn show_native(
    app: AppHandle,
    scope: Scope,
    item: Value,
    cancel: CancellationToken,
) -> Result<(), String> {
    use mac_usernotifications::{AuthorizationStatus, Notification};
    // UNUserNotificationCenter 要求实际签名的 .app；开发预览仍使用应用内气泡。
    mac_usernotifications::check_bundle().map_err(|_| "系统通知需要安装后的栖点应用")?;
    let settings = mac_usernotifications::get_notification_settings()
        .await
        .map_err(|_| "无法读取系统通知权限")?;
    let allowed = match settings.authorization_status {
        AuthorizationStatus::NotDetermined => mac_usernotifications::request_auth()
            .await
            .map_err(|_| "无法申请系统通知权限")?,
        AuthorizationStatus::Authorized
        | AuthorizationStatus::Provisional
        | AuthorizationStatus::Ephemeral => true,
        _ => false,
    };
    if !allowed || cancel.is_cancelled() || !delivery_current(&app, &scope, &item)? {
        return Ok(());
    }
    let notification_id = format!("perch-assignment-{}", text(&item, "id"));
    let handle = Notification::new()
        .id(&notification_id)
        .thread_id("perch-assignments")
        .title(title(&item))
        .message(text(&item, "title"))
        .action(mac_usernotifications::Action::button("open", "查看详情"))
        .default_sound()
        .timeout(Duration::from_secs(7 * 24 * 60 * 60))
        .send()
        .await
        .map_err(|_| "系统通知提交失败")?;
    tokio::select! {
        response = handle.response() => {
            if let Ok(response) = response {
                if response.is_default_action() || response.action_identifier == "open" {
                    open_notification(&app, &scope, &item)?;
                }
            }
        }
        _ = cancel.cancelled() => {}
    }
    mac_usernotifications::close_delivered(&notification_id).await;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn show_native(
    app: AppHandle,
    scope: Scope,
    item: Value,
    cancel: CancellationToken,
) -> Result<(), String> {
    // Windows Toast 的关闭/超时会结束等待；阻塞平台 API 不占用异步执行线程。
    tauri::async_runtime::spawn_blocking(move || {
        if cancel.is_cancelled() || !delivery_current(&app, &scope, &item)? {
            return Ok(());
        }
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(title(&item))
            .body(text(&item, "title"))
            .action("open", "查看详情");
        #[cfg(target_os = "windows")]
        if !tauri::is_dev() {
            notification.app_id(&app.config().identifier);
        }
        let handle = notification.show().map_err(|_| "系统通知提交失败")?;
        handle.wait_for_action(|action| {
            if !cancel.is_cancelled()
                && matches!(action, "default" | "open")
                && open_notification(&app, &scope, &item).is_err()
            {
                eprintln!("打开指派提醒失败，请在应用内查看未读提醒");
            }
        });
        Ok(())
    })
    .await
    .map_err(|_| "系统通知执行失败")?
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut previous: Option<(Scope, Vec<String>)> = None;
        let mut active: HashMap<String, CancellationToken> = HashMap::new();
        loop {
            // 同一工作空间锁下读取未读并持久化发送意图；失败、未知结果均不自动重放。
            let snapshot = (|| -> Result<_, String> {
                let state = app.state::<AppState>();
                let store = state.store.lock().map_err(|_| "提醒存储暂不可用")?;
                let scope = Scope::from_store(&store);
                let values = assignment_inbox::list(&store)?;
                let pending = assignment_inbox::claim_pending(&store)?;
                Ok((scope, values, pending))
            })();
            match snapshot {
                Ok((scope, values, pending)) => {
                    let ids: Vec<_> = values
                        .iter()
                        .map(|item| text(item, "id").to_owned())
                        .collect();
                    let switched = previous.as_ref().is_some_and(|(old, _)| old != &scope);
                    active.retain(|id, token| {
                        if switched || !ids.contains(id) {
                            token.cancel();
                            false
                        } else {
                            !token.is_cancelled()
                        }
                    });
                    let signature = (scope.clone(), ids);
                    if previous.as_ref() != Some(&signature) {
                        if update_badge(&app, values.len()).is_err() {
                            eprintln!("指派提醒角标更新失败");
                        }
                        let _ = app.emit(
                            "assignment-notifications-updated",
                            json!({"workspaceId":scope.workspace}),
                        );
                        previous = Some(signature);
                    }
                    if !pending.is_empty() {
                        let _ = app.emit(
                            "assignment-notification-new",
                            json!({"workspaceId":scope.workspace,"items":pending}),
                        );
                        let foreground = app.get_webview_window("main").is_some_and(|window| {
                            window.is_visible().unwrap_or(false)
                                && window.is_focused().unwrap_or(false)
                        });
                        if !foreground {
                            for item in pending {
                                let token = CancellationToken::new();
                                active.insert(text(&item, "id").to_owned(), token.clone());
                                let (app, scope) = (app.clone(), scope.clone());
                                tauri::async_runtime::spawn(async move {
                                    if show_native(app, scope, item, token.clone()).await.is_err() {
                                        // 不输出任务标题、连接账号或系统错误中的个人信息。
                                        eprintln!("指派系统通知未送达，请检查系统通知权限；未读提醒已保留");
                                    }
                                    token.cancel();
                                });
                            }
                        }
                    }
                }
                Err(_) => eprintln!("指派提醒检查失败，稍后重新读取本地未读状态"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_counts_keep_tasks_and_bugs_separate() {
        let result = inbox(
            "workspace",
            "generation",
            vec![
                json!({"kind":"task"}),
                json!({"kind":"bug"}),
                json!({"kind":"task"}),
            ],
        );
        assert_eq!(result["unreadCount"], 3);
        assert_eq!(result["taskCount"], 2);
        assert_eq!(result["bugCount"], 1);
    }

    #[test]
    fn stale_generation_and_consumed_notifications_cannot_open_an_entity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        store
            .save("connection", json!({"id":"c", "name":"fixture"}))
            .unwrap();
        store.conn.lock().unwrap().execute("INSERT INTO assignment_notifications(id,connection_id,account,kind,remote_id,entity_id,title,created_at) VALUES('notice','c','me','task','1','task','fixture',1)", []).unwrap();
        let scope = Scope::from_store(&store);
        assert!(current_item(&store, &scope, "notice").unwrap().is_some());
        let mut stale = scope.clone();
        stale.generation = dir.path().join("old-generation");
        assert!(current_item(&store, &stale, "notice").unwrap().is_none());
        assert!(require_generation(&store, "old-generation").is_err());
        assert!(require_generation(&store, &generation_key(&store)).is_ok());
        assert!(require_workspace(&store, "old-workspace").is_err());
        assert_eq!(assignment_inbox::list(&store).unwrap().len(), 1);
        assignment_inbox::mark_read(&store, Some("notice")).unwrap();
        assert!(current_item(&store, &scope, "notice").unwrap().is_none());
    }
}
