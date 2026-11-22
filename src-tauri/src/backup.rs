use crate::{
    backup_cron::CronSchedule,
    credential_vault,
    s3_backup::{S3Client, S3Credentials, S3Object, S3Target, UploadError, UploadProgress},
    storage::Store,
    AppState,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex, MutexGuard};
const KEY: &str = "s3-backup-v1";
const RETAINED_BACKUPS: usize = 10;
fn operation() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
fn acquire() -> Result<MutexGuard<'static, ()>, String> {
    operation()
        .try_lock()
        .map_err(|_| "备份或还原正在执行，请稍后重试".into())
}
pub fn maintenance_guard() -> Result<MutexGuard<'static, ()>, String> {
    acquire()
}
struct NotifyingGuard {
    app: AppHandle,
    guard: Option<MutexGuard<'static, ()>>,
}
impl Drop for NotifyingGuard {
    fn drop(&mut self) {
        drop(self.guard.take());
        let _ = self.app.emit("backup-updated", ());
    }
}
fn notifying(app: &AppHandle) -> Result<NotifyingGuard, String> {
    Ok(NotifyingGuard {
        app: app.clone(),
        guard: Some(acquire()?),
    })
}
#[cfg(test)]
fn ensure_idle() -> Result<(), String> {
    drop(acquire()?);
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupConfig {
    pub revision: i64,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub force_path_style: bool,
    pub has_credentials: bool,
    pub enabled: bool,
    pub cron: String,
}
impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            revision: 0,
            endpoint: String::new(),
            region: "auto".into(),
            bucket: String::new(),
            prefix: "backups/".into(),
            force_path_style: false,
            has_credentials: false,
            enabled: false,
            cron: "0 2 * * *".into(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgressView {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub key: String,
    pub filename: String,
    pub size: u64,
    pub created_at: String,
    pub trigger: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub monthly_archive: bool,
    #[serde(default)]
    pub progress: Option<BackupProgressView>,
    #[serde(default)]
    pub retry_available: bool,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupState {
    pub config: BackupConfig,
    pub records: Vec<BackupRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
    pub running: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveInput {
    #[serde(flatten)]
    pub config: BackupConfig,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}
fn read(store: &Store) -> Result<BackupState, String> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = store
        .conn
        .lock()
        .map_err(|_| "备份存储不可用")?
        .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| "读取备份配置失败")?;
    match raw {
        None => Ok(BackupState::default()),
        Some(raw) => serde_json::from_value::<BackupState>(credential_vault::decrypt_json(
            &store.workspace_id,
            "settings:s3-backup",
            &raw,
        )?)
        .map_err(|_| "备份配置格式无效".to_string())
        .and_then(|mut s| {
            s.config.has_credentials = match entry(&store.workspace_id, &s.config)?.get_password() {
                Ok(_) => true,
                Err(credential_vault::Error::NoEntry) => false,
                Err(e) => return Err(e.to_string()),
            };
            for r in &mut s.records {
                r.retry_available = retry_path(store, r).is_some();
            }
            Ok(s)
        }),
    }
}
fn recover_interrupted(r: &mut BackupRecord) {
    match r.status.as_str() {
        "preparing" | "packing" => {
            r.status = "failed".into();
            r.error = Some("上次打包已中断，请重新创建备份".into());
            r.progress = None;
        }
        "uploading" | "verifying" => {
            r.status = "unknown".into();
            r.error = Some("上次上传结果未确认，请核验远端记录".into());
            r.progress = None;
        }
        _ => {}
    }
}
// 持有备份操作锁后才能将上次进程留下的阶段恢复并落库；普通读取不改变活动状态。
fn recover_persisted(store: &Store) -> Result<BackupState, String> {
    let mut s = read(store)?;
    let mut changed = false;
    for r in &mut s.records {
        let old = r.status.clone();
        recover_interrupted(r);
        changed |= old != r.status;
        r.retry_available = retry_path(store, r).is_some();
    }
    if changed {
        write(store, &s)?;
    }
    Ok(s)
}
fn state_snapshot(store: &Store) -> Result<BackupState, String> {
    if let Ok(guard) = acquire() {
        let s = recover_persisted(store)?;
        drop(guard);
        Ok(s)
    } else {
        read(store)
    }
}
fn recover_for_action(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    recover_persisted(&store)?;
    Ok(())
}
fn action_allowed(c: &Context, r: &BackupRecord) -> bool {
    owned_backup_key(c, &r.key) && matches!(r.status.as_str(), "unknown" | "failed")
}
fn archive_path(generation: &std::path::Path, id: &str) -> Result<PathBuf, String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "备份记录ID无效")?;
    if uuid.to_string() != id {
        return Err("备份记录ID格式无效".into());
    }
    Ok(generation
        .join("staging/s3-backup")
        .join(format!("{id}.zip")))
}
fn retry_path(store: &Store, r: &BackupRecord) -> Option<PathBuf> {
    if !matches!(r.status.as_str(), "failed" | "unknown") || r.size == 0 {
        return None;
    }
    let path = archive_path(&store.generation, &r.id).ok()?;
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != r.size {
        return None;
    }
    Some(path)
}
fn write(store: &Store, s: &BackupState) -> Result<(), String> {
    let raw = credential_vault::encrypt_json(
        &store.workspace_id,
        "settings:s3-backup",
        &serde_json::to_value(s).map_err(|_| "备份配置序列化失败")?,
    )?;
    store.conn.lock().map_err(|_|"备份存储不可用")?.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,raw]).map_err(|_|"保存备份配置失败")?;
    Ok(())
}
fn credential_id(c: &BackupConfig) -> String {
    use sha2::{Digest, Sha256};
    let endpoint = url::Url::parse(c.endpoint.trim())
        .map(|u| u.to_string().trim_end_matches('/').to_owned())
        .unwrap_or_else(|_| c.endpoint.trim().trim_end_matches('/').into());
    format!(
        "backup:s3:{:x}",
        Sha256::digest(
            format!("{}\n{}\n{}", endpoint, c.region.trim(), c.bucket.trim()).as_bytes()
        )
    )
}
fn entry(workspace: &str, c: &BackupConfig) -> Result<credential_vault::Entry, String> {
    credential_vault::entry(workspace, "com.self.workbench", &credential_id(c))
        .map_err(|e| e.to_string())
}
fn target(c: &BackupConfig) -> S3Target {
    S3Target {
        endpoint: c.endpoint.clone(),
        region: c.region.clone(),
        bucket: c.bucket.clone(),
        prefix: c.prefix.clone(),
        force_path_style: c.force_path_style,
    }
}
fn secrets(workspace: &str, c: &BackupConfig) -> Result<S3Credentials, String> {
    let raw = entry(workspace, c)?
        .get_password()
        .map_err(|_| "请填写 S3 访问凭据")?;
    serde_json::from_str(&raw).map_err(|_| "S3 凭据格式无效".into())
}
fn validate(c: &BackupConfig) -> Result<(), String> {
    CronSchedule::parse(&c.cron)?.next_after(Utc::now())?;
    Ok(())
}
fn identity(a: &BackupConfig, b: &BackupConfig) -> bool {
    a.endpoint == b.endpoint && a.region == b.region && a.bucket == b.bucket
}

