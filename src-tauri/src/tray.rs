use crate::AppState;
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, State, Window, WindowEvent,
};

const TRAY_ID: &str = "perch-status";

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    #[default]
    Hide,
    Close,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSettings {
    #[serde(default)]
    close_behavior: CloseBehavior,
}

pub struct TrayState {
    path: PathBuf,
    settings: Mutex<DeviceSettings>,
    available: AtomicBool,
    warning: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPreferences {
    close_behavior: CloseBehavior,
    tray_available: bool,
    warning: Option<String>,
}

fn load_settings(path: &Path) -> Result<DeviceSettings, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "本机窗口设置损坏，暂时使用默认关闭行为".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DeviceSettings::default()),
        Err(_) => Err("无法读取本机窗口设置，暂时使用默认关闭行为".into()),
    }
}

fn save_settings(path: &Path, settings: &DeviceSettings) -> Result<(), String> {
    let parent = path.parent().ok_or("设置路径无效")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建本机设置目录")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法创建本机设置文件")?;
    let bytes = serde_json::to_vec(settings).map_err(|_| "无法编码本机设置")?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "无法写入本机设置")?;
    // 同目录原子替换，在 Windows 上也支持替换已存在文件。
    file.persist(path).map_err(|_| "无法保存本机窗口设置")?;
    Ok(())
}

#[tauri::command]
pub fn tray_preferences(state: State<TrayState>) -> Result<TrayPreferences, String> {
    Ok(TrayPreferences {
        close_behavior: state
            .settings
            .lock()
            .map_err(|_| "窗口设置暂不可用")?
            .close_behavior,
        tray_available: state.available.load(Ordering::Relaxed),
        warning: state.warning.clone(),
    })
}

#[tauri::command]
pub fn tray_preferences_save(
    state: State<TrayState>,
    close_behavior: CloseBehavior,
) -> Result<TrayPreferences, String> {
    let mut settings = state.settings.lock().map_err(|_| "窗口设置暂不可用")?;
    let next = DeviceSettings { close_behavior };
    save_settings(&state.path, &next)?;
    *settings = next;
    drop(settings);
    tray_preferences(state)
}

fn date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn schedule_date(task: &Value, field: &str) -> Option<NaiveDate> {
    let schedule = task.get("schedule")?;
    let value = schedule.get(field)?.as_str()?;
    if schedule.get("kind").and_then(Value::as_str) == Some("all_day") {
        // 全日任务使用半开区间，结束日期的零点不再属于任务。
        date(value).and_then(|day| {
            if field == "end" {
                day.pred_opt()
            } else {
                Some(day)
            }
        })
    } else {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.with_timezone(&Local).date_naive())
    }
}

fn is_overdue(task: &Value, now: DateTime<chrono::Utc>) -> bool {
    if task["status"] == "closed" {
        return false;
    }
    let schedule = &task["schedule"];
    let Some(end) = schedule["end"].as_str() else {
        return false;
    };
    if schedule["kind"] == "all_day" {
        let timezone = schedule["timezone"]
            .as_str()
            .unwrap_or("Asia/Shanghai")
            .parse::<chrono_tz::Tz>();
        return match (date(end), timezone) {
            (Some(end), Ok(tz)) => now.with_timezone(&tz).date_naive() >= end,
            _ => false,
        };
    }
    DateTime::parse_from_rfc3339(end).is_ok_and(|end| now >= end)
}

fn summary_at(snapshot: &Value, now: DateTime<chrono::Utc>) -> String {
    let today = now.with_timezone(&Local).date_naive();
    let empty = Vec::new();
    let tasks = snapshot
        .get("tasks")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let mut today_count = 0;
    let mut doing = 0;
    let mut overdue = 0;
    let mut candidates = Vec::new();
    for task in tasks {
        let status = task.get("status").and_then(Value::as_str).unwrap_or("");
        let late = is_overdue(task, now);
        overdue += usize::from(late);
        if status != "todo" && status != "doing" {
            continue;
        }
        let start = schedule_date(task, "start");
        let end = schedule_date(task, "end").or(start);
        let planned_today =
            matches!((start, end), (Some(start), Some(end)) if start <= today && end >= today);
        today_count += usize::from(planned_today);
        doing += usize::from(status == "doing");
        candidates.push((!late, !planned_today, status != "doing", task));
    }
    candidates.sort_by_key(|(late, today, doing, task)| {
        (
            *late,
            *today,
            *doing,
            task.get("sortOrder").and_then(Value::as_i64).unwrap_or(0),
        )
    });
    let mut text = format!("栖点 · 今日待办 {today_count} · 正在做 {doing} · 逾期 {overdue}");
    for (_, _, _, task) in candidates.into_iter().take(3) {
        let title = task
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("未命名任务");
        let title: String = title
            .chars()
            .filter(|ch| !ch.is_control())
            .take(18)
            .collect();
        text.push_str(&format!("\n• {title}"));
    }
    if today_count == 0 && doing == 0 && overdue == 0 && tasks.is_empty() {
        text.push_str("\n暂无任务，开始安排今天吧");
    }
    text
}

