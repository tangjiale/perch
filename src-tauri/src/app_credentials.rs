use crate::{credential_vault, data::text, storage::Store, AppState};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppCredentials {
    pub account: String,
    pub password: String,
}

impl AppCredentials {
    pub(crate) fn is_empty(&self) -> bool {
        self.account.is_empty() && self.password.is_empty()
    }

    fn purpose(app_id: &str) -> String {
        format!("app:{app_id}:credentials")
    }

    pub(crate) fn read(store: &Store, app_id: &str) -> Result<Self, String> {
        let conn = store.conn.lock().map_err(|error| error.to_string())?;
        let encrypted: Option<String> = conn
            .query_row(
                "SELECT c.encrypted FROM apps a LEFT JOIN app_credentials c ON c.app_id=a.id WHERE a.id=?1",
                [app_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| "无法读取应用账号和密码。")?
            .ok_or("此应用已被删除，请刷新应用列表。")?;
        match encrypted {
            Some(encrypted) => {
                let value = credential_vault::decrypt_json(
                    &store.workspace_id,
                    &Self::purpose(app_id),
                    &encrypted,
                )?;
                serde_json::from_value(value).map_err(|_| "应用凭据格式无效。".into())
            }
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn save(
        &self,
        tx: &Transaction<'_>,
        workspace_id: &str,
        app_id: &str,
    ) -> Result<(), String> {
        if self.account.len() > 128 * 1024 || self.password.len() > 128 * 1024 {
            return Err("应用账号或密码过长。".into());
        }
        if self.is_empty() {
            tx.execute("DELETE FROM app_credentials WHERE app_id=?1", [app_id])
                .map_err(|_| "无法清除应用账号和密码。")?;
        } else {
            let value = serde_json::to_value(self).map_err(|_| "应用凭据格式无效。")?;
            let encrypted =
                credential_vault::encrypt_json(workspace_id, &Self::purpose(app_id), &value)?;
            tx.execute(
                "INSERT INTO app_credentials(app_id,encrypted) VALUES(?1,?2)
                 ON CONFLICT(app_id) DO UPDATE SET encrypted=excluded.encrypted",
                params![app_id, encrypted],
            )
            .map_err(|_| "无法保存应用账号和密码。")?;
        }
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn get_app_credentials(
    state: State<'_, AppState>,
    workspace_id: String,
    app_id: String,
) -> Result<AppCredentials, String> {
    let store = state.store.lock().map_err(|error| error.to_string())?;
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开应用。".into());
    }
    AppCredentials::read(&store, &app_id)
}

#[tauri::command]
pub(crate) fn save_app_details(
    state: State<'_, AppState>,
    workspace_id: String,
    value: Value,
    credentials: AppCredentials,
) -> Result<Value, String> {
    let store = state.store.lock().map_err(|error| error.to_string())?;
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开应用。".into());
    }
    save_details(&store, value, &credentials)
}

fn save_details(
    store: &Store,
    value: Value,
    credentials: &AppCredentials,
) -> Result<Value, String> {
    let mut conn = store.conn.lock().map_err(|error| error.to_string())?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let saved = Store::save_in_transaction(&tx, &store.workspace_id, "apps", value, false)?;
    credentials.save(&tx, &store.workspace_id, text(&saved, "id"))?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_credentials_are_encrypted_and_excluded_from_app_data() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let legacy = store
            .save(
                "app",
                json!({"name":"已有应用","url":"https://example.test"}),
            )
            .unwrap();
        let app_id = text(&legacy, "id");
        assert!(AppCredentials::read(&store, app_id).unwrap().is_empty());

        let credentials = AppCredentials {
            account: "test-account-0021".into(),
            password: " test-password-71 ".into(),
        };
        let saved = save_details(&store, legacy.clone(), &credentials).unwrap();
        let read = AppCredentials::read(&store, app_id).unwrap();
        assert_eq!(read.account, credentials.account);
        assert_eq!(read.password, credentials.password);
        assert_eq!(
            save_details(&store, legacy.clone(), &AppCredentials::default()).unwrap_err(),
            "数据已被修改，请刷新后重试"
        );
        assert_eq!(
            AppCredentials::read(&store, app_id).unwrap().password,
            credentials.password
        );

        let snapshot = store.workspace_snapshot().unwrap();
        assert_eq!(snapshot["apps"][0], saved);
        assert!(!saved.to_string().contains(&credentials.account));
        assert!(!saved.to_string().contains(&credentials.password));
        let encrypted: String = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT encrypted FROM app_credentials WHERE app_id=?1",
                [app_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!encrypted.contains(&credentials.account));
        assert!(!encrypted.contains(&credentials.password));

        let mut changed = saved.clone();
        changed["favorite"] = json!(true);
        let changed = store.save("app", changed).unwrap();
        assert_eq!(
            AppCredentials::read(&store, app_id).unwrap().account,
            credentials.account
        );
        store
            .delete("app", app_id, changed["revision"].as_i64())
            .unwrap();
        let count: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM app_credentials", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn app_credentials_can_be_cleared_and_are_bound_to_the_application() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let first = save_details(
            &store,
            json!({"name":"一","url":"https://example.test/one"}),
            &AppCredentials {
                account: "0007".into(),
                password: "sample-secret".into(),
            },
        )
        .unwrap();
        let second = store
            .save("app", json!({"name":"二","url":"https://example.test/two"}))
            .unwrap();
        let first_id = text(&first, "id");
        let second_id = text(&second, "id");
        store.conn.lock().unwrap()
            .execute(
                "INSERT INTO app_credentials(app_id, encrypted) SELECT ?1,encrypted FROM app_credentials WHERE app_id=?2",
                [second_id, first_id],
            ).unwrap();
        assert!(AppCredentials::read(&store, second_id).is_err());
        save_details(&store, first.clone(), &AppCredentials::default()).unwrap();
        assert!(AppCredentials::read(&store, first_id).unwrap().is_empty());
    }
}
