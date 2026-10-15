//! NATAPP 本地管理：密文配置、受保护运行目录和独立子进程。
//! 官方 3.0.5 契约来源：natapp.cn/get.sh、get.ps1 和 article/config_ini。
//! 仅解析脚本中的版本和 KEY 字面值，绝不执行下载脚本。
//! macOS 分发原始 Mach-O，Windows 分发 ZIP；Token 仅进入 HTTPS 请求
//! 和私有临时 config.ini，不出现在进程参数或返回状态中。
use crate::{credential_vault, AppState};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{Manager, State};
const KEY: &str = "tools:natapp";
const LOG_LIMIT: usize = 256 * 1024;
const PACKAGE_LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tunnel {
    id: String,
    name: String,
    revision: u64,
    encrypted_token: String,
}
#[derive(Clone, Debug, PartialEq)]
struct TunnelMapping {
    forwarding_url: String,
    local_address: String,
}
#[derive(Debug, PartialEq)]
enum MappingUpdate {
    Ignore,
    Clear,
    Online(TunnelMapping),
}
// 只接受客户端专用 STAT 状态行；普通输出中的 URL 不作为隧道映射。
fn mapping_update(line: &[u8], token: &str) -> MappingUpdate {
    if line.len() > 8192 {
        return MappingUpdate::Ignore;
    }
    let text = strip_terminal_escapes(line);
    let text = String::from_utf8_lossy(&text);
    let text = text.trim();
    let Some(text) = stat_record_prefix(text) else {
        return MappingUpdate::Ignore;
    };
    let Some(record) = text.strip_prefix("[STAT]NATAPP") else {
        return MappingUpdate::Ignore;
    };
    if !record.starts_with(char::is_whitespace) {
        return MappingUpdate::Ignore;
    }
    let mut fields = HashMap::new();
    for item in record.split_whitespace() {
        if let Some((key, value)) = item.split_once('=') {
            if matches!(key, "status" | "proto" | "forwarding" | "local")
                && fields.insert(key, value).is_some()
            {
                return MappingUpdate::Clear;
            }
        }
    }
    if fields.get("status") != Some(&"online") {
        return MappingUpdate::Clear;
    }
    let (Some(proto), Some(forwarding), Some(local)) = (
        fields.get("proto"),
        fields.get("forwarding"),
        fields.get("local"),
    ) else {
        return MappingUpdate::Clear;
    };
    if !token.is_empty()
        && (forwarding
            .to_ascii_lowercase()
            .contains(&token.to_ascii_lowercase())
            || local
                .to_ascii_lowercase()
                .contains(&token.to_ascii_lowercase()))
    {
        return MappingUpdate::Clear;
    }
    let Ok(remote) = url::Url::parse(forwarding) else {
        return MappingUpdate::Clear;
    };
    let supported = match *proto {
        "http" | "https" => matches!(remote.scheme(), "http" | "https"),
        "tcp" => remote.scheme() == "tcp",
        "udp" => remote.scheme() == "udp",
        _ => false,
    };
    if !supported
        || !plain_endpoint(&remote)
        || matches!(remote.port(), Some(0))
        || (!token.is_empty()
            && remote.host_str().is_some_and(|host| {
                host.to_ascii_lowercase()
                    .contains(&token.to_ascii_lowercase())
            }))
    {
        return MappingUpdate::Clear;
    }
    let Ok(local_url) = url::Url::parse(&format!("tcp://{local}")) else {
        return MappingUpdate::Clear;
    };
    if !plain_endpoint(&local_url)
        || local_url.port().is_none_or(|port| port == 0)
        || (!token.is_empty()
            && local_url.host_str().is_some_and(|host| {
                host.to_ascii_lowercase()
                    .contains(&token.to_ascii_lowercase())
            }))
    {
        return MappingUpdate::Clear;
    }
    MappingUpdate::Online(TunnelMapping {
        forwarding_url: (*forwarding).into(),
        local_address: (*local).into(),
    })
}
// 官方输出可能带 [YYYY-MM-DD HH:MM:SS]；只接受有效固定日期前缀，不搜索任意子串。
fn stat_record_prefix(text: &str) -> Option<&str> {
    if text.starts_with("[STAT]NATAPP") {
        return Some(text);
    }
    let prefix = text.get(..21)?;
    if !prefix.starts_with('[') || !prefix.ends_with(']') {
        return None;
    }
    let timestamp = &prefix[1..20];
    let bytes = timestamp.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let expected = match index {
            4 | 7 => Some(b'-'),
            10 => Some(b' '),
            13 | 16 => Some(b':'),
            _ => None,
        };
        if expected.map_or(!byte.is_ascii_digit(), |expected| *byte != expected) {
            return None;
        }
    }
    chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S").ok()?;
    Some(text[21..].trim_start())
}
fn plain_endpoint(url: &url::Url) -> bool {
    url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/")
}
fn strip_terminal_escapes(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x1b {
            match input.get(i + 1) {
                Some(b'[') => {
                    i += 2;
                    while i < input.len() {
                        let end = (0x40..=0x7e).contains(&input[i]);
                        i += 1;
                        if end {
                            break;
                        }
                    }
                }
                Some(b']') => {
                    i += 2;
                    while i < input.len() {
                        if input[i] == 7 {
                            i += 1;
                            break;
                        }
                        if input[i] == 0x1b && input.get(i + 1) == Some(&b'\\') {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                Some(_) => i += 2,
                None => i += 1,
            }
        } else {
            out.push(input[i]);
            i += 1
        }
    }
    out
}
fn update_mapping(mapping: &Arc<Mutex<Option<TunnelMapping>>>, line: &[u8], token: &str) {
    let update = mapping_update(line, token);
    if let Ok(mut state) = mapping.lock() {
        match update {
            MappingUpdate::Ignore => {}
            MappingUpdate::Clear => *state = None,
            MappingUpdate::Online(value) => *state = Some(value),
        }
    }
}
fn clear_mapping(runtime: &Runtime) {
    if let Ok(mut mapping) = runtime.mapping.lock() {
        *mapping = None;
    }
}
#[derive(Default)]
struct LogLines {
    pending: Vec<u8>,
    discard: bool,
}
impl LogLines {
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        for byte in chunk {
            if self.discard {
                if *byte == b'\n' {
                    self.discard = false;
                }
                continue;
            }
            self.pending.push(*byte);
            if *byte == b'\n' {
                lines.push(std::mem::take(&mut self.pending));
            } else if self.pending.len() > 8192 {
                self.pending.clear();
                self.discard = true;
                lines.push("[过长日志行已省略]\n".as_bytes().to_vec());
            }
        }
        lines
    }
    fn finish(self) -> Option<Vec<u8>> {
        if self.discard || self.pending.is_empty() {
            None
        } else {
            Some(self.pending)
        }
    }
}
struct Runtime {
    child: Child,
    status: String,
    last_error: Option<String>,
    started_at: String,
    config: PathBuf,
    log: PathBuf,
    token: String,
    generation: PathBuf,
    mapping: Arc<Mutex<Option<TunnelMapping>>>,
}
#[derive(Default)]
struct Inner {
    processes: HashMap<String, Runtime>,
    installing: bool,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}
#[derive(Clone, Default)]
pub struct NatappManager {
    inner: Arc<Mutex<Inner>>,
    operation: Arc<Mutex<()>>,
}
impl NatappManager {
    pub fn maintenance_guard(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        let guard = self.operation.lock().map_err(|e| e.to_string())?;
        self.ensure_idle()?;
        Ok(guard)
    }
    pub fn ensure_idle(&self) -> Result<(), String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        if inner.installing || inner.processes.values().any(|r| r.status == "running") {
            Err("请先停止全部 NATAPP 隧道并等待安装完成".into())
        } else {
            Ok(())
        }
    }
    pub fn cleanup_stale_configs(&self, dir: &Path) -> Result<(), String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        let dir = dir.join("tunnels");
        if dir.exists() {
            for ancestor in dir.ancestors() {
                reject_link(ancestor)?;
            }
            for entry in walkdir::WalkDir::new(&dir).max_depth(4).follow_links(false) {
                let entry = entry.map_err(|_| "无法清理临时配置")?;
                reject_link(entry.path())?;
                if entry.file_name() == "config.ini"
                    && !inner
                        .processes
                        .values()
                        .any(|r| r.status == "running" && r.config == entry.path())
                {
                    fs::remove_file(entry.path()).map_err(|_| "无法清理上次运行的临时配置")?;
                }
            }
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        if let Ok(_operation) = self.operation.lock() {
            if let Ok(mut inner) = self.inner.lock() {
                for runtime in inner.processes.values_mut() {
                    let _ = runtime.child.kill();
                    let _ = runtime.child.wait();
                    let _ = fs::remove_file(&runtime.config);
                    runtime.status = "stopped".into();
                    runtime.token.clear();
                    clear_mapping(runtime);
                }
            }
        }
    }
}
fn root(store: &crate::storage::Store) -> PathBuf {
    store.root.join("tools/natapp")
}
fn executable() -> &'static str {
    if cfg!(windows) {
        "natapp.exe"
    } else {
        "natapp"
    }
}
fn platform(os: &str, arch: &str) -> Result<&'static str, String> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("macOS ARM64"),
        ("macos", "x86_64") => Ok("macOS 64位"),
        ("windows", "x86") => Ok("Windows 32位"),
        ("windows", "x86_64") => Ok("Windows 64位"),
        ("windows", "aarch64") => Err("官方未提供 Windows ARM64 客户端，当前架构暂不支持".into()),
        _ => Err("NATAPP 工具目前支持 macOS 和 Windows".into()),
    }
}
fn read(store: &crate::storage::Store) -> Result<Vec<Tunnel>, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let items: Vec<Tunnel> = value
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or(Ok(vec![]))?;
    let mut ids = std::collections::HashSet::new();
    for item in &items {
        uuid::Uuid::parse_str(&item.id).map_err(|_| "隧道配置ID无效")?;
        if !ids.insert(&item.id) {
            return Err("隧道配置ID重复".into());
        }
    }
    Ok(items)
}
fn write(store: &crate::storage::Store, items: &[Tunnel]) -> Result<(), String> {
    store.conn.lock().map_err(|e|e.to_string())?.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,serde_json::to_string(items).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(())
}
struct TemporaryConfig(PathBuf);
impl Drop for TemporaryConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn reject_link(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("工具数据不能使用符号链接".into());
    }
    Ok(())
}
fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_link(path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}
fn safe_dir(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        reject_link(ancestor)?;
    }

    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("工具目录不能是符号链接".into());
    }
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("whoami.exe")
            .args(["/user", "/fo", "csv", "/nh"])
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| "无法读取 Windows 当前用户身份")?;
        if !output.status.success() {
            return Err("无法读取 Windows 当前用户身份".into());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let sid = text
            .trim()
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .trim_matches('"');
        if !sid.starts_with("S-1-")
            || !sid
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
        {
            return Err("Windows 用户身份无效".into());
        }
        let grant = format!("*{sid}:(OI)(CI)F");
        let status = Command::new("icacls.exe")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &grant])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .status()
            .map_err(|_| "无法保护 NATAPP 目录权限")?;
        if !status.success() {
            return Err("无法保护 NATAPP 目录权限".into());
        }
    }
    Ok(())
}
fn verified(root: &Path) -> Result<bool, String> {
    for ancestor in root.ancestors() {
        reject_link(ancestor)?;
    }
    let bin = root.join(executable());
    if !bin.exists() {
        return Ok(false);
    }
    if fs::symlink_metadata(&bin)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("NATAPP 客户端不能是符号链接".into());
    }
    reject_link(&root.join("client.sha256"))?;
    let expected = fs::read_to_string(root.join("client.sha256"))
        .map_err(|_| "NATAPP 安装校验文件缺失，请重新导入官方安装包")?;
    if fs::metadata(&bin).map_err(|e| e.to_string())?.len() > PACKAGE_LIMIT {
        return Err("NATAPP 客户端大小异常".into());
    }
    let bytes = fs::read(bin).map_err(|e| e.to_string())?;
    if hex::encode(Sha256::digest(bytes)) != expected.trim() {
        return Err("NATAPP 客户端已变化，请重新导入官方安装包".into());
    }
    Ok(true)
}
fn read_version(dir: &Path) -> String {
    let path = dir.join("client.version");
    if path.ancestors().any(|a| reject_link(a).is_err()) {
        return String::new();
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() <= 32) {
        if let Ok(value) = fs::read_to_string(path) {
            if value == "3.0.5" {
                return value;
            }
        }
    }
    String::new()
}
fn snapshot(store: &crate::storage::Store, manager: &NatappManager) -> Result<Value, String> {
    let items = read(store)?;
    let inner = manager.inner.lock().map_err(|e| e.to_string())?;
    let dir = root(store);
    let installed = verified(&dir).unwrap_or(false);
    Ok(
        json!({"installed":installed,"installing":inner.installing,"platform":platform(std::env::consts::OS,std::env::consts::ARCH).map(str::to_owned).unwrap_or_else(|_|format!("{} {}",std::env::consts::OS,std::env::consts::ARCH)),"version":read_version(&dir),"dataDir":dir,"downloadedBytes":inner.downloaded_bytes,"totalBytes":inner.total_bytes,"tunnels":items.iter().map(|t|{let r=inner.processes.get(&t.id).filter(|r|r.generation==store.generation);let mapping=r.filter(|r|r.status=="running").and_then(|r|r.mapping.lock().ok().and_then(|m|m.clone()));json!({"id":t.id,"name":t.name,"revision":t.revision,"hasAuthToken":!t.encrypted_token.is_empty(),"status":r.map(|r|r.status.as_str()).unwrap_or("stopped"),"pid":r.filter(|r|r.status=="running").map(|r|r.child.id()),"lastError":r.and_then(|r|r.last_error.as_ref()),"startedAt":r.map(|r|&r.started_at),"forwardingUrl":mapping.as_ref().map(|m|&m.forwarding_url),"localAddress":mapping.as_ref().map(|m|&m.local_address)})}).collect::<Vec<_>>()}),
    )
}
#[tauri::command]
pub fn natapp_state(
    state: State<AppState>,
    manager: State<NatappManager>,
) -> Result<Value, String> {
    let cleanup = manager.operation.try_lock().ok();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if cleanup.is_some() {
        manager.cleanup_stale_configs(&root(&store))?;
    }
    snapshot(&store, &manager)
}
#[tauri::command]
pub async fn natapp_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, NatappManager>,
) -> Result<Value, String> {
    let (dir, token, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let item = read(&store)?
            .into_iter()
            .next()
            .ok_or("请先添加隧道并填写 authToken")?;
        (
            root(&store),
            credential_vault::decrypt(
                &store.workspace_id,
                &format!("natapp:{}", item.id),
                &item.encrypted_token,
            )?,
            store.generation.clone(),
        )
    };
    let worker = manager.inner().clone();
    let expected_generation = generation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _op = worker.maintenance_guard()?;
        {
            let current = app.state::<AppState>();
            let store = current.store.lock().map_err(|e| e.to_string())?;
            if store.generation != expected_generation {
                return Err("工作空间已变化，请重试安装".into());
            }
        }
        let _installing = InstallationFlag::begin(worker.inner.clone())?;
        tauri::async_runtime::block_on(install_client(&dir, &token, &worker.inner))
    })
    .await
    .map_err(|_| "安装任务失败")??;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.generation != generation {
        return Err("工作空间已变化，请刷新工具状态".into());
    }
    snapshot(&store, &manager)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveInput {
    id: Option<String>,
    name: String,
    revision: Option<u64>,
    auth_token: Option<String>,
}
#[tauri::command]
pub fn natapp_save(
    state: State<AppState>,
    manager: State<NatappManager>,
    value: SaveInput,
) -> Result<Value, String> {
    let _op = manager.operation.lock().map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut items = read(&store)?;
    let name = value.name.trim();
    if name.is_empty() || name.len() > 120 || name.chars().any(char::is_control) {
        return Err("请输入 1–120 字符的隧道名称".into());
    }
    let id = value.id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    uuid::Uuid::parse_str(&id).map_err(|_| "无效隧道 ID")?;
    if manager
        .inner
        .lock()
        .map_err(|e| e.to_string())?
        .processes
        .get(&id)
        .is_some_and(|r| r.status == "running")
    {
        return Err("请先停止隧道再编辑".into());
    }
    let old = items.iter().find(|t| t.id == id);
    if old.is_some_and(|t| Some(t.revision) != value.revision) {
        return Err("隧道配置已变化，请刷新后再保存".into());
    }
    let token = value.auth_token.unwrap_or_default();
    let token = token.trim();
    let encrypted = if token.is_empty() {
        old.map(|t| t.encrypted_token.clone())
            .ok_or("请输入 authToken")?
    } else {
        validate_auth_token(token)?;
        credential_vault::encrypt(&store.workspace_id, &format!("natapp:{id}"), token)?
    };
    let revision = old.map(|t| t.revision + 1).unwrap_or(1);
    items.retain(|t| t.id != id);
    items.push(Tunnel {
        id,
        name: name.into(),
        revision,
        encrypted_token: encrypted,
    });
    write(&store, &items)?;
    snapshot(&store, &manager)
}
#[tauri::command]
pub fn natapp_delete(
    state: State<AppState>,
    manager: State<NatappManager>,
    id: String,
    revision: u64,
) -> Result<Value, String> {
    let _op = manager.operation.lock().map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut items = read(&store)?;
    let item = items.iter().find(|t| t.id == id).ok_or("隧道不存在")?;
    if item.revision != revision {
        return Err("隧道配置已变化".into());
    }
    if manager
        .inner
        .lock()
        .map_err(|e| e.to_string())?
        .processes
        .get(&id)
        .is_some_and(|r| r.status == "running")
    {
        return Err("请先停止隧道再删除".into());
    }
    let dir = root(&store).join("tunnels").join(&id);
    for ancestor in dir.ancestors() {
        reject_link(ancestor)?;
    }
    if dir.exists() {
        for entry in walkdir::WalkDir::new(&dir).follow_links(false) {
            let entry = entry.map_err(|_| "无法核验隧道目录")?;
            reject_link(entry.path())?;
        }
        fs::remove_dir_all(&dir).map_err(|_| "无法删除隧道日志目录")?;
    }
    manager
        .inner
        .lock()
        .map_err(|e| e.to_string())?
        .processes
        .remove(&id);
    items.retain(|t| t.id != id);
    write(&store, &items)?;
    snapshot(&store, &manager)
}
fn limited_log(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    for ancestor in path.ancestors() {
        reject_link(ancestor).map_err(std::io::Error::other)?;
    }
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > LOG_LIMIT as u64 {
        file.seek(SeekFrom::Start(len - LOG_LIMIT as u64))?;
    }
    let mut bytes = vec![];
    file.take(LOG_LIMIT as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}
fn append_log(path: &Path, bytes: &[u8], token: &str) {
    let text = String::from_utf8_lossy(bytes).replace(token, "[已隐藏 authToken]");
    if reject_link(path).is_err() {
        return;
    }
    let mut prior = limited_log(path).unwrap_or_default();
    prior.extend_from_slice(text.as_bytes());
    if prior.len() > LOG_LIMIT {
        prior.drain(..prior.len() - LOG_LIMIT);
    }
    let _ = safe_write(path, &prior);
}
#[tauri::command]
pub fn natapp_start(
    state: State<AppState>,
    manager: State<NatappManager>,
    id: String,
) -> Result<Value, String> {
    let _op = manager.operation.lock().map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let dir = root(&store);
    if !verified(&dir)? {
        return Err("请先安装 NATAPP 客户端".into());
    }
    let item = read(&store)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or("隧道不存在")?;
    let mut inner = manager.inner.lock().map_err(|e| e.to_string())?;
    if inner
        .processes
        .get(&id)
        .is_some_and(|r| r.status == "running")
    {
        return Err("隧道已经启动".into());
    }
    let token = credential_vault::decrypt(
        &store.workspace_id,
        &format!("natapp:{id}"),
        &item.encrypted_token,
    )?;
    let tunnel_dir = dir.join("tunnels").join(&id);
    let runs = tunnel_dir.join("runs");
    let cwd = runs.join(uuid::Uuid::now_v7().to_string());
    safe_dir(&dir)?;
    safe_dir(&dir.join("tunnels"))?;
    safe_dir(&tunnel_dir)?;
    safe_dir(&runs)?;
    safe_dir(&cwd)?;
    let config = cwd.join("config.ini");
    let mut config_cleanup = TemporaryConfig(config.clone());
    let _ = fs::remove_file(&config);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&config).map_err(|e| e.to_string())?;
    write!(
        file,
        "[default]\nauthtoken={token}\nlog=stdout\nloglevel=INFO\nhttp_proxy=\n"
    )
    .map_err(|e| e.to_string())?;
    drop(file);
    let log = cwd.join("output.log");
    safe_write(
        &tunnel_dir.join("latest-run"),
        cwd.file_name()
            .ok_or("无效运行目录")?
            .to_string_lossy()
            .as_bytes(),
    )?;
    safe_write(&log, &[])?;
    let mut command = Command::new(dir.join(executable()));
    command
        .current_dir(&cwd)
        .arg(format!("-config={}", config.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&config);
            return Err(format!("NATAPP 启动失败：{e}"));
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let log_lock = Arc::new(Mutex::new(()));
    let mapping = Arc::new(Mutex::new(None));
    for reader in [
        stdout.map(|s| Box::new(s) as Box<dyn Read + Send>),
        stderr.map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let log = log.clone();
        let token = token.clone();
        let lock = log_lock.clone();
        let mapping = mapping.clone();
        thread::spawn(move || {
            let mut reader = reader;
            let mut lines = LogLines::default();
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for line in lines.feed(&buf[..n]) {
                    if let Ok(_guard) = lock.lock() {
                        update_mapping(&mapping, &line, &token);
                        append_log(&log, &line, &token);
                    }
                }
            }
            if let Some(line) = lines.finish() {
                if let Ok(_guard) = lock.lock() {
                    update_mapping(&mapping, &line, &token);
                    append_log(&log, &line, &token);
                }
            }
        });
    }
    config_cleanup.0.clear();
    let child_pid = child.id();
    inner.processes.insert(
        id.clone(),
        Runtime {
            child,
            status: "running".into(),
            last_error: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            config,
            log,
            token,
            generation: store.generation.clone(),
            mapping,
        },
    );
    drop(inner);
    monitor_runtime(manager.inner.clone(), id, child_pid);
    snapshot(&store, &manager)
}
fn monitor_runtime(manager_clone: Arc<Mutex<Inner>>, id: String, child_pid: u32) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(200));
        let Ok(mut inner) = manager_clone.lock() else {
            break;
        };
        let Some(runtime) = inner.processes.get_mut(&id) else {
            break;
        };
        if runtime.status != "running" || runtime.child.id() != child_pid {
            break;
        }
        match runtime.child.try_wait() {
            Ok(Some(exit)) => {
                runtime.status = if exit.success() { "stopped" } else { "error" }.into();
                runtime.last_error = Some(format!("客户端已退出：{exit}"));
                let _ = fs::remove_file(&runtime.config);
                runtime.token.clear();
                clear_mapping(runtime);
                break;
            }
            Ok(None) => {}
            Err(e) => {
                runtime.last_error = Some(format!("读取客户端状态失败：{e}"));
                let _ = runtime.child.kill();
                let _ = runtime.child.wait();
                runtime.status = "error".into();
                let _ = fs::remove_file(&runtime.config);
                runtime.token.clear();
                clear_mapping(runtime);
                break;
            }
        }
    });
}
#[tauri::command]
pub fn natapp_stop(
    state: State<AppState>,
    manager: State<NatappManager>,
    id: String,
) -> Result<Value, String> {
    let _op = manager.operation.lock().map_err(|e| e.to_string())?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    {
        let mut inner = manager.inner.lock().map_err(|e| e.to_string())?;
        if let Some(r) = inner.processes.get_mut(&id) {
            if r.status == "running" {
                r.child.kill().map_err(|e| e.to_string())?;
                r.child.wait().map_err(|e| e.to_string())?;
            }
            r.status = "stopped".into();
            r.last_error = None;
            let _ = fs::remove_file(&r.config);
            r.token.clear();
            clear_mapping(r);
        }
    }
    snapshot(&store, &manager)
}
#[tauri::command]
pub fn natapp_logs(
    state: State<AppState>,
    manager: State<NatappManager>,
    id: String,
) -> Result<Value, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if !read(&store)?.iter().any(|t| t.id == id) {
        return Err("隧道不存在".into());
    }
    let inner = manager.inner.lock().map_err(|e| e.to_string())?;
    let path = inner
        .processes
        .get(&id)
        .filter(|r| r.generation == store.generation)
        .map(|r| r.log.clone())
        .map(Ok)
        .unwrap_or_else(|| latest_log(&root(&store).join("tunnels").join(id)))?;
    let bytes = match limited_log(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.to_string()),
    };
    Ok(json!({"text":String::from_utf8_lossy(&bytes),"truncated":bytes.len()>=LOG_LIMIT}))
}

