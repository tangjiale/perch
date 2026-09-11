use crate::AppState;
use serde_json::Value;
use tauri::State;

// 保留 SMTP 接受、发送中及未确认记录，避免失败草稿消失后无法核对是否已投递。
#[tauri::command]
pub fn mail_delivery_history(
    state: State<AppState>,
    account_id: String,
) -> Result<Vec<Value>, String> {
    let store = state.store.lock().map_err(|_| "工作空间繁忙")?;
    let conn = store.conn.lock().map_err(|_| "数据库繁忙")?;
    let mut query = conn.prepare("SELECT data,state FROM mail_drafts WHERE account_id=?1 AND state!='draft' ORDER BY rowid DESC LIMIT 200").map_err(|_| "无法读取发送记录")?;
    let rows = query
        .query_map([account_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|_| "无法读取发送记录")?;
    rows.map(|row| {
        let (data, status) = row.map_err(|_| "无法读取发送记录")?;
        let mut value: Value = serde_json::from_str(&data).map_err(|_| "发送记录格式无效")?;
        value["deliveryState"] = Value::String(status);
        Ok(value)
    })
    .collect()
}
