mod agent_presets;
mod assignment_inbox;
mod assignment_notifications;
mod backup;
mod backup_cron;
mod bug_steps;
mod credential_vault;
mod data;
mod dingtalk_calendar;
mod exchange_rates;
mod image_actions;
mod integrations;
mod knowledge;
mod local_bugs;
mod local_skills;
mod mail;
mod mail_history;
mod mcp;
mod meetings;
mod meeting_realtime;
mod natapp;
mod parser;
mod prompt_optimizer;
mod provider_models;
mod s3_backup;
mod shared_tables;
pub mod storage;
mod streaming;
mod task_reminders;
mod tray;
mod updater;
mod website_icon;
mod zentao;
mod zentao_auth;
use serde_json::Value;
use std::sync::Mutex;
use tauri::{Manager, State};
pub struct AppState {
    pub store: Mutex<storage::Store>,
}
#[tauri::command]
fn workspace_snapshot(state: State<AppState>) -> Result<Value, String> {
    state.store.lock().map_err(|e| e.to_string())?.workspace_snapshot()
}
#[tauri::command]
fn storage_info(state: State<AppState>) -> Result<Value, String> {
    Ok(state.store.lock().map_err(|e| e.to_string())?.info())
}
macro_rules! commands {
 ($($save:ident,$delete:ident,$kind:literal);*$(;)?)=>{$(
 #[tauri::command] fn $save(state:State<AppState>,value:Value)->Result<Value,String>{state.store.lock().map_err(|e|e.to_string())?.save($kind,value)}
 #[tauri::command] fn $delete(state:State<AppState>,id:String,revision:Option<i64>)->Result<(),String>{state.store.lock().map_err(|e|e.to_string())?.delete($kind,&id,revision)}
 )*};
}
commands!(save_project,delete_project,"project";save_app,delete_app,"app";save_category,delete_category,"category";save_model,delete_model,"model";save_agent,delete_agent,"agent";save_knowledge,delete_knowledge,"knowledge";save_document,delete_document,"document";save_conversation,delete_conversation,"conversation";save_message,delete_message,"message");
#[tauri::command]
fn save_provider(
    state: State<AppState>,
    value: Value,
    secret: Option<String>,
) -> Result<Value, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .save_provider_with_secret(value, secret.as_deref())
}
#[tauri::command]
fn delete_provider(
    state: State<AppState>,
    id: String,
    revision: Option<i64>,
) -> Result<(), String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .delete("provider", &id, revision)
}
#[tauri::command]
fn save_connection(state: State<AppState>, value: Value) -> Result<Value, String> {
    zentao_auth::save_existing(state, value)
}
#[tauri::command]
fn delete_connection(
    state: State<AppState>,
    id: String,
    revision: Option<i64>,
) -> Result<(), String> {
    zentao_auth::delete_connection(state, id, revision)
}
#[tauri::command]
async fn save_task(state: State<'_, AppState>, value: Value) -> Result<Value, String> {
    zentao::save_task(state, value).await
}
#[tauri::command]
fn delete_task(state: State<AppState>, id: String, revision: Option<i64>) -> Result<(), String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .delete("task", &id, revision)
}
#[tauri::command]
fn data_backup(state: State<AppState>, destination: String) -> Result<(), String> {
    let _backup_guard = backup::maintenance_guard()?;
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .backup(std::path::Path::new(&destination))
}
#[tauri::command]
fn storage_move(
    state: State<AppState>,
    manager: State<natapp::NatappManager>,
    target: String,
) -> Result<Value, String> {
    let _backup_guard = backup::maintenance_guard()?;
    let _tools_guard = manager.maintenance_guard()?;
    integrations::ensure_idle()?;
    mail::ensure_idle()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    meeting_realtime::ensure_all_idle()?;
    let next = store.migrate_copy(std::path::Path::new(&target))?;
    next.activate_default_bootstrap()?;
    *store = next;
    Ok(store.info())
}
#[tauri::command]
fn data_restore(
    state: State<AppState>,
    manager: State<natapp::NatappManager>,
    archive: String,
    target: String,
) -> Result<Value, String> {
    let _backup_guard = backup::maintenance_guard()?;
    let _tools_guard = manager.maintenance_guard()?;
    integrations::ensure_idle()?;
    mail::ensure_idle()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    meeting_realtime::ensure_all_idle()?;
    let next = storage::Store::restore_to(
        std::path::Path::new(&archive),
        std::path::Path::new(&target),
    )?;
    next.activate_default_bootstrap()?;
    *store = next;
    Ok(store.info())
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(natapp::NatappManager::default())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("desktop-interaction")
                // 在文档加载前覆盖所有 frame，不放宽邮件 iframe 的沙箱限制。
                .js_init_script_on_all_frames(include_str!("desktop-interaction.js"))
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let store = storage::Store::open_default().map_err(std::io::Error::other)?;
            if let Err(error) = app
                .state::<natapp::NatappManager>()
                .cleanup_stale_configs(&store.root.join("tools/natapp"))
            {
                eprintln!("NATAPP 临时配置清理失败：{error}");
            }
            app.manage(AppState {
                store: Mutex::new(store),
            });
            tray::init(app).map_err(std::io::Error::other)?;
            mail::start_background(app.handle().clone());
            zentao::background::start(app.handle().clone());
            dingtalk_calendar::start(app.handle().clone());
            task_reminders::start(app.handle().clone());
            assignment_notifications::start(app.handle().clone());
            backup::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(tray::on_window_event)
        .invoke_handler(tauri::generate_handler![
            assignment_notifications::assignment_notification_list,
            assignment_notifications::assignment_notification_read,
            assignment_notifications::assignment_notification_read_entity,
            exchange_rates::get_exchange_rates,
            workspace_snapshot,
            backup::backup_state,
            backup::backup_save,
            backup::backup_test,
            backup::backup_refresh,
            backup::backup_create,
            backup::backup_verify,
            backup::backup_retry,
            backup::backup_restore,
            backup::backup_download,
            task_reminders::task_reminder_permission,
            tray::tray_preferences,
            tray::tray_preferences_save,
            mail::mail_accounts,
            mail::mail_account_save,
            mail::mail_account_delete,
            mail::mail_account_test,
            mail::mail_sync,
            mail::mail_folders,
            mail::mail_messages,
            mail::mail_message,
            mail::mail_message_flag,
            mail::mail_message_move,
            mail::mail_attachment_save,
            mail::mail_drafts,
            mail::mail_draft_save,
            mail::mail_draft_delete,
            mail::mail_send,
            mail::mail_unread,
            mail::mail_unread_count,
            mail_history::mail_delivery_history,
            updater::updater_status,
            updater::update_prepare,
            zentao_auth::zentao_connect,
            zentao_auth::zentao_auth_preferences,
            website_icon::website_icon_fetch,
            mcp::mcp_list,
            mcp::mcp_save,
            mcp::mcp_delete,
            dingtalk_calendar::dingtalk_calendar_state,
            dingtalk_calendar::dingtalk_calendar_save,
            dingtalk_calendar::dingtalk_calendar_sync,
            storage_info,
            natapp::natapp_state,
            natapp::natapp_install,
            natapp::natapp_save,
            natapp::natapp_delete,
            natapp::natapp_start,
            natapp::natapp_stop,
            natapp::natapp_logs,
            save_task,
            delete_task,
            save_project,
            delete_project,
            save_app,
            delete_app,
            save_category,
            delete_category,
            save_provider,
            delete_provider,
            save_model,
            delete_model,
            save_agent,
            delete_agent,
            local_skills::local_skills_import,
            local_skills::local_skills_set_enabled,
            local_skills::save_skill,
            local_skills::delete_skill,
            save_knowledge,
            delete_knowledge,
            save_document,
            delete_document,
            save_conversation,
            delete_conversation,
            save_message,
            delete_message,
            save_connection,
            delete_connection,
            data_backup,
            storage_move,
            data_restore,
            integrations::provider_test,
            provider_models::provider_models,
            provider_models::provider_models_import,
            integrations::chat_send,
            integrations::chat_set_model,
            integrations::chat_cancel,
            prompt_optimizer::prompt_list,
            prompt_optimizer::prompt_save,
            prompt_optimizer::prompt_delete,
            prompt_optimizer::prompt_generate,
            prompt_optimizer::prompt_cancel,
            meeting_realtime::meeting_realtime_start,
            meeting_realtime::meeting_realtime_push,
            meeting_realtime::meeting_realtime_stop,
            meetings::meeting_list,
            meetings::meeting_save,
            meetings::meeting_delete,
            meetings::meeting_append_audio,
            meetings::meeting_transcribe,
            meetings::meeting_generate,
            meetings::meeting_audio,
            meetings::meeting_action_complete,
            meetings::meeting_action_task,
            meetings::meeting_add_marker,
            zentao::zentao_sync,
            zentao::assignment::zentao_task_assignment,
            zentao::bug_history::zentao_bug_history,
            zentao::media::zentao_task_image_read,
            zentao::media::zentao_bug_image_read,
            zentao::media::zentao_bug_history_image_read,
            image_actions::image_clipboard_write,
            image_actions::image_download,
            zentao::media::zentao_task_image_upload,
            local_bugs::local_bug_save,
            shared_tables::list_shared_tables,
            shared_tables::save_shared_table,
            shared_tables::delete_shared_table,
            shared_tables::list_shared_table_groups,
            shared_tables::save_shared_table_group,
            shared_tables::delete_shared_table_group,
            zentao::bug_create::zentao_bug_create_options,
            zentao::bug_create::zentao_bug_create,
            zentao::bugs::zentao_bugs_sync,
            zentao::bugs::zentao_bug_save,
            zentao::users::zentao_bug_assignees,
            zentao::bugs::zentao_bug_transition,
            knowledge::knowledge_import,
            knowledge::knowledge_retry,
            knowledge::knowledge_export,
            knowledge::knowledge_index
        ])
        .build(tauri::generate_context!())
        .expect("无法启动栖点")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<natapp::NatappManager>().shutdown();
            }
            tray::on_run_event(app, event);
        });
}
