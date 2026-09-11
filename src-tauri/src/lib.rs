mod agent_presets;
mod credential_vault;
mod data;
mod dingtalk_calendar;
mod integrations;
mod knowledge;
mod local_skills;
mod mail;
mod mail_history;
mod mcp;
mod parser;
mod provider_models;
pub mod storage;
mod streaming;
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
    state.store.lock().map_err(|e| e.to_string())?.snapshot()
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
commands!(save_project,delete_project,"project";save_app,delete_app,"app";save_category,delete_category,"category";save_provider,delete_provider,"provider";save_model,delete_model,"model";save_agent,delete_agent,"agent";save_knowledge,delete_knowledge,"knowledge";save_document,delete_document,"document";save_conversation,delete_conversation,"conversation";save_message,delete_message,"message");
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
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .backup(std::path::Path::new(&destination))
}
#[tauri::command]
fn storage_move(state: State<AppState>, target: String) -> Result<Value, String> {
    integrations::ensure_idle()?;
    mail::ensure_idle()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let next = store.migrate_copy(std::path::Path::new(&target))?;
    next.activate_default_bootstrap()?;
    *store = next;
    Ok(store.info())
}
#[tauri::command]
fn data_restore(state: State<AppState>, archive: String, target: String) -> Result<Value, String> {
    integrations::ensure_idle()?;
    mail::ensure_idle()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
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
        .setup(|app| {
            let store = storage::Store::open_default().map_err(std::io::Error::other)?;
            app.manage(AppState {
                store: Mutex::new(store),
            });
            tray::init(app).map_err(std::io::Error::other)?;
            mail::start_background(app.handle().clone());
            zentao::background::start(app.handle().clone());
            dingtalk_calendar::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(tray::on_window_event)
        .invoke_handler(tauri::generate_handler![
            workspace_snapshot,
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
            integrations::credential_set,
            integrations::provider_test,
            provider_models::provider_models,
            provider_models::provider_models_import,
            integrations::chat_send,
            integrations::chat_cancel,
            zentao::zentao_sync,
            zentao::media::zentao_task_image_read,
            zentao::media::zentao_task_image_upload,
            zentao::bugs::zentao_bugs_sync,
            zentao::bugs::zentao_bug_save,
            zentao::bugs::zentao_bug_transition,
            knowledge::knowledge_import,
            knowledge::knowledge_retry,
            knowledge::knowledge_export,
            knowledge::knowledge_index
        ])
        .build(tauri::generate_context!())
        .expect("无法启动栖点")
        .run(tray::on_run_event);
}
