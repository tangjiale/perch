use crate::{
    credential_vault, image_actions::write_clipboard_text,
    password_vault_excel::read_password_vault_excel, storage::Store, AppState,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::IpAddr, sync::MutexGuard};
use tauri::{AppHandle, Manager};

const STALE_RECORD: &str = "记录已被修改或删除，请刷新密码库后重试。";
const MAX_FIELD_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VaultKind {
    Application,
    Server,
}

impl VaultKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Server => "server",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VaultEnvironment {
    Dev,
    Test,
    Prod,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum VaultFields {
    Application {
        app_name: String,
        app_code: String,
        address: String,
        account1: String,
        account2: String,
        notes: String,
    },
    Server {
        ip: String,
        account: String,
        environment: VaultEnvironment,
        notes: String,
    },
}

impl VaultFields {
    fn kind(&self) -> VaultKind {
        match self {
            Self::Application { .. } => VaultKind::Application,
            Self::Server { .. } => VaultKind::Server,
        }
    }

    pub(crate) fn validate(mut self) -> Result<Self, String> {
        match &mut self {
            Self::Application {
                app_name,
                app_code,
                address,
                account1,
                account2,
                notes,
            } => {
                normalize_fields([app_name, app_code, address, account1, account2, notes])?;
                if app_name.is_empty() {
                    return Err("请填写应用名称。".into());
                }
            }
            Self::Server {
                ip, account, notes, ..
            } => {
                normalize_fields([ip, account, notes])?;
                *ip = ip
                    .parse::<IpAddr>()
                    .map_err(|_| "请输入有效的 IPv4 或 IPv6 地址。")?
                    .to_string();
            }
        }
        Ok(self)
    }
}

fn normalize_fields<const N: usize>(fields: [&mut String; N]) -> Result<(), String> {
    for field in fields {
        if field.len() > MAX_FIELD_BYTES {
            return Err("字段内容过长。".into());
        }
        let trimmed = field.trim();
        if trimmed.len() != field.len() {
            *field = trimmed.to_owned();
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct VaultVersion {
    id: String,
    revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VaultCopyField {
    Account,
    Account1,
    Account2,
    Password,
}

#[derive(Deserialize)]
pub(crate) struct VaultDraft {
    id: Option<String>,
    revision: Option<i64>,
    #[serde(flatten)]
    fields: VaultFields,
    password: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Hash)]
struct Payload {
    fields: VaultFields,
    password: String,
}

impl Payload {
    fn new(fields: VaultFields, password: String) -> Result<Self, String> {
        if password.len() > MAX_FIELD_BYTES {
            return Err("密码内容过长。".into());
        }
        Ok(Self {
            fields: fields.validate()?,
            password,
        })
    }

    fn public(self, id: String, revision: i64) -> VaultEntry {
        VaultEntry {
            id,
            revision,
            fields: self.fields,
            has_password: !self.password.is_empty(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultEntry {
    id: String,
    revision: i64,
    #[serde(flatten)]
    fields: VaultFields,
    has_password: bool,
}

#[derive(Serialize)]
pub(crate) struct RevealedPassword {
    id: String,
    password: String,
}

#[derive(Default, Serialize)]
pub(crate) struct VaultImportResult {
    created: usize,
    skipped: usize,
}

fn current_store<'a>(
    state: &'a AppState,
    workspace_id: &str,
) -> Result<MutexGuard<'a, Store>, String> {
    let store = state.store.lock().map_err(|_| "无法读取工作空间。")?;
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开密码库。".into());
    }
    Ok(store)
}

async fn in_workspace<T: Send + 'static>(
    app: AppHandle,
    workspace_id: String,
    operation: impl FnOnce(&Store) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = current_store(&state, &workspace_id)?;
        operation(&store)
    })
    .await
    .map_err(|_| "密码库操作未完成，请重试。".to_owned())?
}

fn purpose(kind: VaultKind, id: &str) -> String {
    format!("password-vault:{}:{id}", kind.as_str())
}

fn seal(workspace: &str, id: &str, payload: &Payload) -> Result<String, String> {
    let value = serde_json::to_value(payload).map_err(|_| "密码库数据格式无效。")?;
    credential_vault::encrypt_json(workspace, &purpose(payload.fields.kind(), id), &value)
}

fn open(workspace: &str, kind: VaultKind, id: &str, encrypted: &str) -> Result<Payload, String> {
    let value = credential_vault::decrypt_json(workspace, &purpose(kind, id), encrypted)?;
    let payload: Payload = serde_json::from_value(value).map_err(|_| "密码库数据格式无效。")?;
    if payload.fields.kind() != kind {
        return Err("密码库记录类型无效。".into());
    }
    Ok(payload)
}

fn read_records<T>(
    conn: &Connection,
    workspace: &str,
    kind: VaultKind,
    transform: impl Fn(String, i64, Payload) -> T,
) -> Result<Vec<T>, String> {
    let mut query = conn
        .prepare(
            "SELECT id,revision,encrypted FROM password_vault WHERE kind=?1 ORDER BY rowid DESC",
        )
        .map_err(|_| "无法读取密码库。")?;
    let rows = query
        .query_map([kind.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| "无法读取密码库。")?;
    rows.map(|row| {
        let (id, revision, encrypted) = row.map_err(|_| "无法读取密码库。")?;
        let payload = open(workspace, kind, &id, &encrypted)?;
        Ok(transform(id, revision, payload))
    })
    .collect()
}

fn insert(conn: &Connection, workspace: &str, payload: &Payload) -> Result<String, String> {
    let id = uuid::Uuid::now_v7().to_string();
    let encrypted = seal(workspace, &id, payload)?;
    conn.execute(
        "INSERT INTO password_vault(id,kind,revision,encrypted) VALUES(?1,?2,1,?3)",
        params![id, payload.fields.kind().as_str(), encrypted],
    )
    .map_err(|_| "无法保存密码库记录。")?;
    Ok(id)
}

fn save_entry(store: &Store, input: VaultDraft) -> Result<VaultEntry, String> {
    let payload = Payload::new(input.fields, input.password)?;
    let conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
    let (id, revision) = match (input.id, input.revision) {
        (None, None) => (insert(&conn, &store.workspace_id, &payload)?, 1),
        (Some(id), Some(revision)) if revision > 0 => {
            let next_revision = revision.checked_add(1).ok_or(STALE_RECORD)?;
            let encrypted = seal(&store.workspace_id, &id, &payload)?;
            let count = conn.execute(
                "UPDATE password_vault SET encrypted=?1,revision=?2 WHERE id=?3 AND kind=?4 AND revision=?5",
                params![encrypted, next_revision, id, payload.fields.kind().as_str(), revision],
            ).map_err(|_| "无法更新密码库记录。")?;
            if count != 1 {
                return Err(STALE_RECORD.into());
            }
            (id, next_revision)
        }
        _ => return Err("编辑时需提供记录和版本。".into()),
    };
    Ok(payload.public(id, revision))
}

fn import_entries(
    store: &Store,
    kind: VaultKind,
    entries: Vec<Payload>,
) -> Result<VaultImportResult, String> {
    let mut conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
    let tx = conn.transaction().map_err(|_| "无法开始导入。")?;
    let mut seen: HashSet<_> =
        read_records(&tx, &store.workspace_id, kind, |_, _, payload| payload)?
            .into_iter()
            .collect();
    let mut result = VaultImportResult::default();
    for payload in entries {
        if seen.contains(&payload) {
            result.skipped += 1;
        } else {
            insert(&tx, &store.workspace_id, &payload)?;
            seen.insert(payload);
            result.created += 1;
        }
    }
    tx.commit().map_err(|_| "无法完成导入。")?;
    Ok(result)
}

fn read_versioned(
    conn: &Connection,
    workspace: &str,
    kind: VaultKind,
    record: &VaultVersion,
) -> Result<Payload, String> {
    let encrypted: String = conn
        .prepare_cached(
            "SELECT encrypted FROM password_vault WHERE id=?1 AND kind=?2 AND revision=?3",
        )
        .map_err(|_| "无法读取密码库。")?
        .query_row(params![record.id, kind.as_str(), record.revision], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "无法读取密码库。")?
        .ok_or(STALE_RECORD)?;
    open(workspace, kind, &record.id, &encrypted)
}

fn read_copy_value(
    store: &Store,
    kind: VaultKind,
    record: &VaultVersion,
    field: VaultCopyField,
) -> Result<String, String> {
    let conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
    let payload = read_versioned(&conn, &store.workspace_id, kind, record)?;
    let value = match (field, payload.fields) {
        (VaultCopyField::Password, _) => payload.password,
        (VaultCopyField::Account1, VaultFields::Application { account1, .. }) => account1,
        (VaultCopyField::Account2, VaultFields::Application { account2, .. }) => account2,
        (VaultCopyField::Account, VaultFields::Server { account, .. }) => account,
        _ => return Err("此记录没有该字段。".into()),
    };
    if value.is_empty() {
        return Err("没有可复制的内容。".into());
    }
    Ok(value)
}

fn reveal(
    store: &Store,
    kind: VaultKind,
    records: Vec<VaultVersion>,
) -> Result<Vec<RevealedPassword>, String> {
    let conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
    records
        .into_iter()
        .map(|record| {
            let password = read_versioned(&conn, &store.workspace_id, kind, &record)?.password;
            Ok(RevealedPassword {
                id: record.id,
                password,
            })
        })
        .collect()
}

fn delete_entry(store: &Store, kind: VaultKind, id: &str, revision: i64) -> Result<(), String> {
    let conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
    let changed = conn
        .execute(
            "DELETE FROM password_vault WHERE id=?1 AND kind=?2 AND revision=?3",
            params![id, kind.as_str(), revision],
        )
        .map_err(|_| "无法删除密码库记录。")?;
    if changed != 1 {
        return Err(STALE_RECORD.into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_password_vault(
    app: AppHandle,
    workspace_id: String,
    kind: VaultKind,
) -> Result<Vec<VaultEntry>, String> {
    in_workspace(app, workspace_id, move |store| {
        let conn = store.conn.lock().map_err(|_| "无法读取密码库。")?;
        read_records(&conn, &store.workspace_id, kind, |id, revision, payload| {
            payload.public(id, revision)
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn save_password_vault(
    app: AppHandle,
    workspace_id: String,
    input: VaultDraft,
) -> Result<VaultEntry, String> {
    in_workspace(app, workspace_id, move |store| save_entry(store, input)).await
}

#[tauri::command]
pub(crate) async fn delete_password_vault(
    app: AppHandle,
    workspace_id: String,
    kind: VaultKind,
    id: String,
    revision: i64,
) -> Result<(), String> {
    in_workspace(app, workspace_id, move |store| {
        delete_entry(store, kind, &id, revision)
    })
    .await
}

#[tauri::command]
pub(crate) async fn reveal_password_vault(
    app: AppHandle,
    workspace_id: String,
    kind: VaultKind,
    records: Vec<VaultVersion>,
) -> Result<Vec<RevealedPassword>, String> {
    in_workspace(app, workspace_id, move |store| reveal(store, kind, records)).await
}

#[tauri::command]
pub(crate) async fn copy_password_vault(
    app: AppHandle,
    workspace_id: String,
    kind: VaultKind,
    record: VaultVersion,
    field: VaultCopyField,
) -> Result<(), String> {
    in_workspace(app, workspace_id, move |store| {
        write_clipboard_text(read_copy_value(store, kind, &record, field)?)
    })
    .await
}

#[tauri::command]
pub(crate) async fn import_password_vault_excel(
    app: AppHandle,
    workspace_id: String,
    kind: VaultKind,
    file_name: String,
    contents: String,
) -> Result<VaultImportResult, String> {
    let parsed = read_password_vault_excel(kind, file_name, contents).await?;
    if let Some(error) = parsed.errors.first() {
        return Err(format!("Excel 第 {} 行：{}", error.row, error.message));
    }
    if parsed.rows.is_empty() {
        return Err("Excel 中没有可导入的记录。".into());
    }
    let entries = parsed
        .rows
        .into_iter()
        .map(|row| Payload::new(row.fields, row.password))
        .collect::<Result<Vec<_>, _>>()?;
    in_workspace(app, workspace_id, move |store| {
        import_entries(store, kind, entries)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn application(password: &str) -> Value {
        json!({
            "kind":"application", "appName":"Fixture portal", "appCode":"0042",
            "address":"https://example.test/app", "account1":"fixture-admin", "account2":"fixture-reader",
            "notes":"fixture note", "password":password
        })
    }

    fn draft(value: Value) -> VaultDraft {
        serde_json::from_value(value).unwrap()
    }

    fn version(entry: &VaultEntry) -> VaultVersion {
        VaultVersion {
            id: entry.id.clone(),
            revision: entry.revision,
        }
    }

    #[test]
    fn vault_records_are_encrypted_and_reveal_requires_the_current_version() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_at(directory.path().to_path_buf()).unwrap();
        let password = " sample-only-password ";
        let original = save_entry(&store, draft(application(password))).unwrap();
        let listed = serde_json::to_value(&original).unwrap();
        assert_eq!(listed["kind"], "application");
        assert_eq!(listed["account1"], "fixture-admin");
        assert_eq!(listed["account2"], "fixture-reader");
        assert_eq!(listed["hasPassword"], true);
        assert!(listed.get("password").is_none());
        assert!(!store
            .workspace_snapshot()
            .unwrap()
            .to_string()
            .contains("Fixture portal"));

        let conn = store.conn.lock().unwrap();
        let raw: String = conn
            .query_row(
                "SELECT encrypted FROM password_vault WHERE id=?1",
                [&original.id],
                |row| row.get(0),
            )
            .unwrap();
        for value in [
            password,
            "fixture-admin",
            "fixture-reader",
            "fixture note",
            "https://example.test/app",
        ] {
            assert!(!raw.contains(value));
        }
        drop(conn);

        assert!(reveal(&store, VaultKind::Server, vec![version(&original)]).is_err());
        let output = reveal(&store, VaultKind::Application, vec![version(&original)]).unwrap();
        assert_eq!(output[0].password, password);
        for (field, expected) in [
            (VaultCopyField::Account1, "fixture-admin"),
            (VaultCopyField::Account2, "fixture-reader"),
            (VaultCopyField::Password, password),
        ] {
            assert_eq!(
                read_copy_value(&store, VaultKind::Application, &version(&original), field)
                    .unwrap(),
                expected
            );
        }
        let mut update = application("replacement");
        update["id"] = json!(original.id);
        update["revision"] = json!(original.revision);
        let updated = save_entry(&store, draft(update.clone())).unwrap();
        assert!(save_entry(&store, draft(update)).is_err());
        assert!(reveal(&store, VaultKind::Application, vec![version(&original)]).is_err());
        assert_eq!(
            read_copy_value(
                &store,
                VaultKind::Application,
                &version(&original),
                VaultCopyField::Password
            )
            .unwrap_err(),
            STALE_RECORD
        );
        assert!(delete_entry(
            &store,
            VaultKind::Application,
            &original.id,
            original.revision
        )
        .is_err());
        delete_entry(
            &store,
            VaultKind::Application,
            &updated.id,
            updated.revision,
        )
        .unwrap();
        assert!(reveal(&store, VaultKind::Application, vec![version(&updated)]).is_err());
    }

    #[test]
    fn vault_import_preserves_distinct_credentials_and_skips_exact_retries() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_at(directory.path().to_path_buf()).unwrap();
        let server = |password: &str| {
            Payload::new(
                VaultFields::Server {
                    ip: "2001:0db8:0:0::1".into(),
                    account: "fixture-account".into(),
                    environment: VaultEnvironment::Test,
                    notes: "".into(),
                },
                password.into(),
            )
            .unwrap()
        };
        let kind = VaultKind::Server;
        let saved = import_entries(
            &store,
            kind,
            vec![server("one"), server("one"), server("two")],
        )
        .unwrap();
        assert_eq!((saved.created, saved.skipped), (2, 1));
        let retried = import_entries(&store, kind, vec![server("two"), server("one")]).unwrap();
        assert_eq!((retried.created, retried.skipped), (0, 2));

        let conn = store.conn.lock().unwrap();
        let records = read_records(&conn, &store.workspace_id, kind, |id, revision, payload| {
            payload.public(id, revision)
        })
        .unwrap();
        drop(conn);
        assert_eq!(
            read_copy_value(&store, kind, &version(&records[0]), VaultCopyField::Account).unwrap(),
            "fixture-account"
        );
        let records = serde_json::to_value(records).unwrap();
        assert_eq!(records[0]["ip"], "2001:db8::1");
        assert_eq!(records[0]["environment"], "test");
        assert!(records[0].get("password").is_none());
        assert!(serde_json::from_value::<VaultDraft>(json!({
            "kind":"server", "ip":"192.0.2.1", "account":"fixture",
            "environment":"stage", "notes":"", "password":""
        }))
        .is_err());
    }
}
