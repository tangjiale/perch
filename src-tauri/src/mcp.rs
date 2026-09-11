use crate::{storage::Store, AppState};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use tauri::State;

const PREFIX: &str = "mcp.connection.";

trait Secrets {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, value: Option<&str>) -> Result<(), String>;
}

impl Secrets for crate::credential_vault::Entry {
    fn read(&self) -> Result<Option<String>, String> {
        match self.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取 MCP 凭据，请检查系统钥匙串权限".into()),
        }
    }
    fn write(&self, value: Option<&str>) -> Result<(), String> {
        match value {
            Some(value) => self
                .set_password(value)
                .map_err(|_| "无法保存 MCP 凭据".into()),
            None => match self.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err("无法清除 MCP 凭据".into()),
            },
        }
    }
}

fn entry(store: &Store, id: &str) -> Result<crate::credential_vault::Entry, String> {
    crate::credential_vault::entry(
        &store.workspace_id,
        "com.self.workbench",
        &format!("mcp:{id}"),
    )
    .map_err(|_| "系统钥匙串不可用".into())
}

fn normalize(value: &Value) -> Result<Value, String> {
    let object = value.as_object().ok_or("MCP 配置必须是对象")?;
    let id = value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    if id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("MCP 配置 ID 无效".into());
    }
    let name = value["name"].as_str().unwrap_or("").trim();
    if name.is_empty() || name.len() > 256 {
        return Err("请输入有效的 MCP 名称".into());
    }
    let transport = value["transport"].as_str().unwrap_or("");
    let args = value["args"].as_array().ok_or("启动参数必须是字符串数组")?;
    if args.len() > 256
        || args.iter().any(|arg| {
            arg.as_str()
                .is_none_or(|s| s.len() > 8192 || s.contains('\0'))
        })
    {
        return Err("启动参数必须是有效的字符串数组".into());
    }
    let enabled = value["enabled"].as_bool().ok_or("启用状态必须是布尔值")?;
    let revision = match object.get("revision") {
        None => 0,
        Some(value) => value.as_i64().filter(|v| *v >= 0).ok_or("版本号无效")?,
    };
    let (url, command) = match transport {
        "streamable-http" => {
            let raw = value["url"].as_str().unwrap_or("").trim();
            let url = url::Url::parse(raw).map_err(|_| "MCP 地址无效")?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("MCP 地址仅支持 HTTP/HTTPS，不能包含凭据、查询参数或片段".into());
            }
            (url.to_string(), String::new())
        }
        "stdio" => {
            let command = value["command"].as_str().unwrap_or("").trim();
            if command.is_empty() || command.len() > 4096 || command.contains(['\0', '\n', '\r']) {
                return Err("请输入有效的启动命令".into());
            }
            (String::new(), command.to_string())
        }
        _ => return Err("不支持的 MCP 传输方式".into()),
    };
    // 只保存公开配置字段，避免将额外的 headers/env 或令牌写入数据库。
    Ok(
        json!({"id":id,"name":name,"transport":transport,"url":url,"command":command,
        "args":if transport == "stdio" {args.clone()} else {vec![]},"enabled":enabled,"revision":revision}),
    )
}

fn normalize_secret(raw: &str) -> Result<Option<String>, String> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    if raw.len() > 65536 {
        return Err("MCP 凭据超过大小限制".into());
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "MCP 凭据必须是合法 JSON")?;
    let object = value.as_object().ok_or("MCP 凭据必须是对象")?;
    if object.keys().any(|key| key != "headers" && key != "env") {
        return Err("MCP 凭据仅支持 headers 和 env 字段".into());
    }
    for (kind, fields) in object {
        let fields = fields
            .as_object()
            .ok_or("headers 和 env 必须是字符串字典")?;
        for (key, value) in fields {
            let value = value.as_str().ok_or("凭据值必须是字符串")?;
            if key.is_empty()
                || key.contains(['\0', '\n', '\r', '='])
                || value.contains('\0')
                || (kind == "headers"
                    && (value.contains(['\n', '\r'])
                        || reqwest::header::HeaderName::from_bytes(key.as_bytes()).is_err()))
            {
                return Err("MCP 凭据包含无效的名称或值".into());
            }
        }
    }
    if object
        .values()
        .all(|fields| fields.as_object().is_some_and(|v| v.is_empty()))
    {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn list(store: &Store) -> Result<Vec<Value>, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let mut statement = conn
        .prepare("SELECT value FROM settings WHERE key GLOB 'mcp.connection.*' ORDER BY key")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    })
    .collect()
}