fn latest_log(dir: &Path) -> Result<PathBuf, String> {
    let marker = dir.join("latest-run");
    reject_link(&marker)?;
    let run = match fs::read_to_string(marker) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(dir.join("output.log")),
        Err(e) => return Err(e.to_string()),
    };
    uuid::Uuid::parse_str(&run).map_err(|_| "无效日志记录")?;
    let path = dir.join("runs").join(run).join("output.log");
    for ancestor in path.ancestors() {
        reject_link(ancestor)?;
    }
    Ok(path)
}
struct InstallationFlag(Arc<Mutex<Inner>>);
impl InstallationFlag {
    fn begin(inner: Arc<Mutex<Inner>>) -> Result<Self, String> {
        {
            let mut state = inner.lock().map_err(|_| "无法读取安装状态")?;
            if state.installing {
                return Err("NATAPP 正在安装，请稍候".into());
            }
            state.installing = true;
            state.downloaded_bytes = 0;
            state.total_bytes = None;
        }
        Ok(Self(inner))
    }
}
impl Drop for InstallationFlag {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.installing = false;
        }
    }
}
fn validate_auth_token(token: &str) -> Result<(), String> {
    if token.len() != 16 || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(
            "authToken 应为 16 位字母或数字，请从 NATAPP 官网的隧道列表复制（不是账户 API Token）"
                .into(),
        );
    }
    Ok(())
}
fn literal(script: &str, key: &str) -> Result<String, String> {
    let mut found = None;
    for line in script.trim_start_matches('\u{feff}').lines() {
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        let left = left
            .trim()
            .strip_prefix("export ")
            .unwrap_or(left.trim())
            .trim();
        if left.strip_prefix('$').unwrap_or(left) != key {
            continue;
        }
        if found.is_some() {
            return Err("官方安装信息包含重复字段".into());
        }
        let right = right.trim();
        let quote = right
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or("官方安装信息格式已变化")?;
        let body = &right[quote.len_utf8()..];
        let end = body.find(quote).ok_or("官方安装信息格式已变化")?;
        let value = &body[..end];
        // 不接受变量替换、转义或命令表达式，只读取版本／下载 KEY 的简单字面值。
        if value.is_empty()
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err("官方安装信息包含无效字面值".into());
        }
        let suffix = body[end + quote.len_utf8()..].trim();
        let suffix = suffix.strip_prefix(';').unwrap_or(suffix).trim();
        if !suffix.is_empty() && !suffix.starts_with('#') {
            return Err("官方安装信息格式已变化".into());
        }
        found = Some(value.to_owned());
    }
    found.ok_or_else(|| "官方安装信息格式已变化".into())
}
fn installation_metadata(script: &str) -> Result<String, String> {
    // 官方拒绝无效 Token 时仍返回 HTTP 200 和仅含 echo/exit 的错误脚本。
    // 不回显响应正文，避免响应中烘焙的 Token 进入界面或日志。
    let version = match literal(script, "NATAPP_VERSION") {
        Ok(version) => version,
        Err(error) => {
            if script.contains("authtoken 输入错误")
                || script.contains("缺少 authtoken")
                || script.contains("未在网址中填写 authtoken")
            {
                return Err("NATAPP 官网拒绝了 authToken，请从官网隧道列表复制正确的 16 位 authToken 后重试".into());
            }
            return Err(error);
        }
    };
    if version != "3.0.5" {
        return Err("官方客户端版本已变化，请等待栖点适配".into());
    }
    let key = literal(script, "KEY")?;
    if key.len() > 32 || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("官方安装信息校验失败".into());
    }
    Ok(key)
}
fn artifact(os: &str, arch: &str) -> Result<String, String> {
    platform(os, arch)?;
    let name = match (os, arch) {
        ("macos", "aarch64") => "natapp_darwin_arm64/natapp",
        ("macos", "x86_64") => "natapp_darwin_amd64/natapp",
        ("windows", "x86") => "natapp_windows_386_3_0_5.zip",
        ("windows", "x86_64") => "natapp_windows_amd64_3_0_5.zip",
        _ => return Err("当前系统不支持".into()),
    };
    Ok(format!(
        "https://download.natapp.cn/assets/downloads/clients/3_0_5/{name}"
    ))
}
async fn bounded(response: reqwest::Response, limit: u64) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!(
            "官方客户端下载失败（HTTP {}），请确认 authToken 有效及下载权限",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|n| n > limit) {
        return Err("官方客户端下载文件过大".into());
    }
    let mut response = response;
    let mut data = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| "下载连接中断")? {
        if data.len() as u64 + chunk.len() as u64 > limit {
            return Err("官方客户端下载文件过大".into());
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
fn payload(data: &[u8], windows: bool) -> Result<Vec<u8>, String> {
    if windows {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data))
            .map_err(|_| "官方安装包不是有效 ZIP")?;
        let mut output = None;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(|_| "安装包损坏")?;
            if file.name() == "natapp.exe" {
                if output.is_some()
                    || file.size() > PACKAGE_LIMIT
                    || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
                {
                    return Err("安装包客户端内容异常".into());
                }
                let mut bytes = vec![];
                file.by_ref()
                    .take(PACKAGE_LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "无法读取安装包")?;
                output = Some(bytes);
            }
        }
        let bytes = output.ok_or("安装包中没有 natapp.exe")?;
        if !bytes.starts_with(b"MZ") {
            return Err("安装包不是 Windows 客户端".into());
        }
        Ok(bytes)
    } else {
        if ![
            &[0xcf, 0xfa, 0xed, 0xfe][..],
            &[0xfe, 0xed, 0xfa, 0xcf][..],
            &[0xca, 0xfe, 0xba, 0xbe][..],
        ]
        .iter()
        .any(|magic| data.starts_with(magic))
        {
            return Err("下载内容不是 macOS 客户端".into());
        }
        Ok(data.to_vec())
    }
}
fn replace_client(source: &Path, target: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        fs::rename(source, target).map_err(|_| "无法安装客户端")?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let target: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err("无法安装客户端，请确认已有 NATAPP 已停止".into());
        }
    }
    Ok(())
}
async fn install_client(
    dir: &Path,
    token: &str,
    progress: &Arc<Mutex<Inner>>,
) -> Result<(), String> {
    validate_auth_token(token)?;
    let url = artifact(std::env::consts::OS, std::env::consts::ARCH)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建下载连接")?;
    let script_url = if cfg!(windows) {
        "https://natapp.cn/get.ps1"
    } else {
        "https://natapp.cn/get.sh"
    };
    install_from_sources(dir, token, progress, &client, script_url, &url).await
}
// 来源仅由上方固定官方契约构造；拆出下载流程供本地模拟服务验收，不对 IPC 暴露 URL。
async fn install_from_sources(
    dir: &Path,
    token: &str,
    progress: &Arc<Mutex<Inner>>,
    client: &reqwest::Client,
    script_url: &str,
    url: &str,
) -> Result<(), String> {
    let response = client
        .get(script_url)
        .query(&[("authtoken", token)])
        .send()
        .await
        .map_err(|_| "无法读取官方安装信息")?;
    let script = bounded(response, 256 * 1024).await?;
    let script = std::str::from_utf8(&script).map_err(|_| "官方安装信息编码无效")?;
    let key = installation_metadata(script)?;
    let response = client
        .get(url)
        .query(&[("key", key.as_str()), ("authtoken", token)])
        .send()
        .await
        .map_err(|_| "官方客户端下载连接失败")?;
    if !response.status().is_success() {
        return Err(format!(
            "官方客户端下载失败（HTTP {}），请检查 authToken 及下载权限",
            response.status().as_u16()
        ));
    }
    let mut response = response;
    let total = response.content_length();
    if total.is_some_and(|n| n > PACKAGE_LIMIT) {
        return Err("客户端下载文件过大".into());
    }
    {
        let mut inner = progress.lock().map_err(|e| e.to_string())?;
        inner.downloaded_bytes = 0;
        inner.total_bytes = total;
    }
    let mut data = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| "客户端下载连接中断")? {
        if data.len() as u64 + chunk.len() as u64 > PACKAGE_LIMIT {
            return Err("客户端下载文件过大".into());
        }
        data.extend_from_slice(&chunk);
        progress.lock().map_err(|e| e.to_string())?.downloaded_bytes = data.len() as u64;
    }
    let bin = payload(&data, cfg!(windows))?;
    safe_dir(dir)?;
    let temp = dir.join(format!("client-{}.tmp", uuid::Uuid::now_v7()));
    safe_write(&temp, &bin).map_err(|_| "无法保存客户端")?;
    let _temporary = TemporaryConfig(temp.clone());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法设置客户端权限")?;
    }
    safe_write(
        &dir.join(if cfg!(windows) {
            "official-package.zip"
        } else {
            "official-package.bin"
        }),
        &data,
    )
    .map_err(|_| "无法保存官方安装包")?;
    reject_link(&dir.join(executable()))?;
    replace_client(&temp, &dir.join(executable()))?;
    safe_write(
        &dir.join("client.sha256"),
        hex::encode(Sha256::digest(bin)).as_bytes(),
    )
    .map_err(|_| "无法保存安装校验")?;
    safe_write(&dir.join("client.version"), b"3.0.5")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_selection() {
        assert!(artifact("macos", "aarch64")
            .unwrap()
            .contains("darwin_arm64"));
        assert!(artifact("windows", "x86_64")
            .unwrap()
            .ends_with("amd64_3_0_5.zip"));
        assert!(artifact("windows", "aarch64").is_err());
    }
    #[test]
    fn invalid_packages_rejected() {
        assert!(payload(b"html error", false).is_err());
        assert!(payload(b"bad zip", true).is_err());
    }
    #[test]
    fn package_paths_and_binary_headers() {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("../natapp.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"MZfixture").unwrap();
        let data = archive.finish().unwrap().into_inner();
        assert!(payload(&data, true).is_err());
        assert!(payload(&[0xcf, 0xfa, 0xed, 0xfe, 0], false).is_ok());
        assert!(payload(b"MZwindows", false).is_err());
    }
    #[tokio::test]
    async fn http_errors_never_expose_request_secrets() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let response = reqwest::Client::new()
            .get(format!("http://{addr}/?authtoken=fixture-secret"))
            .send()
            .await
            .unwrap();
        let error = bounded(response, 10).await.unwrap_err();
        assert!(error.contains("403"));
        assert!(!error.contains("fixture-secret"));
        assert!(!error.contains("http"));
        server.await.unwrap();
    }
    #[test]
    fn strict_script_literals() {
        assert_eq!(literal("KEY=\"123\"", "KEY").unwrap(), "123");
        assert!(literal("KEY=\"1\"\nKEY=\"2\"", "KEY").is_err());
        assert!(literal("KEY=$(command)", "KEY").is_err());
    }
    #[test]
    fn current_official_shell_and_powershell_metadata() {
        // 2026-10-13 官方占位请求的真实赋值行；不存下载脚本烘焙的任何凭据。
        const SHELL:&str="#!/bin/sh\nNATAPP_VERSION=\"3.0.5\"\nNATAPP_SCRIPT_BASE=\"https://natapp.cn\"\nKEY=\"6561\"\n";
        const POWERSHELL:&str="# natapp Windows\r\n$NATAPP_VERSION = \"3.0.5\"\r\n$NATAPP_SCRIPT_BASE = \"https://natapp.cn\"\r\n$KEY            = \"6561\"\r\n";
        assert_eq!(installation_metadata(SHELL).unwrap(), "6561");
        assert_eq!(
            installation_metadata(&format!(
                "{SHELL}if [ -z \"$AUTHTOKEN\" ]; then\n echo '缺少 authtoken'\nfi\n"
            ))
            .unwrap(),
            "6561"
        );
        assert_eq!(installation_metadata(POWERSHELL).unwrap(), "6561");
        assert_eq!(
            installation_metadata(
                "\u{feff}export NATAPP_VERSION = '3.0.5' # version\n$KEY = '6561'; # download"
            )
            .unwrap(),
            "6561"
        );
    }
    #[test]
    fn official_http_200_token_error_is_actionable() {
        let response =
            "#!/bin/sh\necho '[natapp] 错误：authtoken 输入错误（请在官网隧道列表复制）'\nexit 1\n";
        let error = installation_metadata(response).unwrap_err();
        assert!(error.contains("官网拒绝"));
        assert!(!error.contains("格式已变化"));
        for invalid in [
            "",
            "fixture-auth-token",
            "000000000000000",
            "00000000000000000",
            "000000000000000_",
        ] {
            assert!(validate_auth_token(invalid).is_err());
        }
        assert!(validate_auth_token("0000000000000000").is_ok());
    }
    #[test]
    fn installation_failure_resets_busy_and_progress() {
        let inner = Arc::new(Mutex::new(Inner::default()));
        {
            let _flag = InstallationFlag::begin(inner.clone()).unwrap();
            assert!(inner.lock().unwrap().installing);
            assert!(installation_metadata("echo 'authtoken 输入错误'").is_err());
        }
        assert!(!inner.lock().unwrap().installing);
        {
            inner.lock().unwrap().downloaded_bytes = 999;
            let _flag = InstallationFlag::begin(inner.clone()).unwrap();
            assert_eq!(inner.lock().unwrap().downloaded_bytes, 0);
        }
        assert!(!inner.lock().unwrap().installing);
        for bad in [
            "KEY=\"$(whoami)\"",
            "KEY=\"123\"; execute",
            "KEY=\"123\"\nKEY=unquoted",
        ] {
            assert!(literal(bad, "KEY").is_err());
        }
    }
    #[cfg(not(windows))]
    #[tokio::test]
    async fn installer_downloads_valid_client_and_restores_state() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let binary = vec![0xcf, 0xfa, 0xed, 0xfe, 0, 1, 2, 3];
        let expected = binary.clone();
        let script = b"#!/bin/sh\nNATAPP_VERSION=\"3.0.5\"\nKEY=\"6561\"\n".to_vec();
        let server = tokio::spawn(async move {
            for body in [script, binary] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let size = stream.read(&mut buf).await.unwrap();
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..size]).to_string());
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let progress = Arc::new(Mutex::new(Inner::default()));
        {
            let _flag = InstallationFlag::begin(progress.clone()).unwrap();
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();
            install_from_sources(
                &dir,
                "0000000000000000",
                &progress,
                &client,
                &format!("http://{addr}/get.sh"),
                &format!("http://{addr}/natapp"),
            )
            .await
            .unwrap();
            assert!(progress.lock().unwrap().installing);
        }
        server.await.unwrap();
        assert!(verified(&dir).unwrap());
        assert_eq!(fs::read(dir.join(executable())).unwrap(), expected);
        assert_eq!(read_version(&dir), "3.0.5");
        assert_eq!(progress.lock().unwrap().downloaded_bytes, 8);
        assert!(!progress.lock().unwrap().installing);
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /get.sh?authtoken=0000000000000000 "));
        assert!(requests[1].starts_with("GET /natapp?key=6561&authtoken=0000000000000000 "));
        assert!(!fs::read_to_string(dir.join("client.sha256"))
            .unwrap()
            .contains("0000000000000000"));
    }
    #[test]
    fn configs_persist_without_exposing_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(dir.path().to_path_buf()).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        let token = "fixture-auth-token";
        let encrypted =
            credential_vault::encrypt(&store.workspace_id, &format!("natapp:{id}"), token).unwrap();
        write(
            &store,
            &[Tunnel {
                id: id.clone(),
                name: "测试隧道".into(),
                revision: 1,
                encrypted_token: encrypted,
            }],
        )
        .unwrap();
        let view = snapshot(&store, &NatappManager::default()).unwrap();
        assert_eq!(view["tunnels"][0]["hasAuthToken"], true);
        assert!(!view.to_string().contains(token));
        assert!(!view.to_string().contains("encryptedToken"));
        let rows = read(&store).unwrap();
        assert_ne!(rows[0].encrypted_token, token);
        assert_eq!(
            credential_vault::decrypt(
                &store.workspace_id,
                &format!("natapp:{id}"),
                &rows[0].encrypted_token
            )
            .unwrap(),
            token
        );
        drop(store);
        let reopened = crate::storage::Store::open_at(dir.path().to_path_buf()).unwrap();
        assert_eq!(read(&reopened).unwrap()[0].id, id);
    }
    #[cfg(unix)]
    #[test]
    fn protected_paths_cleanup_and_tamper() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        symlink(outside.path(), base.as_path().join("tools")).unwrap();
        assert!(safe_dir(&base.as_path().join("tools/natapp")).is_err());
        let client = base.as_path().join("natapp");
        safe_write(&client, b"client").unwrap();
        safe_write(
            &base.as_path().join("client.sha256"),
            hex::encode(Sha256::digest(b"client")).as_bytes(),
        )
        .unwrap();
        assert!(verified(base.as_path()).unwrap());
        safe_write(&client, b"modified").unwrap();
        assert!(verified(base.as_path()).is_err());
        let config = base.as_path().join("tunnels/id/runs/run/config.ini");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        safe_write(&config, b"secret").unwrap();
        NatappManager::default()
            .cleanup_stale_configs(base.as_path())
            .unwrap();
        assert!(!config.exists());
    }
    #[cfg(unix)]
    #[test]
    fn maintenance_and_shutdown_reap_fake_client() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.ini");
        safe_write(&config, b"fixture").unwrap();
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let manager = NatappManager::default();
        manager.inner.lock().unwrap().processes.insert(
            "fake".into(),
            Runtime {
                child,
                status: "running".into(),
                last_error: None,
                started_at: "now".into(),
                config: config.clone(),
                log: dir.path().join("output.log"),
                token: "fixture".into(),
                generation: dir.path().to_path_buf(),
                mapping: Arc::new(Mutex::new(None)),
            },
        );
        assert!(manager.maintenance_guard().is_err());
        manager.shutdown();
        assert!(!config.exists());
        let inner = manager.inner.lock().unwrap();
        assert_eq!(inner.processes["fake"].status, "stopped");
        assert!(inner.processes["fake"].token.is_empty());
        drop(inner);
        assert!(manager.maintenance_guard().is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn natural_exit_reports_failure_and_cleans_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.ini");
        safe_write(&config, b"fixture-token").unwrap();
        let child = Command::new("/usr/bin/false").spawn().unwrap();
        let pid = child.id();
        let manager = NatappManager::default();
        manager.inner.lock().unwrap().processes.insert(
            "fake".into(),
            Runtime {
                child,
                status: "running".into(),
                last_error: None,
                started_at: "now".into(),
                config: config.clone(),
                log: dir.path().join("output.log"),
                token: "fixture-token".into(),
                generation: dir.path().to_path_buf(),
                mapping: Arc::new(Mutex::new(Some(expected_mapping()))),
            },
        );
        monitor_runtime(manager.inner.clone(), "fake".into(), pid);
        for _ in 0..50 {
            if manager.inner.lock().unwrap().processes["fake"].status != "running" {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let inner = manager.inner.lock().unwrap();
        let runtime = &inner.processes["fake"];
        assert_eq!(runtime.status, "error");
        assert!(runtime.last_error.as_ref().unwrap().contains("退出"));
        assert!(runtime.token.is_empty());
        assert!(runtime.mapping.lock().unwrap().is_none());
        assert!(!config.exists());
    }
    const ONLINE:&[u8]=b"[2026-10-13 13:11:27][STAT]NATAPP status=online proto=http forwarding=http://t78aa33c.natappfree.cc local=127.0.0.1:5666";
    fn expected_mapping() -> TunnelMapping {
        TunnelMapping {
            forwarding_url: "http://t78aa33c.natappfree.cc".into(),
            local_address: "127.0.0.1:5666".into(),
        }
    }
    #[test]
    fn mapping_accepts_real_stat_line_chunked_crlf_and_ansi() {
        let mapping = Arc::new(Mutex::new(None));
        let mut lines = LogLines::default();
        let mut decorated = b"\x1b[32m".to_vec();
        decorated.extend_from_slice(ONLINE);
        decorated.extend_from_slice(b"\x1b[0m\r\n");
        for chunk in decorated.chunks(3) {
            for line in lines.feed(chunk) {
                update_mapping(&mapping, &line, "fixture-token");
            }
        }
        assert_eq!(*mapping.lock().unwrap(), Some(expected_mapping()));
        let mut incomplete = LogLines::default();
        assert!(incomplete.feed(&ONLINE[..ONLINE.len() / 2]).is_empty());
    }
    #[test]
    fn mapping_timestamp_prefix_is_strict() {
        let plain=b"[STAT]NATAPP status=online proto=http forwarding=http://t78aa33c.natappfree.cc local=127.0.0.1:5666";
        assert_eq!(
            mapping_update(plain, ""),
            MappingUpdate::Online(expected_mapping())
        );
        for prefix in [
            "[2026-99-13 13:11:27]",
            "[2026-10-13 25:11:27]",
            "[not-a-timestamp]",
            "info [2026-10-13 13:11:27]",
            "[2026-10-13 13:11:27]noise",
        ] {
            let line = [prefix.as_bytes(), plain].concat();
            assert_eq!(mapping_update(&line, ""), MappingUpdate::Ignore);
        }
    }
    #[test]
    fn mapping_ignores_noise_and_rejects_unsafe_or_secret_fields() {
        for noise in ["visit http://t78aa33c.natappfree.cc","noise [STAT]NATAPP status=online proto=http forwarding=http://foo.example local=127.0.0.1:5666","[STAT]NATAPPX status=online"]{assert_eq!(mapping_update(noise.as_bytes(),""),MappingUpdate::Ignore);}
        for bad in ["[STAT]NATAPP status=online proto=http forwarding=javascript:alert(1) local=127.0.0.1:5666","[STAT]NATAPP status=online proto=http forwarding=http://user:password@example.com local=127.0.0.1:5666","[STAT]NATAPP status=online proto=http forwarding=http://example.com?token=secret local=127.0.0.1:5666","[STAT]NATAPP status=online proto=http forwarding=http://example.com local=127.0.0.1","[STAT]NATAPP status=online proto=http forwarding=http://example.com local=127.0.0.1:0","[STAT]NATAPP status=online proto=http forwarding=http://example.com local=127.0.0.1:5666 forwarding=http://other.example"]{assert_eq!(mapping_update(bad.as_bytes(),""),MappingUpdate::Clear);}
        let token = "0123456789abcdef";
        let raw=format!("[STAT]NATAPP status=online proto=http forwarding=http://{token}.example.com local=127.0.0.1:5666");
        assert_eq!(mapping_update(raw.as_bytes(), token), MappingUpdate::Clear);
        let encoded="[STAT]NATAPP status=online proto=http forwarding=http://%30%31%32%33%34%35%36%37%38%39abcdef.example.com local=127.0.0.1:5666";
        assert_eq!(
            mapping_update(encoded.as_bytes(), token),
            MappingUpdate::Clear
        );
    }
    #[test]
    fn reconnect_clears_mapping_and_requires_new_complete_online_record() {
        let mapping = Arc::new(Mutex::new(None));
        update_mapping(&mapping, ONLINE, "");
        assert!(mapping.lock().unwrap().is_some());
        update_mapping(
            &mapping,
            b"[2026-10-13 13:12:00][STAT]NATAPP status=reconnecting",
            "",
        );
        assert!(mapping.lock().unwrap().is_none());
        update_mapping(
            &mapping,
            b"[STAT]NATAPP status=online proto=http forwarding=http://new.example",
            "",
        );
        assert!(mapping.lock().unwrap().is_none());
        update_mapping(&mapping,b"[STAT]NATAPP status=online proto=tcp forwarding=tcp://tcp.example:1234 local=[::1]:5666","");
        assert_eq!(
            mapping.lock().unwrap().as_ref().unwrap().local_address,
            "[::1]:5666"
        );
        update_mapping(
            &mapping,
            b"[2026-10-13 13:12:02][STAT]NATAPP status=offline",
            "",
        );
        assert!(mapping.lock().unwrap().is_none());
    }
    #[test]
    fn oversized_chunked_lines_do_not_produce_mapping() {
        let mut lines = LogLines::default();
        let mut long = vec![b'x'; 8193];
        long.extend_from_slice(ONLINE);
        long.extend_from_slice(b"\n");
        let mapping = Arc::new(Mutex::new(None));
        for chunk in long.chunks(17) {
            for line in lines.feed(chunk) {
                update_mapping(&mapping, &line, "");
            }
        }
        assert!(mapping.lock().unwrap().is_none());
        for line in lines.feed(&[ONLINE, b"\n"].concat()) {
            update_mapping(&mapping, &line, "");
        }
        assert_eq!(*mapping.lock().unwrap(), Some(expected_mapping()));
    }
    #[cfg(unix)]
    #[test]
    fn mapping_isolated_by_run_workspace_and_runtime_status() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(dir.path().to_path_buf()).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        write(
            &store,
            &[Tunnel {
                id: id.clone(),
                name: "测试隧道".into(),
                revision: 1,
                encrypted_token: "fixture-ciphertext".into(),
            }],
        )
        .unwrap();
        let old = Arc::new(Mutex::new(None));
        let current = Arc::new(Mutex::new(None));
        update_mapping(&old, ONLINE, "");
        let manager = NatappManager::default();
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        manager.inner.lock().unwrap().processes.insert(
            id.clone(),
            Runtime {
                child,
                status: "running".into(),
                last_error: None,
                started_at: "now".into(),
                config: dir.path().join("config.ini"),
                log: dir.path().join("log"),
                token: String::new(),
                generation: store.generation.clone(),
                mapping: current.clone(),
            },
        );
        assert_eq!(
            snapshot(&store, &manager).unwrap()["tunnels"][0]["forwardingUrl"],
            Value::Null
        );
        update_mapping(&current, ONLINE, "");
        assert_eq!(
            snapshot(&store, &manager).unwrap()["tunnels"][0]["forwardingUrl"],
            "http://t78aa33c.natappfree.cc"
        );
        update_mapping(&old,b"[STAT]NATAPP status=online proto=http forwarding=http://old.example local=127.0.0.1:1","");
        assert_eq!(
            snapshot(&store, &manager).unwrap()["tunnels"][0]["forwardingUrl"],
            "http://t78aa33c.natappfree.cc"
        );
        manager
            .inner
            .lock()
            .unwrap()
            .processes
            .get_mut(&id)
            .unwrap()
            .generation = dir.path().join("old-generation");
        assert_eq!(
            snapshot(&store, &manager).unwrap()["tunnels"][0]["forwardingUrl"],
            Value::Null
        );
        manager
            .inner
            .lock()
            .unwrap()
            .processes
            .get_mut(&id)
            .unwrap()
            .generation = store.generation.clone();
        manager.shutdown();
        let view = snapshot(&store, &manager).unwrap();
        assert_eq!(view["tunnels"][0]["forwardingUrl"], Value::Null);
        assert_eq!(view["tunnels"][0]["localAddress"], Value::Null);
        assert!(current.lock().unwrap().is_none());
    }
    #[test]
    fn logs_redact_and_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("log");
        append_log(&path, b"secret-token connected\n", "secret-token");
        assert!(!fs::read_to_string(&path).unwrap().contains("secret-token"));
        append_log(&path, &vec![b'x'; LOG_LIMIT * 2], "secret-token");
        assert_eq!(fs::metadata(path).unwrap().len(), LOG_LIMIT as u64);
    }
}