fn validate_save(
    old: &BackupConfig,
    new: &BackupConfig,
    access: Option<&str>,
    secret: Option<&str>,
) -> Result<(), String> {
    if old.revision != new.revision {
        return Err("备份配置已更新，请刷新后重试".into());
    }
    let access = access.unwrap_or("").trim();
    let secret = secret.unwrap_or("");
    let provided = !access.is_empty() || !secret.is_empty();
    if provided && (access.is_empty() || secret.is_empty()) {
        return Err("Access Key ID 和 Secret Access Key 必须同时填写".into());
    }
    if !identity(old, new) && !provided {
        return Err("更换端点、区域或存储桶后，请重新填写访问凭据".into());
    }
    Ok(())
}
fn owned_backup_key(c: &Context, key: &str) -> bool {
    owned_backup_name(c, key).is_some()
}
fn owned_backup_name<'a>(c: &Context, key: &'a str) -> Option<&'a str> {
    let prefix = owned_prefix(c);
    let name = key.strip_prefix(&prefix)?.strip_prefix("perch-")?;
    let name = name.strip_prefix("monthly-").unwrap_or(name);
    let name = name.strip_suffix(".zip")?;
    if name.len() != 53 || name.as_bytes().get(16) != Some(&b'-') {
        return None;
    }
    (chrono::NaiveDateTime::parse_from_str(&name[..16], "%Y%m%dT%H%M%SZ").is_ok()
        && uuid::Uuid::parse_str(&name[17..]).is_ok())
    .then_some(name)
}
fn retention_candidates(c: &Context, records: &[BackupRecord]) -> Vec<String> {
    let mut eligible: Vec<_> = records
        .iter()
        .filter(|r| r.status == "success")
        .filter_map(|r| owned_backup_name(c, &r.key).map(|name| (name, &r.key)))
        .collect();
    // 从对象名取得原始创建时间，让旧归档和普通备份使用相同的 UTC 顺序。
    eligible.sort_unstable_by(|a, b| b.cmp(a));
    eligible.dedup();
    eligible
        .into_iter()
        .skip(RETAINED_BACKUPS)
        .map(|(_, key)| key.clone())
        .collect()
}
#[cfg(test)]
fn upload_outcome(record: &mut BackupRecord, success: bool) {
    record.status = if success { "success" } else { "unknown" }.into();
    record.error = if success {
        None
    } else {
        Some("上传结果未确认，请刷新记录核验；不会自动重传".into())
    };
}
fn output(mut s: BackupState) -> BackupState {
    s.running = operation().try_lock().is_err();
    s.next_run_at = if s.config.enabled {
        CronSchedule::parse(&s.config.cron)
            .and_then(|c| c.next_after(Utc::now()))
            .ok()
            .map(|d| d.to_rfc3339())
    } else {
        None
    };
    s
}
#[tauri::command]
pub async fn backup_state(app: AppHandle) -> Result<BackupState, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        Ok(output(state_snapshot(&store)?))
    })
    .await
    .map_err(|_| "读取备份状态任务失败")?
}
#[tauri::command]
pub fn backup_save(app: AppHandle, value: SaveInput) -> Result<BackupState, String> {
    let _op = notifying(&app)?;
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    let mut s = read(&store)?;
    validate_save(
        &s.config,
        &value.config,
        value.access_key_id.as_deref(),
        value.secret_access_key.as_deref(),
    )?;
    validate(&value.config)?;
    let access = value.access_key_id.unwrap_or_default();
    let secret = value.secret_access_key.unwrap_or_default();
    let provided = !access.trim().is_empty() || !secret.is_empty();
    let cred = if provided {
        S3Credentials {
            access_key_id: access.trim().into(),
            secret_access_key: secret,
        }
    } else {
        secrets(&store.workspace_id, &value.config)?
    };
    S3Client::new(target(&value.config), cred.clone())?;
    let vault = entry(&store.workspace_id, &value.config)?;
    let previous = match vault.get_password() {
        Ok(value) => Some(value),
        Err(credential_vault::Error::NoEntry) => None,
        Err(error) => return Err(error.to_string()),
    };
    if provided {
        vault
            .set_password(&serde_json::to_string(&cred).map_err(|_| "凭据序列化失败")?)
            .map_err(|e| e.to_string())?
    }
    if !identity(&s.config, &value.config)
        || s.config.prefix.trim_matches('/') != value.config.prefix.trim_matches('/')
    {
        s.records.clear();
    }
    s.config = value.config;
    s.config.revision += 1;
    s.config.has_credentials = true;
    s.next_run_at = None;
    s.running = false;
    if let Err(error) = write(&store, &s) {
        if provided {
            match previous {
                Some(p) => {
                    let _ = vault.set_password(&p);
                }
                None => {
                    let _ = vault.delete_credential();
                }
            }
        }
        return Err(error);
    }
    drop(store);
    let _ = app.emit("backup-updated", ());
    drop(_op);
    Ok(output(s))
}
#[derive(Clone)]
struct Context {
    workspace: String,
    generation: PathBuf,
    root: PathBuf,
    revision: i64,
    config: BackupConfig,
}
fn capture(app: &AppHandle) -> Result<(Context, S3Client), String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    let s = read(&store)?;
    let client = S3Client::new(target(&s.config), secrets(&store.workspace_id, &s.config)?)?;
    Ok((
        Context {
            workspace: store.workspace_id.clone(),
            generation: store.generation.clone(),
            root: store.root.clone(),
            revision: s.config.revision,
            config: s.config,
        },
        client,
    ))
}
fn check(store: &Store, c: &Context) -> Result<(), String> {
    if store.workspace_id != c.workspace
        || store.generation != c.generation
        || read(store)?.config.revision != c.revision
    {
        Err("工作空间或备份配置已变化，请重新执行".into())
    } else {
        Ok(())
    }
}
fn update(
    app: &AppHandle,
    c: &Context,
    f: impl FnOnce(&mut BackupState),
) -> Result<BackupState, String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    check(&store, c)?;
    let mut s = read(&store)?;
    f(&mut s);
    for r in &mut s.records {
        r.retry_available = retry_path(&store, r).is_some();
    }
    write(&store, &s)?;
    let _ = app.emit("backup-updated", ());
    Ok(s)
}
#[tauri::command]
pub async fn backup_test(app: AppHandle, value: SaveInput) -> Result<BackupState, String> {
    let _op = notifying(&app)?;
    validate(&value.config)?;
    let state = app.state::<AppState>();
    let (workspace, old) = {
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        (store.workspace_id.clone(), read(&store)?)
    };
    let access = value.access_key_id.unwrap_or_default();
    let secret = value.secret_access_key.unwrap_or_default();
    let creds = if access.trim().is_empty() && secret.is_empty() {
        if !identity(&old.config, &value.config) {
            return Err("目的地已变化，请填写访问凭据".into());
        }
        secrets(&workspace, &value.config)?
    } else {
        if access.trim().is_empty() || secret.is_empty() {
            return Err("请同时填写两项访问凭据".into());
        }
        S3Credentials {
            access_key_id: access,
            secret_access_key: secret,
        }
    };
    S3Client::new(target(&value.config), creds)?.test().await?;
    drop(_op);
    Ok(output(old))
}
fn owned_prefix(c: &Context) -> String {
    format!(
        "{}{}/",
        if c.config.prefix.trim_matches('/').is_empty() {
            String::new()
        } else {
            c.config.prefix.trim_matches('/').to_owned() + "/"
        },
        c.workspace
    )
}