fn refresh_summary(app: &AppHandle) {
    let snapshot = app
        .state::<AppState>()
        .store
        .lock()
        .ok()
        .and_then(|store| store.snapshot().ok());
    let text = snapshot
        .map(|snapshot| summary_at(&snapshot, chrono::Utc::now()))
        .unwrap_or_else(|| "栖点 · 任务摘要暂不可用".into());
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn on_run_event(app: &AppHandle, event: tauri::RunEvent) {
    #[cfg(target_os = "macos")]
    if matches!(event, tauri::RunEvent::Reopen { .. }) {
        show_main(app);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray-open", "打开栖点", true, None::<&str>)?;
    let tasks = MenuItem::with_id(app, "tray-tasks", "查看任务待办", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "tray-quit", "退出栖点", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &tasks, &separator, &quit])?;
    // 复用打包配置中的应用图标，避免托盘与桌面使用不同的品牌资源。
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| std::io::Error::other("应用图标未配置，无法创建托盘"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(false)
        .tooltip("栖点 · 正在载入任务摘要")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open" => show_main(app),
            "tray-tasks" => {
                show_main(app);
                let _ = app.emit("tray-navigate", "/tasks");
            }
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main(tray.app_handle()),
            TrayIconEvent::Enter { .. } => {
                let app = tray.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || refresh_summary(&app));
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn init(app: &mut App) -> Result<(), String> {
    let path = app
        .path()
        .app_config_dir()
        .map_err(|_| "无法定位本机设置目录")?
        .join("device-settings.json");
    let (settings, mut warning) = match load_settings(&path) {
        Ok(settings) => (settings, None),
        Err(error) => (DeviceSettings::default(), Some(error)),
    };
    let available = create_tray(app.handle()).is_ok();
    if !available {
        warning = Some("系统托盘不可用，关闭窗口将退出应用，请重新启动后再试。".into());
    }
    app.manage(TrayState {
        path,
        settings: Mutex::new(settings),
        available: AtomicBool::new(available),
        warning,
    });
    if available {
        let app = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let handle = app.clone();
                let _ =
                    tauri::async_runtime::spawn_blocking(move || refresh_summary(&handle)).await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }
    Ok(())
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        let state = window.app_handle().state::<TrayState>();
        let hide = state.available.load(Ordering::Relaxed)
            && state
                .settings
                .lock()
                .map(|settings| settings.close_behavior == CloseBehavior::Hide)
                .unwrap_or(false);
        if hide {
            // 隐藏失败时不拦截关闭，避免留下无法恢复的后台进程。
            if window.hide().is_ok() {
                api.prevent_close();
            }
        } else {
            window.app_handle().exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn device_settings_default_and_atomic_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("device-settings.json");
        assert_eq!(
            load_settings(&path).unwrap().close_behavior,
            CloseBehavior::Hide
        );
        save_settings(
            &path,
            &DeviceSettings {
                close_behavior: CloseBehavior::Close,
            },
        )
        .unwrap();
        assert_eq!(
            load_settings(&path).unwrap().close_behavior,
            CloseBehavior::Close
        );
        save_settings(&path, &DeviceSettings::default()).unwrap();
        assert_eq!(
            load_settings(&path).unwrap().close_behavior,
            CloseBehavior::Hide
        );
        fs::write(&path, "bad").unwrap();
        assert!(load_settings(&path).is_err());
    }
    fn summary(snapshot: &Value, today: NaiveDate) -> String {
        use chrono::TimeZone;
        summary_at(
            snapshot,
            Local
                .from_local_datetime(&today.and_hms_opt(12, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&chrono::Utc),
        )
    }
    #[test]
    fn summary_excludes_closed_expiry_and_sorts_open_tasks() {
        let value = json!({"tasks":[
            {"title":"今日计划", "status":"todo", "schedule":{"kind":"all_day","start":"2026-09-08"}},
            {"title":"过期任务", "status":"doing", "schedule":{"kind":"all_day","start":"2026-09-07","end":"2026-09-08","timezone":"Asia/Shanghai"}},
            {"title":"已完成", "status":"done", "schedule":{"kind":"all_day","start":"2026-09-01","end":"2026-09-02","timezone":"Asia/Shanghai"}},
            {"title":"已关闭", "status":"closed", "schedule":{"kind":"all_day","start":"2026-09-01","end":"2026-09-02","timezone":"Asia/Shanghai"}},
            {"title":"错误日期", "status":"todo", "schedule":{"kind":"all_day","end":"2026-02-30"}}
        ]});
        let result = summary(&value, date("2026-09-08").unwrap());
        assert!(result.contains("今日待办 1 · 正在做 1 · 逾期 2"));
        assert!(result.lines().nth(1).unwrap().contains("过期任务"));
        assert!(!result.contains("已完成"));
        assert!(!result.contains("已关闭"));
    }
    #[test]
    fn summary_limits_untrusted_titles_and_includes_multiday() {
        let value = json!({"tasks":[{"title":"任\n务".repeat(100),"status":"todo","schedule":{"kind":"all_day","start":"2026-09-07","end":"2026-09-09"}}]});
        let result = summary(&value, date("2026-09-08").unwrap());
        assert!(result.contains("今日待办 1"));
        assert_eq!(result.lines().count(), 2);
        assert!(result.chars().count() < 100);
        assert!(summary(&value, date("2026-09-09").unwrap()).contains("今日待办 0"));
        assert!(summary(&json!({}), date("2026-09-08").unwrap()).contains("暂无任务"));
    }
}