fn save(
    store: &Store,
    value: Value,
    secret: Option<&str>,
    secrets: &impl Secrets,
) -> Result<Value, String> {
    let mut value = normalize(&value)?;
    let next_secret = secret.map(normalize_secret).transpose()?;
    let key = format!("{PREFIX}{}", value["id"].as_str().unwrap());
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let old: Option<String> = tx
        .query_row("SELECT value FROM settings WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let old: Option<Value> = old
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|e| e.to_string())?;
    let revision = old
        .as_ref()
        .and_then(|value| value["revision"].as_i64())
        .unwrap_or(0);
    if value["revision"].as_i64() != Some(revision) {
        return Err("MCP 配置已被修改，请刷新后重试".into());
    }
    value["revision"] = json!(revision.checked_add(1).ok_or("版本号超出范围")?);
    value["hasSecrets"] = json!(next_secret
        .as_ref()
        .map(|v| v.is_some())
        .unwrap_or_else(|| old.as_ref().is_some_and(|v| v["hasSecrets"] == true)));
    tx.execute("INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value.to_string()]).map_err(|e| e.to_string())?;
    let previous = if next_secret.is_some() {
        secrets.read()?
    } else {
        None
    };
    if let Some(next) = &next_secret {
        secrets.write(next.as_deref())?;
    }
    if let Err(error) = tx.commit() {
        if next_secret.is_some() && secrets.write(previous.as_deref()).is_err() {
            return Err("配置提交失败，凭据恢复失败，请重新配置 MCP 凭据".into());
        }
        return Err(error.to_string());
    }
    Ok(value)
}

fn delete(store: &Store, id: &str, revision: i64, secrets: &impl Secrets) -> Result<(), String> {
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let key = format!("{PREFIX}{id}");
    let raw: Option<String> = tx
        .query_row("SELECT value FROM settings WHERE key=?1", [&key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let old: Value =
        serde_json::from_str(&raw.ok_or("MCP 配置不存在")?).map_err(|e| e.to_string())?;
    if old["revision"].as_i64() != Some(revision) {
        return Err("MCP 配置已被修改，请刷新后重试".into());
    }
    tx.execute("DELETE FROM settings WHERE key=?1", [&key])
        .map_err(|e| e.to_string())?;
    let previous = secrets.read()?;
    secrets.write(None)?;
    if let Err(error) = tx.commit() {
        if secrets.write(previous.as_deref()).is_err() {
            return Err("删除提交失败，凭据恢复失败，请重新配置 MCP 凭据".into());
        }
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn mcp_list(state: State<AppState>) -> Result<Vec<Value>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    list(&store)
}

#[tauri::command]
pub fn mcp_save(
    state: State<AppState>,
    value: Value,
    secret_json: Option<String>,
) -> Result<Value, String> {
    let value = normalize(&value)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let secrets = entry(&store, value["id"].as_str().unwrap())?;
    save(&store, value, secret_json.as_deref(), &secrets)
}

#[tauri::command]
pub fn mcp_delete(state: State<AppState>, id: String, revision: i64) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    delete(&store, &id, revision, &entry(&store, &id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct MemorySecrets(RefCell<Option<String>>);
    impl Secrets for MemorySecrets {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.0.borrow().clone())
        }
        fn write(&self, value: Option<&str>) -> Result<(), String> {
            *self.0.borrow_mut() = value.map(str::to_string);
            Ok(())
        }
    }
    fn config() -> Value {
        json!({"id":"test-mcp","name":"本地工具","transport":"stdio","command":"npx","args":["server"],"enabled":true})
    }
    #[test]
    fn persistence_conflicts_and_secret_isolation() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open_at(temp.path().join("data")).unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key,value) VALUES ('theme','\"light\"')",
                [],
            )
            .unwrap();
        let secrets = MemorySecrets::default();
        let value = save(
            &store,
            config(),
            Some(r#"{"env":{"TOKEN":"private-secret"}}"#),
            &secrets,
        )
        .unwrap();
        assert_eq!(value["revision"], 1);
        assert_eq!(value["hasSecrets"], true);
        assert!(!list(&store).unwrap()[0]
            .to_string()
            .contains("private-secret"));
        assert!(save(&store, config(), None, &secrets).is_err());
        assert!(delete(&store, "test-mcp", 0, &secrets).is_err());
        let root = store.root.clone();
        drop(store);
        let store = Store::open_at(root).unwrap();
        assert_eq!(list(&store).unwrap(), vec![value.clone()]);
        let value = save(&store, value, Some(""), &secrets).unwrap();
        assert_eq!(value["hasSecrets"], false);
        assert_eq!(secrets.read().unwrap(), None);
        delete(&store, "test-mcp", 2, &secrets).unwrap();
        assert!(list(&store).unwrap().is_empty());
        assert_eq!(
            store
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT value FROM settings WHERE key='theme'", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "\"light\""
        );
    }
    #[test]
    fn validates_transport_and_private_fields() {
        let mut value = config();
        value["args"] = json!([1]);
        assert!(normalize(&value).is_err());
        value = config();
        value["transport"] = json!("streamable-http");
        for url in [
            "file:///tmp/x",
            "https://user:pass@example.com/mcp",
            "https://example.com/mcp?token=x",
        ] {
            value["url"] = json!(url);
            assert!(normalize(&value).is_err());
        }
        value["url"] = json!("https://example.com/mcp");
        value["headers"] = json!({"Authorization":"secret"});
        assert!(normalize(&value).unwrap().get("headers").is_none());
        assert!(normalize_secret(r#"{"headers":{"Authorization":"a\nb"}}"#).is_err());
        assert!(normalize_secret(r#"{"env":{"TOKEN":4}}"#).is_err());
        assert!(normalize_secret(r#"{"headers":{},"env":{}}"#)
            .unwrap()
            .is_none());
    }
}