fn reconcile_records(s: &mut BackupState, c: &Context, objects: Vec<S3Object>) {
    let objects: Vec<_> = objects
        .into_iter()
        .filter(|o| owned_backup_key(c, &o.key))
        .collect();
    let previous = s.records.clone();
    s.records.retain(|r| {
        !objects.iter().any(|o| o.key == r.key) && r.status != "success" && r.trigger != "remote"
    });
    for record in &mut s.records {
        if record.status == "unknown" {
            record.status = "failed".into();
            record.error =
                Some("远端未找到备份；本地文件存在时可重新上传，否则请重新创建备份".into());
            record.progress = None;
        }
    }
    for o in objects {
        let known = previous.iter().find(|r| r.key == o.key);
        if let Some(r) = known {
            if matches!(
                r.status.as_str(),
                "preparing" | "packing" | "uploading" | "verifying"
            ) {
                s.records.push(r.clone());
                continue;
            }
            if r.size > 0 && r.size != o.size {
                let mut r = r.clone();
                r.status = "unknown".into();
                r.error =
                    Some("远端文件大小与备份记录不一致，禁止还原或覆盖；请重新创建备份".into());
                r.progress = None;
                s.records.push(r);
                continue;
            }
        }
        s.records.push(BackupRecord {
            id: known.map(|r| r.id.clone()).unwrap_or_else(|| o.key.clone()),
            filename: o.key.rsplit('/').next().unwrap_or(&o.key).into(),
            key: o.key.clone(),
            size: o.size,
            created_at: known
                .map(|r| r.created_at.clone())
                .unwrap_or(o.last_modified),
            trigger: known.map(|r| r.trigger.clone()).unwrap_or("remote".into()),
            status: "success".into(),
            error: refresh_error(known),
            monthly_archive: known.is_some_and(|r| r.monthly_archive)
                || o.key
                    .rsplit('/')
                    .next()
                    .is_some_and(|f| f.starts_with("perch-monthly-")),
            progress: None,
            retry_available: false,
        });
    }
    s.records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
}
async fn refresh_inner(
    app: &AppHandle,
    c: &Context,
    client: &S3Client,
) -> Result<BackupState, String> {
    let objects = client.list().await?;
    let s = update(app, c, |s| reconcile_records(s, c, objects))?;
    for r in s.records.iter().filter(|r| r.status == "success") {
        cleanup_archive(c, r);
    }
    Ok(s)
}
#[tauri::command]
pub async fn backup_refresh(app: AppHandle) -> Result<BackupState, String> {
    let op = notifying(&app)?;
    recover_for_action(&app)?;
    let (c, client) = capture(&app)?;
    let s = refresh_inner(&app, &c, &client).await?;
    drop(op);
    Ok(output(s))
}
fn refresh_error(known: Option<&BackupRecord>) -> Option<String> {
    known
        .filter(|r| r.status == "success")
        .and_then(|r| r.error.clone())
}
#[cfg(test)]
fn mark_postprocess_failure(s: &mut BackupState, before: &[String], trigger: &str) -> bool {
    if let Some(r) = s
        .records
        .iter_mut()
        .find(|r| !before.contains(&r.id) && r.trigger == trigger && r.status == "success")
    {
        r.error = Some("已上传，但备份记录刷新或保留清理失败，请刷新核验并检查存储权限".into());
        true
    } else {
        false
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    workspace_id: String,
    data_root: String,
    record: BackupRecord,
}
fn emit_record(app: &AppHandle, c: &Context, record: &BackupRecord) {
    let _ = app.emit(
        "backup-progress",
        ProgressEvent {
            workspace_id: c.workspace.clone(),
            data_root: c.root.to_string_lossy().into(),
            record: record.clone(),
        },
    );
}
struct ProgressReporter {
    app: AppHandle,
    context: Context,
    inner: std::sync::Mutex<(BackupRecord, Option<Instant>)>,
}
impl ProgressReporter {
    fn new(app: &AppHandle, c: &Context, record: BackupRecord) -> Arc<Self> {
        Arc::new(Self {
            app: app.clone(),
            context: c.clone(),
            inner: std::sync::Mutex::new((record, None)),
        })
    }
    fn report(&self, phase: &str, completed: u64, total: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let phase_changed = inner.0.progress.as_ref().is_none_or(|p| p.phase != phase);
        inner.0.status = match phase {
            "database" | "packing" => "packing",
            "verifying" => "verifying",
            _ => "uploading",
        }
        .into();
        inner.0.progress = Some(BackupProgressView {
            phase: phase.into(),
            completed,
            total: if total > 0 { Some(total) } else { None },
        });
        if phase_changed
            || inner
                .1
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(100))
            || total > 0 && completed == total
        {
            emit_record(&self.app, &self.context, &inner.0);
            inner.1 = Some(Instant::now());
        }
    }
}
fn persist_record(
    app: &AppHandle,
    c: &Context,
    id: &str,
    f: impl FnOnce(&mut BackupRecord),
) -> Result<BackupState, String> {
    let s = update(app, c, |s| {
        if let Some(r) = s.records.iter_mut().find(|r| r.id == id) {
            f(r);
        }
    })?;
    if let Some(r) = s.records.iter().find(|r| r.id == id) {
        emit_record(app, c, r);
    }
    Ok(s)
}
fn record_of(app: &AppHandle, c: &Context, id: &str) -> Result<BackupRecord, String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    check(&store, c)?;
    read(&store)?
        .records
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| "备份记录不存在".into())
}
fn cleanup_archive(c: &Context, r: &BackupRecord) {
    if let Ok(path) = archive_path(&c.generation, &r.id) {
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            let _ = std::fs::remove_file(path);
        }
    }
}
fn create_record(
    store: &Store,
    c: &Context,
    id: &str,
    trigger: &str,
) -> Result<BackupRecord, String> {
    archive_path(&c.generation, id)?;
    let mut s = read(store)?;
    if s.records.iter().any(|r| r.id == id) {
        return Err("备份记录ID已存在，请刷新记录后重试".into());
    }
    let now = Utc::now();
    let filename = format!("perch-{}-{id}.zip", now.format("%Y%m%dT%H%M%SZ"));
    let r = BackupRecord {
        id: id.into(),
        key: format!("{}{filename}", owned_prefix(c)),
        filename,
        size: 0,
        created_at: now.to_rfc3339(),
        trigger: trigger.into(),
        status: "preparing".into(),
        error: None,
        monthly_archive: false,
        progress: Some(BackupProgressView {
            phase: "preparing".into(),
            completed: 0,
            total: None,
        }),
        retry_available: false,
    };
    s.records.insert(0, r.clone());
    write(store, &s)?;
    Ok(r)
}
fn context_only(app: &AppHandle) -> Result<Context, String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    let s = read(&store)?;
    Ok(Context {
        workspace: store.workspace_id.clone(),
        generation: store.generation.clone(),
        root: store.root.clone(),
        revision: s.config.revision,
        config: s.config,
    })
}
async fn create_checked(
    app: &AppHandle,
    trigger: &str,
    id: Option<String>,
) -> Result<BackupState, String> {
    recover_for_action(app)?;
    let c = context_only(app)?;
    let id = id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let record = {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        check(&store, &c)?;
        create_record(&store, &c, &id, trigger)?
    };
    emit_record(app, &c, &record);
    let _ = app.emit("backup-updated", ());
    let result = create_inner(app, &c, &id).await;
    if let Err(ref error) = result {
        let _ = persist_record(app, &c, &id, |r| {
            if r.status == "success" {
                r.error =
                    Some("已上传，但备份记录刷新或保留清理失败，请刷新核验并检查存储权限".into());
            } else if !matches!(r.status.as_str(), "unknown" | "failed") {
                r.status = "failed".into();
                r.error = Some(error.clone());
                r.progress = None;
            }
        });
    }
    result
}
async fn create_inner(app: &AppHandle, c: &Context, id: &str) -> Result<BackupState, String> {
    let (_, client) = capture(app)?;
    refresh_inner(app, c, &client).await?;
    let pending = record_of(app, c, id)?;
    if client.head(&pending.key).await?.is_some() {
        return Err("远端已存在同名对象，禁止覆盖；请核验记录或重新创建备份".into());
    }
    persist_record(app, c, id, |r| {
        r.status = "packing".into();
        r.progress = Some(BackupProgressView {
            phase: "database".into(),
            completed: 0,
            total: None,
        });
    })?;
    let record = record_of(app, c, id)?;
    let reporter = ProgressReporter::new(app, c, record);
    let path = archive_path(&c.generation, id)?;
    std::fs::create_dir_all(path.parent().ok_or("备份临时目录无效")?)
        .map_err(|_| "无法创建备份暂存目录")?;
    let worker_app = app.clone();
    let worker_context = c.clone();
    let worker_path = path.clone();
    let worker_reporter = reporter.clone();
    tokio::task::spawn_blocking(move || {
        let state = worker_app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        check(&store, &worker_context)?;
        store.backup_with_progress(&worker_path, |p| {
            worker_reporter.report(p.phase, p.completed, p.total)
        })
    })
    .await
    .map_err(|_| "备份打包任务异常中断")??;
    let size = std::fs::metadata(&path)
        .map_err(|_| "无法读取备份文件")?
        .len();
    persist_record(app, c, id, |r| {
        r.size = size;
        r.status = "uploading".into();
        r.progress = Some(BackupProgressView {
            phase: "hashing".into(),
            completed: 0,
            total: Some(size),
        });
    })?;
    upload_record(app, c, &client, id, &path).await?;
    after_success(app, c, &client, id).await
}
async fn after_success(
    app: &AppHandle,
    c: &Context,
    client: &S3Client,
    id: &str,
) -> Result<BackupState, String> {
    let result = async {
        let s = refresh_inner(app, c, client).await?;
        for key in retention_candidates(c, &s.records) {
            client.delete(&key).await?;
            update(app, c, |s| s.records.retain(|r| r.key != key))?;
        }
        update(app, c, |_| {})
    }
    .await;
    if result.is_err() {
        let _ = persist_record(app, c, id, |r| {
            if r.status == "success" {
                r.error =
                    Some("已上传，但备份记录刷新或保留清理失败，请刷新核验并检查存储权限".into());
            }
        });
    }
    result
}
fn set_upload_failure(r: &mut BackupRecord, error: &UploadError) {
    r.status = if error.uncertain { "unknown" } else { "failed" }.into();
    r.error = Some(error.message.clone());
    r.progress = None;
}
fn apply_head(r: &mut BackupRecord, head: Result<Option<u64>, String>) {
    r.progress = None;
    match head {
        Ok(Some(size)) if size == r.size && r.size > 0 => {
            r.status = "success".into();
            r.error = None;
            r.retry_available = false;
        }
        Ok(Some(_)) => {
            r.status = "unknown".into();
            r.error = Some("远端文件大小不匹配，禁止覆盖或还原；请重新创建备份".into());
        }
        Ok(None) => {
            r.status = "failed".into();
            r.error = Some("远端未找到备份；本地文件存在时可重新上传，否则请重新创建备份".into());
        }
        Err(error) => {
            r.status = "unknown".into();
            r.error = Some(format!(
                "远端备份核验失败，结果仍未确认：{error}。请稍后重试核验"
            ));
        }
    }
}
async fn upload_record(
    app: &AppHandle,
    c: &Context,
    client: &S3Client,
    id: &str,
    path: &std::path::Path,
) -> Result<BackupState, String> {
    let record = record_of(app, c, id)?;
    let reporter = ProgressReporter::new(app, c, record.clone());
    let callback: Arc<dyn Fn(UploadProgress) + Send + Sync> =
        Arc::new(move |p| reporter.report(p.phase, p.completed, p.total));
    if let Err(error) = client
        .upload_with_progress(&record.key, path, callback)
        .await
    {
        persist_record(app, c, id, |r| set_upload_failure(r, &error))?;
        return Err(error.message);
    }
    persist_record(app, c, id, |r| {
        r.status = "verifying".into();
        r.progress = Some(BackupProgressView {
            phase: "verifying".into(),
            completed: r.size,
            total: Some(r.size),
        });
    })?;
    let head = client.head(&record.key).await;
    let s = persist_record(app, c, id, |r| apply_head(r, head))?;
    let r = s
        .records
        .iter()
        .find(|r| r.id == id)
        .ok_or("备份记录不存在")?;
    if r.status == "success" {
        cleanup_archive(c, r);
        Ok(s)
    } else {
        Err(r.error.clone().unwrap_or("备份核验失败".into()))
    }
}
#[tauri::command]
pub async fn backup_create(app: AppHandle, id: Option<String>) -> Result<BackupState, String> {
    let op = notifying(&app)?;
    let s = create_checked(&app, "manual", id).await?;
    drop(op);
    Ok(output(s))
}
#[tauri::command]
pub async fn backup_verify(app: AppHandle, id: String) -> Result<BackupState, String> {
    let op = notifying(&app)?;
    recover_for_action(&app)?;
    let (c, client) = capture(&app)?;
    let record = record_of(&app, &c, &id)?;
    if !action_allowed(&c, &record) {
        return Err("此备份记录不需要核验或不属于当前存储目录".into());
    }
    let head = client.head(&record.key).await;
    let s = persist_record(&app, &c, &id, |r| apply_head(r, head))?;
    if let Some(r) = s
        .records
        .iter()
        .find(|r| r.id == id && r.status == "success")
    {
        cleanup_archive(&c, r);
    }
    drop(op);
    Ok(output(s))
}
#[tauri::command]
pub async fn backup_retry(app: AppHandle, id: String) -> Result<BackupState, String> {
    let op = notifying(&app)?;
    recover_for_action(&app)?;
    let (c, client) = capture(&app)?;
    let record = record_of(&app, &c, &id)?;
    if !action_allowed(&c, &record) {
        return Err("仅失败或未确认的本工作空间备份可以重新上传".into());
    }
    let head = client.head(&record.key).await;
    match head {
        Ok(Some(size)) if size == record.size && size > 0 => {
            persist_record(&app, &c, &id, |r| apply_head(r, Ok(Some(size))))?;
            cleanup_archive(&c, &record);
            let s = after_success(&app, &c, &client, &id).await?;
            drop(op);
            return Ok(output(s));
        }
        Ok(None) => {
            persist_record(&app, &c, &id, |r| apply_head(r, Ok(None)))?;
        }
        other => {
            persist_record(&app, &c, &id, |r| apply_head(r, other))?;
            return Err("远端对象无法确认或大小不匹配，禁止重新上传覆盖".into());
        }
    }
    let path = match archive_path(&c.generation, &id) {
        Ok(path) => path,
        Err(_) => {
            persist_record(&app, &c, &id, |r| {
                r.status = "failed".into();
                r.error = Some("此记录没有可重传的本地备份文件，请重新创建备份".into());
                r.retry_available = false;
            })?;
            return Err("此记录没有可重传的本地备份文件，请重新创建备份".into());
        }
    };
    let valid = std::fs::symlink_metadata(&path).is_ok_and(|m| {
        m.is_file() && !m.file_type().is_symlink() && m.len() == record.size && record.size > 0
    });
    if !valid {
        persist_record(&app, &c, &id, |r| {
            r.status = "failed".into();
            r.error = Some("本地备份文件已不存在或损坏，请重新创建备份".into());
            r.retry_available = false;
        })?;
        return Err("本地备份文件已不存在或损坏，请重新创建备份".into());
    }
    persist_record(&app, &c, &id, |r| {
        r.status = "uploading".into();
        r.error = None;
        r.progress = Some(BackupProgressView {
            phase: "hashing".into(),
            completed: 0,
            total: Some(r.size),
        });
    })?;
    upload_record(&app, &c, &client, &id, &path).await?;
    let s = after_success(&app, &c, &client, &id).await?;
    drop(op);
    Ok(output(s))
}
async fn download_record_file(
    c: &Context,
    record: &BackupRecord,
    destination: &std::path::Path,
    client: &S3Client,
    before_publish: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if record.status != "success" || record.size == 0 {
        return Err("仅已确认成功的备份记录可以下载".into());
    }
    if !owned_backup_key(c, &record.key) {
        return Err("备份记录不属于当前工作空间或上传目录".into());
    }
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err("请选择有效的本地备份保存路径".into());
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Err("下载目标已存在，请选择其他文件名".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("无法检查下载目标文件".into()),
    }
    let parent = destination.parent().ok_or("下载目标目录无效")?;
    let temporary = tempfile::Builder::new()
        .prefix(".perch-download-")
        .tempdir_in(parent)
        .map_err(|_| "无法在目标目录创建下载暂存文件")?;
    let path = temporary.path().join("backup.zip");
    client.download(&record.key, &path).await?;
    let size = std::fs::metadata(&path)
        .map_err(|_| "无法核对下载文件大小")?
        .len();
    if size != record.size {
        return Err("下载文件大小与备份记录不一致，已取消保存，请刷新核验记录".into());
    }
    before_publish()?;
    // 同目录硬链接只发布完整文件，且原子拒绝覆盖并发创建的目标。
    std::fs::hard_link(&path, destination).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "下载目标已存在，请选择其他文件名".to_string()
        } else {
            "无法发布下载文件，请检查目标目录权限与文件系统".to_string()
        }
    })?;
    Ok(())
}
#[tauri::command]
pub async fn backup_download(
    app: AppHandle,
    id: String,
    destination: String,
) -> Result<(), String> {
    let _op = notifying(&app)?;
    let (c, client) = capture(&app)?;
    let record = record_of(&app, &c, &id)?;
    let state = app.state::<AppState>();
    download_record_file(
        &c,
        &record,
        std::path::Path::new(&destination),
        &client,
        || {
            let store = state.store.lock().map_err(|_| "工作空间不可用")?;
            check(&store, &c)
        },
    )
    .await
}
#[tauri::command]
pub async fn backup_restore(app: AppHandle, id: String) -> Result<Value, String> {
    let _op = notifying(&app)?;
    let (c, client) = capture(&app)?;
    let record = {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        read(&store)?
            .records
            .into_iter()
            .find(|r| r.id == id && r.status == "success" && owned_backup_key(&c, &r.key))
            .ok_or("备份记录不存在或尚未确认成功")?
    };
    let temp = tempfile::tempdir().map_err(|_| "无法创建还原临时目录")?;
    let archive = temp.path().join("restore.zip");
    client.download(&record.key, &archive).await?;
    let parent = c.root.parent().ok_or("工作空间目录无效")?;
    let target = parent.join(format!("perch-restored-{}", uuid::Uuid::now_v7()));
    let manager = app.state::<crate::natapp::NatappManager>();
    let _tools = manager.maintenance_guard()?;
    crate::integrations::ensure_idle()?;
    crate::mail::ensure_idle()?;
    let state = app.state::<AppState>();
    let mut store = state.store.lock().map_err(|_| "工作空间不可用")?;
    check(&store, &c)?;
    let next = Store::restore_to(&archive, &target)?;
    check(&store, &c)?;
    next.activate_default_bootstrap()?;
    *store = next;
    let info = store.info();
    let _ = app.emit("backup-updated", ());
    Ok(info)
}
fn claim_scheduled(store: &Store, revision: i64, slot: DateTime<Utc>) -> Result<bool, String> {
    use rusqlite::OptionalExtension;
    let key = "s3-backup-last-scheduled-slot";
    let marker = serde_json::to_string(&format!(
        "{}:{}:{}",
        store.workspace_id,
        revision,
        slot.to_rfc3339()
    ))
    .map_err(|_| "调度状态序列化失败")?;
    let conn = store.conn.lock().map_err(|_| "备份存储不可用")?;
    let old: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| "读取备份调度状态失败")?;
    if old.as_deref() == Some(&marker) {
        return Ok(false);
    }
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key,marker]).map_err(|_|"保存备份调度状态失败")?;
    Ok(true)
}
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut next: Option<(String, PathBuf, i64, DateTime<Utc>)> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let candidate = {
                let state = app.state::<AppState>();
                let result = (|| {
                    let store = state.store.lock().ok()?;
                    let s = read(&store).ok()?;
                    if !s.config.enabled || !s.config.has_credentials {
                        return None;
                    }
                    Some((
                        store.workspace_id.clone(),
                        store.generation.clone(),
                        s.config.revision,
                        s.config.cron,
                    ))
                })();
                result
            };
            let Some((w, g, r, cron)) = candidate else {
                next = None;
                continue;
            };
            if !next
                .as_ref()
                .is_some_and(|(pw, pg, pr, _)| pw == &w && pg == &g && *pr == r)
            {
                next = CronSchedule::parse(&cron)
                    .and_then(|s| s.next_after(Utc::now()))
                    .ok()
                    .map(|t| (w.clone(), g.clone(), r, t));
            }
            if next.as_ref().is_some_and(|(_, _, _, t)| *t <= Utc::now()) {
                if let Ok(_op) = notifying(&app) {
                    let claimed = {
                        let state = app.state::<AppState>();
                        state
                            .store
                            .lock()
                            .ok()
                            .filter(|s| s.workspace_id == w && s.generation == g)
                            .and_then(|s| claim_scheduled(&s, r, next.as_ref().unwrap().3).ok())
                            .unwrap_or(false)
                    };
                    if claimed {
                        let _ = create_checked(&app, "scheduled", None).await;
                    }
                    next = CronSchedule::parse(&cron)
                        .and_then(|s| s.next_after(Utc::now()))
                        .ok()
                        .map(|t| (w, g, r, t));
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    // 这些测试共享进程级备份操作锁，串行隔离以免将其他测试误判为活动备份。
    static OPERATION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn config_roundtrip_and_secrets_stay_out_of_view() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut s = BackupState::default();
        s.config.endpoint = "https://example.test".into();
        write(&store, &s).unwrap();
        assert_eq!(read(&store).unwrap().config.endpoint, s.config.endpoint);
        let raw: String = store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!raw.contains("example.test"));
        assert!(!serde_json::to_string(&s)
            .unwrap()
            .contains("secretAccessKey"));
    }
    #[test]
    fn destination_identity_and_operation_exclusion() {
        let _test_guard = OPERATION_TEST_LOCK.lock().unwrap();
        let a = BackupConfig::default();
        let mut b = a.clone();
        b.bucket = "other".into();
        assert!(!identity(&a, &b));
        let guard = acquire().unwrap();
        assert!(ensure_idle().is_err());
        drop(guard);
        assert!(ensure_idle().is_ok());
    }
    #[test]
    fn revision_identity_and_partial_credentials_rejected() {
        let a = BackupConfig::default();
        let mut b = a.clone();
        b.revision = 1;
        assert!(validate_save(&a, &b, Some("a"), Some("b")).is_err());
        b.revision = 0;
        b.endpoint = "https://other.test".into();
        assert!(validate_save(&a, &b, None, None).is_err());
        assert!(validate_save(&a, &b, Some("a"), None).is_err());
        assert!(validate_save(&a, &b, Some("a"), Some("b")).is_ok());
        assert_ne!(credential_id(&a), credential_id(&b));
    }
    fn context(store: &Store) -> Context {
        Context {
            workspace: store.workspace_id.clone(),
            generation: store.generation.clone(),
            root: store.root.clone(),
            revision: 0,
            config: BackupConfig::default(),
        }
    }
    fn record(key: String) -> BackupRecord {
        BackupRecord {
            id: "test".into(),
            key,
            filename: "x.zip".into(),
            size: 1,
            created_at: "2020-01-01T00:00:00Z".into(),
            trigger: "manual".into(),
            status: "success".into(),
            error: None,
            monthly_archive: false,
            progress: None,
            retry_available: false,
        }
    }
    #[test]
    fn generation_revision_reject_stale() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut c = context(&store);
        assert!(check(&store, &c).is_ok());
        c.generation = PathBuf::from("stale");
        assert!(check(&store, &c).is_err());
    }
    #[test]
    fn retention_keeps_latest_ten_including_monthly_archives() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let prefix = owned_prefix(&c);
        let mut records: Vec<_> = (1..=13)
            .map(|day| {
                let monthly_archive = [2, 12].contains(&day);
                let label = if monthly_archive { "monthly-" } else { "" };
                let mut item = record(format!(
                    "{prefix}perch-{label}202001{day:02}T000000Z-00000000-0000-0000-0000-000000000001.zip"
                ));
                item.monthly_archive = monthly_archive;
                item
            })
            .collect();
        let expired: Vec<_> = records[..3].iter().rev().map(|r| r.key.clone()).collect();
        assert!(retention_candidates(&c, &records[..10]).is_empty());
        records.push(records[3].clone());
        records.push(record(format!("{prefix}perch-user-file.zip")));
        records.push(record(
            "another-workspace/perch-20210201T000000Z-00000000-0000-0000-0000-000000000001.zip".into(),
        ));
        let mut unverified = record(format!(
            "{prefix}perch-20210201T000000Z-00000000-0000-0000-0000-000000000001.zip"
        ));
        unverified.status = "unknown".into();
        records.push(unverified);
        assert_eq!(retention_candidates(&c, &records), expired);
    }
    #[test]
    fn legacy_backup_config_keeps_connection_and_schedule() {
        let parsed: BackupConfig = serde_json::from_value(serde_json::json!({
            "revision": 4, "endpoint":"https://s3.example.test", "region":"auto", "bucket":"fixtures",
            "prefix":"backup/", "forcePathStyle":false, "hasCredentials":true, "enabled":true,
            "cron":"0 2 * * *", "monthlyArchive":true, "retentionDays":30, "maxBackups":0
        }))
        .unwrap();
        assert_eq!(
            (
                parsed.revision,
                parsed.bucket.as_str(),
                parsed.prefix.as_str(),
                parsed.cron.as_str()
            ),
            (4, "fixtures", "backup/", "0 2 * * *")
        );
        assert!(parsed.enabled && parsed.has_credentials);
    }
    #[test]
    fn unknown_upload_and_records_survive_reopen_without_credentials() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut s = BackupState::default();
        let mut r = record("x".into());
        upload_outcome(&mut r, false);
        assert_eq!(r.status, "unknown");
        s.records.push(r);
        s.config.has_credentials = true;
        write(&store, &s).unwrap();
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let s = read(&store).unwrap();
        assert_eq!(s.records[0].status, "unknown");
        assert!(!s.config.has_credentials);
    }
    #[tokio::test]
    async fn local_http_archive_upload_download_and_restore_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().join("source")).unwrap();
        let mut snapshot = BackupState::default();
        snapshot.config.cron = "0 3 * * *".into();
        write(&store, &snapshot).unwrap();
        let archive = root.path().join("backup.zip");
        store.backup(&archive).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut uploaded = Vec::new();
            for expected in ["PUT", "GET"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let header_end = loop {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let header = String::from_utf8_lossy(&data[..header_end]);
                assert!(header.starts_with(expected));
                assert!(header
                    .to_lowercase()
                    .contains("authorization: aws4-hmac-sha256"));
                if expected == "PUT" {
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    while data.len() < header_end + length {
                        let mut buf = [0u8; 4096];
                        let n = socket.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        data.extend_from_slice(&buf[..n]);
                    }
                    uploaded = data[header_end..header_end + length].to_vec();
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                } else {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        uploaded.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                    socket.write_all(&uploaded).await.unwrap();
                }
            }
        });
        let client = S3Client::new(
            S3Target {
                endpoint: format!("http://{address}"),
                region: "auto".into(),
                bucket: "test".into(),
                prefix: String::new(),
                force_path_style: true,
            },
            S3Credentials {
                access_key_id: "test-key".into(),
                secret_access_key: "test-secret".into(),
            },
        )
        .unwrap();
        client.upload("perch-test.zip", &archive).await.unwrap();
        let downloaded = root.path().join("downloaded.zip");
        client
            .download("perch-test.zip", &downloaded)
            .await
            .unwrap();
        server.await.unwrap();
        let restored = Store::restore_to(&downloaded, &root.path().join("restored")).unwrap();
        assert_eq!(read(&restored).unwrap().config.cron, "0 3 * * *");
        assert_eq!(restored.workspace_id, store.workspace_id);
        assert_ne!(restored.generation, store.generation);
    }
    #[test]
    fn scheduled_slot_intent_is_durable_and_not_replayed() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let slot = Utc::now();
        assert!(claim_scheduled(&store, 1, slot).unwrap());
        assert!(!claim_scheduled(&store, 1, slot).unwrap());
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert!(!claim_scheduled(&store, 1, slot).unwrap());
        assert!(claim_scheduled(&store, 2, slot).unwrap());
    }
    #[test]
    fn post_upload_failure_preserves_success_and_survives_refresh() {
        let mut s = BackupState::default();
        let mut r = record("key".into());
        r.id = "new-upload".into();
        s.records.push(r);
        assert!(mark_postprocess_failure(&mut s, &["old".into()], "manual"));
        assert_eq!(s.records[0].status, "success");
        assert!(s.records[0]
            .error
            .as_deref()
            .unwrap()
            .starts_with("已上传，但"));
        assert_eq!(refresh_error(Some(&s.records[0])), s.records[0].error);
        s.records[0].status = "unknown".into();
        assert!(refresh_error(Some(&s.records[0])).is_none());
        assert!(!mark_postprocess_failure(&mut s, &[], "manual"));
    }
    #[test]
    fn prepare_record_is_durable_before_pack_and_stable_id_rejected_on_duplicate() {
        let _test_guard = OPERATION_TEST_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let id = uuid::Uuid::now_v7().to_string();
        let r = create_record(&store, &c, &id, "manual").unwrap();
        assert_eq!(r.status, "preparing");
        assert_eq!(r.progress.as_ref().unwrap().phase, "preparing");
        assert!(!archive_path(&c.generation, &id).unwrap().exists());
        assert!(create_record(&store, &c, &id, "manual").is_err());
        let mut s = read(&store).unwrap();
        assert_eq!(s.records[0].id, id);
        s.records[0].status = "packing".into();
        write(&store, &s).unwrap();
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let s = state_snapshot(&store).unwrap();
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.records[0].id, id);
        assert_eq!(s.records[0].status, "failed");
        assert!(!s.records[0].retry_available);
    }
    #[test]
    fn legacy_records_default_progress_and_retry_and_upload_errors_are_distinct() {
        let mut r:BackupRecord=serde_json::from_value(serde_json::json!({"id":"old","key":"x","filename":"x","size":2,"createdAt":"2020-01-01T00:00:00Z","trigger":"manual","status":"unknown","monthlyArchive":false})).unwrap();
        assert!(r.progress.is_none());
        assert!(!r.retry_available);
        set_upload_failure(
            &mut r,
            &UploadError {
                message: "权限不足".into(),
                uncertain: false,
            },
        );
        assert_eq!(r.status, "failed");
        assert_eq!(r.error.as_deref(), Some("权限不足"));
        set_upload_failure(
            &mut r,
            &UploadError {
                message: "超时".into(),
                uncertain: true,
            },
        );
        assert_eq!(r.status, "unknown");
        apply_head(&mut r, Err("network".into()));
        assert_eq!(r.status, "unknown");
        apply_head(&mut r, Ok(Some(3)));
        assert_eq!(r.status, "unknown");
        apply_head(&mut r, Ok(None));
        assert_eq!(r.status, "failed");
        apply_head(&mut r, Ok(Some(2)));
        assert_eq!(r.status, "success");
    }
    #[test]
    fn refresh_preserves_inflight_and_checks_unknown_size_and_absence() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let id = uuid::Uuid::now_v7().to_string();
        let pending = create_record(&store, &c, &id, "manual").unwrap();
        let mut s = BackupState::default();
        s.records.push(pending.clone());
        reconcile_records(&mut s, &c, vec![]);
        assert_eq!(s.records[0].status, "preparing");
        s.records[0].status = "unknown".into();
        s.records[0].size = 42;
        reconcile_records(
            &mut s,
            &c,
            vec![S3Object {
                key: pending.key.clone(),
                size: 41,
                last_modified: pending.created_at.clone(),
            }],
        );
        assert_eq!(s.records[0].status, "unknown");
        reconcile_records(&mut s, &c, vec![]);
        assert_eq!(s.records[0].status, "failed");
        reconcile_records(
            &mut s,
            &c,
            vec![S3Object {
                key: pending.key,
                size: 42,
                last_modified: pending.created_at,
            }],
        );
        assert_eq!(s.records[0].status, "success");
        assert_eq!(s.records[0].id, id);
    }
    #[test]
    fn retry_availability_requires_valid_retained_archive_and_cleanup_is_scoped() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let id = uuid::Uuid::now_v7().to_string();
        let mut r = record("x".into());
        r.id = id;
        r.status = "unknown".into();
        r.size = 7;
        let path = archive_path(&c.generation, &r.id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert!(retry_path(&store, &r).is_none());
        std::fs::write(&path, b"archive").unwrap();
        assert_eq!(retry_path(&store, &r), Some(path.clone()));
        apply_head(&mut r, Ok(None));
        assert!(path.exists());
        assert!(retry_path(&store, &r).is_some());
        apply_head(&mut r, Ok(Some(7)));
        assert_eq!(r.status, "success");
        cleanup_archive(&c, &r);
        assert!(!path.exists());
        assert!(archive_path(&c.generation, "../outside").is_err());
    }
    #[tokio::test]
    async fn simulated_head_confirms_absence_and_size_without_upload() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for status in ["404 Not Found", "200 OK"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut b = [0; 1024];
                    let n = socket.read(&mut b).await.unwrap();
                    bytes.extend_from_slice(&b[..n]);
                    if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                assert!(String::from_utf8_lossy(&bytes).starts_with("HEAD "));
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Length: 7\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let client = S3Client::new(
            S3Target {
                endpoint: format!("http://{address}"),
                region: "auto".into(),
                bucket: "test".into(),
                prefix: String::new(),
                force_path_style: true,
            },
            S3Credentials {
                access_key_id: "test".into(),
                secret_access_key: "test".into(),
            },
        )
        .unwrap();
        let mut r = record("x".into());
        r.status = "unknown".into();
        r.size = 7;
        apply_head(&mut r, client.head("x").await);
        assert_eq!(r.status, "failed");
        apply_head(&mut r, client.head("x").await);
        assert_eq!(r.status, "success");
        server.await.unwrap();
    }
    #[test]
    fn restarted_upload_recovery_is_persisted_before_locked_verify_or_retry() {
        let _test_guard = OPERATION_TEST_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let id = uuid::Uuid::now_v7().to_string();
        let mut r = create_record(&store, &c, &id, "manual").unwrap();
        r.status = "uploading".into();
        r.size = 7;
        let path = archive_path(&c.generation, &id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"archive").unwrap();
        let mut s = read(&store).unwrap();
        s.records[0] = r;
        write(&store, &s).unwrap();
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(read(&store).unwrap().records[0].status, "uploading");
        let displayed = state_snapshot(&store).unwrap();
        assert_eq!(displayed.records[0].status, "unknown");
        assert!(displayed.records[0].retry_available);
        assert_eq!(read(&store).unwrap().records[0].status, "unknown");
        let guard = acquire().unwrap();
        let s = recover_persisted(&store).unwrap();
        let mut r = s.records[0].clone();
        assert!(action_allowed(&context(&store), &r));
        apply_head(&mut r, Ok(None));
        assert_eq!(r.status, "failed");
        assert!(retry_path(&store, &r).is_some());
        assert!(action_allowed(&context(&store), &r));
        apply_head(&mut r, Ok(Some(7)));
        assert_eq!(r.status, "success");
        drop(guard);
    }
    #[test]
    fn direct_action_without_prior_state_read_recovers_but_active_snapshot_does_not() {
        let _test_guard = OPERATION_TEST_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let c = context(&store);
        let id = uuid::Uuid::now_v7().to_string();
        let mut r = create_record(&store, &c, &id, "manual").unwrap();
        r.status = "verifying".into();
        r.size = 8;
        let mut s = read(&store).unwrap();
        s.records[0] = r;
        write(&store, &s).unwrap();
        let guard = acquire().unwrap();
        assert_eq!(
            state_snapshot(&store).unwrap().records[0].status,
            "verifying"
        );
        let recovered = recover_persisted(&store).unwrap();
        assert_eq!(recovered.records[0].status, "unknown");
        assert!(action_allowed(&c, &recovered.records[0]));
        assert_eq!(read(&store).unwrap().records[0].status, "unknown");
        drop(guard);
    }
    fn downloadable(c: &Context, size: u64) -> BackupRecord {
        let mut r = record(format!(
            "{}perch-20261014T000000Z-00000000-0000-0000-0000-000000000001.zip",
            owned_prefix(c)
        ));
        r.size = size;
        r
    }
    #[tokio::test]
    async fn download_publishes_complete_bytes_and_rejects_partial_or_wrong_size() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body, header_size, record_size, expected_ok) in [
            (200, "zip bytes", 9, 9, true),
            (200, "short", 9, 9, false),
            (200, "zip bytes", 9, 10, false),
            (403, "denied", 6, 9, false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open_at(root.path().join("store")).unwrap();
            let c = context(&store);
            let r = downloadable(&c, record_size);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(String::from_utf8_lossy(&buf[..n]).starts_with("GET /bucket/backups/"));
                socket.write_all(format!("HTTP/1.1 {status} Status\r\nContent-Length: {header_size}\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
            });
            let client = S3Client::new(
                S3Target {
                    endpoint: format!("http://{addr}"),
                    region: "auto".into(),
                    bucket: "bucket".into(),
                    prefix: "backups/".into(),
                    force_path_style: true,
                },
                S3Credentials {
                    access_key_id: "test".into(),
                    secret_access_key: "test".into(),
                },
            )
            .unwrap();
            let dest = root.path().join("download.zip");
            let result = download_record_file(&c, &r, &dest, &client, || check(&store, &c)).await;
            assert_eq!(result.is_ok(), expected_ok);
            if expected_ok {
                assert_eq!(std::fs::read(&dest).unwrap(), body.as_bytes());
            } else {
                assert!(!dest.exists());
            }
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".perch-download-")));
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn download_rejects_records_and_existing_destination_without_network() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().join("store")).unwrap();
        let c = context(&store);
        let mut r = downloadable(&c, 9);
        let client = S3Client::new(
            S3Target {
                endpoint: "http://127.0.0.1:1".into(),
                region: "auto".into(),
                bucket: "bucket".into(),
                prefix: "backups/".into(),
                force_path_style: true,
            },
            S3Credentials {
                access_key_id: "test".into(),
                secret_access_key: "test".into(),
            },
        )
        .unwrap();
        let dest = root.path().join("download.zip");
        r.status = "unknown".into();
        assert!(download_record_file(&c, &r, &dest, &client, || Ok(()))
            .await
            .unwrap_err()
            .contains("成功"));
        r.status = "success".into();
        r.key = "other-workspace/a.zip".into();
        assert!(download_record_file(&c, &r, &dest, &client, || Ok(()))
            .await
            .unwrap_err()
            .contains("工作空间"));
        r = downloadable(&c, 9);
        std::fs::write(&dest, b"existing").unwrap();
        assert!(download_record_file(&c, &r, &dest, &client, || Ok(()))
            .await
            .unwrap_err()
            .contains("存在"));
        assert_eq!(std::fs::read(dest).unwrap(), b"existing");
    }
    #[tokio::test]
    async fn download_publish_rechecks_context_and_never_overwrites_racing_destination() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for race in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Store::open_at(root.path().join("store")).unwrap();
            let c = context(&store);
            let r = downloadable(&c, 3);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0; 8192];
                socket.read(&mut buf).await.unwrap();
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nzip",
                    )
                    .await
                    .unwrap();
            });
            let client = S3Client::new(
                S3Target {
                    endpoint: format!("http://{addr}"),
                    region: "auto".into(),
                    bucket: "bucket".into(),
                    prefix: "backups/".into(),
                    force_path_style: true,
                },
                S3Credentials {
                    access_key_id: "test".into(),
                    secret_access_key: "test".into(),
                },
            )
            .unwrap();
            let dest = root.path().join("download.zip");
            let result = download_record_file(&c, &r, &dest, &client, || {
                if race {
                    std::fs::write(&dest, b"original").unwrap();
                    Ok(())
                } else {
                    let mut changed = c.clone();
                    changed.revision += 1;
                    check(&store, &changed)
                }
            })
            .await;
            assert!(result.is_err());
            if race {
                assert_eq!(std::fs::read(&dest).unwrap(), b"original");
            } else {
                assert!(!dest.exists());
            }
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".perch-download-")));
            server.await.unwrap();
        }
    }
}
