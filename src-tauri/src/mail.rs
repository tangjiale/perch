//! 邮件账户与缓存。远端内容仅作为数据，所有网络请求均在存储锁外执行。
use crate::{
    integrations::{credential, now, read_secret},
    storage::Store,
    AppState,
};
use imap::types::{Flag, NameAttribute};
use lettre::{
    message::{header::ContentType, Attachment, Mailbox, MultiPart, SinglePart},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    Message, SmtpTransport, Transport,
};
use mailparse::MailHeaderMap;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

const MAX_MAIL: usize = 25 * 1024 * 1024;
const MAX_BATCH: usize = 100 * 1024 * 1024;
const CACHE_COUNT: u32 = 200;
const TIMEOUT: Duration = Duration::from_secs(20);
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account {
    pub id: String,
    #[serde(default)]
    pub revision: i64,
    pub name: String,
    pub email: String,
    pub sender_name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
    pub enabled: bool,
    pub sync_interval_seconds: u64,
    #[serde(default)]
    pub has_imap_credential: bool,
    #[serde(default)]
    pub has_smtp_credential: bool,
    #[serde(default)]
    pub last_sync: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub id: String,
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub reply_to_message_id: Option<String>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}
#[derive(Clone)]
struct Context {
    account: Account,
    workspace: String,
    generation: PathBuf,
}
fn db<T>(app: &AppHandle, f: impl FnOnce(&Store, &mut Connection) -> Result<T>) -> Result<T> {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|_| "工作空间繁忙")?;
    let mut conn = store.conn.lock().map_err(|_| "数据库繁忙")?;
    f(&store, &mut conn)
}
fn parse<T: serde::de::DeserializeOwned>(s: String) -> Result<T> {
    serde_json::from_str(&s).map_err(|_| "邮件数据格式无效".into())
}
fn sql<T>(v: rusqlite::Result<T>) -> Result<T> {
    v.map_err(|_| "邮件数据库操作失败".into())
}
fn account(conn: &Connection, id: &str) -> Result<Account> {
    parse(sql(conn.query_row(
        "SELECT data FROM mail_accounts WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?)
}
fn context(app: &AppHandle, id: &str) -> Result<Context> {
    db(app, |s, c| {
        Ok(Context {
            account: account(c, id)?,
            workspace: s.workspace_id.clone(),
            generation: s.generation.clone(),
        })
    })
}
fn verify(s: &Store, c: &Connection, ctx: &Context) -> Result<()> {
    if s.workspace_id != ctx.workspace || s.generation != ctx.generation {
        return Err("工作空间已切换，请重新操作".into());
    }
    if account(c, &ctx.account.id)?.revision != ctx.account.revision {
        return Err("邮箱配置已变更，请重新操作".into());
    }
    Ok(())
}
fn notify(app: &AppHandle, workspace: &str) {
    let _ = app.emit("mail-updated", json!({"workspaceId":workspace}));
}
fn validate_account(a: &Account) -> Result<()> {
    uuid::Uuid::parse_str(&a.id).map_err(|_| "邮箱 ID 无效")?;
    a.email
        .parse::<lettre::Address>()
        .map_err(|_| "邮箱地址格式无效")?;
    for v in [&a.name, &a.sender_name, &a.username, &a.smtp_username] {
        if v.trim().is_empty() || v.len() > 256 || v.chars().any(char::is_control) {
            return Err("邮箱名称与用户名不能为空或包含控制字符".into());
        }
    }
    for v in [&a.imap_host, &a.smtp_host] {
        if v.is_empty()
            || v.len() > 253
            || v.chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | '@' | '\\') || c.is_control())
        {
            return Err("服务器应填写主机名，不含协议或路径".into());
        }
    }
    if a.imap_port == 0
        || a.smtp_port == 0
        || !["tls", "starttls"].contains(&a.imap_security.as_str())
        || !["tls", "starttls"].contains(&a.smtp_security.as_str())
    {
        return Err("仅支持 TLS 或 STARTTLS 安全连接".into());
    }
    if !(60..=900).contains(&a.sync_interval_seconds) {
        return Err("刷新间隔须为 60 至 900 秒".into());
    }
    Ok(())
}
fn key_id(id: &str, protocol: &str) -> String {
    format!("mail:{id}:{protocol}")
}
fn secret(ctx: &Context, protocol: &str) -> Result<String> {
    let v = read_secret(&ctx.workspace, &key_id(&ctx.account.id, protocol))?;
    if v.is_empty() {
        Err(format!("请在邮箱设置中填写 {protocol} 密码或授权码"))
    } else {
        Ok(v)
    }
}
fn restore_secret(workspace: &str, id: &str, old: &str) {
    if let Ok(entry) = credential(workspace, id) {
        if old.is_empty() {
            let _ = entry.delete_credential();
        } else {
            let _ = entry.set_password(old);
        }
    }
}

#[tauri::command]
pub fn mail_accounts(app: AppHandle) -> Result<Vec<Account>> {
    db(&app, |_, c| list_accounts(c))
}
fn list_accounts(c: &Connection) -> Result<Vec<Account>> {
    let mut q = sql(c.prepare("SELECT data FROM mail_accounts ORDER BY rowid"))?;
    let rows = sql(q.query_map([], |r| r.get::<_, String>(0)))?
        .map(|r| parse(sql(r)?))
        .collect();
    rows
}
#[tauri::command]
pub fn mail_account_save(
    app: AppHandle,
    mut account: Account,
    imap_secret: Option<String>,
    smtp_secret: Option<String>,
) -> Result<Account> {
    validate_account(&account)?;
    db(&app, |store, c| {
        let _guard = Busy::acquire(format!("{}:{}", store.workspace_id, account.id))?;
        let old = sql(c
            .query_row(
                "SELECT data FROM mail_accounts WHERE id=?1",
                [&account.id],
                |r| r.get::<_, String>(0),
            )
            .optional())?
        .map(parse::<Account>)
        .transpose()?;
        if old.as_ref().map(|a| a.revision).unwrap_or(0) != account.revision {
            return Err("邮箱已被修改，请刷新后重试".into());
        }
        let mut pending = Vec::new();
        for (protocol, new) in [("imap", imap_secret), ("smtp", smtp_secret)] {
            if let Some(new) = new.filter(|s| !s.is_empty()) {
                if new.len() > 4096 {
                    return Err("授权码过长".into());
                }
                let id = key_id(&account.id, protocol);
                let previous = read_secret(&store.workspace_id, &id)?;
                let entry = credential(&store.workspace_id, &id)?;
                pending.push((id, previous, new, entry));
            }
        }
        let mut changed: Vec<(String, String)> = Vec::new();
        for (id, previous, new, entry) in pending {
            if entry.set_password(&new).is_err() {
                for (id, previous) in &changed {
                    restore_secret(&store.workspace_id, id, previous);
                }
                return Err("无法保存邮箱凭据到系统钥匙串".into());
            }
            changed.push((id, previous));
        }
        account.revision += 1;
        account.last_sync = old.as_ref().and_then(|a| a.last_sync);
        account.last_error = None;
        account.has_imap_credential = changed.iter().any(|(id, _)| id.ends_with(":imap"))
            || old.as_ref().is_some_and(|a| a.has_imap_credential);
        account.has_smtp_credential = changed.iter().any(|(id, _)| id.ends_with(":smtp"))
            || old.as_ref().is_some_and(|a| a.has_smtp_credential);
        let reset_cache = old.as_ref().is_some_and(|old| {
            old.email != account.email
                || old.username != account.username
                || old.imap_host != account.imap_host
                || old.imap_port != account.imap_port
        });
        if reset_cache {
            account.last_sync = None;
        }
        let result = (|| {
            let tx = sql(c.transaction())?;
            sql(tx.execute("INSERT INTO mail_accounts VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,data=excluded.data",params![account.id,account.revision,json!(account).to_string()]))?;
            if reset_cache {
                sql(tx.execute(
                    "DELETE FROM mail_messages WHERE account_id=?1",
                    [&account.id],
                ))?;
                sql(tx.execute(
                    "DELETE FROM mail_folders WHERE account_id=?1",
                    [&account.id],
                ))?;
            }
            sql(tx.commit())
        })();
        if let Err(e) = result {
            for (id, previous) in &changed {
                restore_secret(&store.workspace_id, id, previous);
            }
            return Err(e);
        }
        notify(&app, &store.workspace_id);
        Ok(account)
    })
}
#[tauri::command]
pub fn mail_account_delete(app: AppHandle, account_id: String, revision: i64) -> Result<()> {
    db(&app, |s, c| {
        if account(c, &account_id)?.revision != revision {
            return Err("邮箱配置已修改，请刷新后重试".into());
        }
        let _guard = Busy::acquire(format!("{}:{}", s.workspace_id, account_id))?;
        sql(c.execute(
            "DELETE FROM mail_accounts WHERE id=?1 AND revision=?2",
            params![account_id, revision],
        ))?;
        for p in ["imap", "smtp"] {
            if let Ok(e) = credential(&s.workspace_id, &key_id(&account_id, p)) {
                let _ = e.delete_credential();
            }
        }
        notify(&app, &s.workspace_id);
        Ok(())
    })
}

static BUSY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
pub fn ensure_idle() -> Result<()> {
    if BUSY
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "邮件服务繁忙")?
        .is_empty()
    {
        Ok(())
    } else {
        Err("邮件正在同步或发送，请稍后再切换工作空间".into())
    }
}
struct Busy(String);
impl Busy {
    fn acquire(key: String) -> Result<Self> {
        if !BUSY
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "邮件服务繁忙")?
            .insert(key.clone())
        {
            return Err("该邮箱正在操作，请稍后重试".into());
        }
        Ok(Self(key))
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        if let Ok(mut b) = BUSY.get_or_init(Default::default).lock() {
            b.remove(&self.0);
        }
    }
}

// 限制单连接接收量和总用时，避免恶意或异常服务器无限返回数据。
struct LimitedStream {
    inner: native_tls::TlsStream<TcpStream>,
    bytes: usize,
    started: Instant,
}
impl Read for LimitedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes >= MAX_BATCH + MAX_MAIL || self.started.elapsed() > Duration::from_secs(180) {
            return Err(std::io::Error::other("mail response limit"));
        }
        let max = buf.len().min(MAX_BATCH + MAX_MAIL - self.bytes);
        let n = self.inner.read(&mut buf[..max])?;
        self.bytes += n;
        Ok(n)
    }
}
impl Write for LimitedStream {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.started.elapsed() > Duration::from_secs(180) {
            return Err(std::io::Error::other("mail operation timeout"));
        }
        self.inner.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
type Session = imap::Session<LimitedStream>;
fn login(ctx: &Context) -> Result<Session> {
    let a = &ctx.account;
    let pass = secret(ctx, "imap")?;
    let addresses = (a.imap_host.as_str(), a.imap_port)
        .to_socket_addrs()
        .map_err(|_| "IMAP 服务器地址无法解析")?;
    let mut stream = None;
    for addr in addresses.take(4) {
        if let Ok(s) = TcpStream::connect_timeout(&addr, TIMEOUT) {
            stream = Some(s);
            break;
        }
    }
    let mut stream = stream.ok_or("无法连接 IMAP 服务器，请检查主机和端口")?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| "无法设置 IMAP 超时")?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|_| "无法设置 IMAP 超时")?;
    let tls = native_tls::TlsConnector::new().map_err(|_| "无法初始化 TLS")?;
    if a.imap_security == "starttls" {
        if !read_line(&mut stream)?
            .to_ascii_uppercase()
            .starts_with("* OK")
        {
            return Err("IMAP 服务响应无效".into());
        }
        stream
            .write_all(b"p0 STARTTLS\r\n")
            .map_err(|_| "IMAP STARTTLS 请求失败")?;
        let mut accepted = false;
        for _ in 0..10 {
            let line = read_line(&mut stream)?.to_ascii_uppercase();
            if line.starts_with("P0 ") {
                accepted = line.starts_with("P0 OK ") || line.trim() == "P0 OK";
                break;
            }
        }
        if !accepted {
            return Err("服务器未接受 STARTTLS，不会回退到明文认证".into());
        }
    }
    let secure = tls
        .connect(&a.imap_host, stream)
        .map_err(|_| "IMAP TLS 失败，请检查证书与安全连接类型")?;
    let mut client = imap::Client::new(LimitedStream {
        inner: secure,
        bytes: 0,
        started: Instant::now(),
    });
    if a.imap_security == "tls" {
        client.read_greeting().map_err(|_| "IMAP 服务响应无效")?;
    }
    client
        .login(&a.username, pass)
        .map_err(|_| "IMAP 登录失败，请确认用户名和邮箱授权码".into())
}
fn read_line(stream: &mut TcpStream) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    let started = Instant::now();
    while line.len() < 8192 {
        if started.elapsed() > TIMEOUT {
            return Err("IMAP 握手超时".into());
        }
        stream
            .read_exact(&mut byte)
            .map_err(|_| "IMAP 握手超时或连接已断开")?;
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return String::from_utf8(line).map_err(|_| "IMAP 握手响应无效".into());
        }
    }
    Err("IMAP 握手响应过长".into())
}
fn smtp(ctx: &Context) -> Result<SmtpTransport> {
    let a = &ctx.account;
    let tls = TlsParameters::new(a.smtp_host.clone()).map_err(|_| "SMTP TLS 配置无效")?;
    Ok(SmtpTransport::builder_dangerous(&a.smtp_host)
        .port(a.smtp_port)
        .timeout(Some(TIMEOUT))
        .tls(if a.smtp_security == "tls" {
            Tls::Wrapper(tls)
        } else {
            Tls::Required(tls)
        })
        .credentials(Credentials::new(
            a.smtp_username.clone(),
            secret(ctx, "smtp")?,
        ))
        .build())
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| "邮件操作意外中止".to_string())?
}
#[tauri::command]
pub async fn mail_account_test(app: AppHandle, account_id: String) -> Result<String> {
    blocking(move || {
        let ctx = context(&app, &account_id)?;
        let mut s = login(&ctx)?;
        let _ = s.logout();
        if !smtp(&ctx)?
            .test_connection()
            .map_err(|_| "SMTP 连接或认证失败，请检查配置")?
        {
            return Err("SMTP 服务未接受连接".into());
        }
        db(&app, |s, c| verify(s, c, &ctx))?;
        Ok("IMAP 与 SMTP 安全连接及认证成功".into())
    })
    .await
}

fn folder_kind(path: &str, attributes: &[NameAttribute<'_>]) -> &'static str {
    let p = path.to_lowercase();
    let flag = |s: &str| {
        attributes
            .iter()
            .any(|a| matches!(a,NameAttribute::Custom(v) if v.eq_ignore_ascii_case(s)))
    };
    if p == "inbox" {
        "inbox"
    } else if flag("\\Sent")
        || ["sent", "sent messages", "sent items", "已发送"].contains(&p.as_str())
    {
        "sent"
    } else if flag("\\Drafts") || ["drafts", "草稿箱", "草稿"].contains(&p.as_str()) {
        "drafts"
    } else if flag("\\Trash")
        || ["trash", "deleted messages", "deleted items", "已删除"].contains(&p.as_str())
    {
        "trash"
    } else if flag("\\Junk") || ["junk", "spam", "垃圾邮件"].contains(&p.as_str()) {
        "junk"
    } else if flag("\\Archive") || ["archive", "归档"].contains(&p.as_str()) {
        "archive"
    } else {
        "other"
    }
}
fn folders_remote<T: Read + Write>(session: &mut imap::Session<T>) -> Result<Vec<Value>> {
    let names = session
        .list(None, Some("*"))
        .map_err(|_| "无法读取邮箱文件夹")?;
    if names.len() > 200 {
        return Err("邮箱文件夹超过 200 个，暂不支持同步".into());
    }
    let mut out = Vec::new();
    for name in names
        .iter()
        .filter(|n| !n.attributes().contains(&NameAttribute::NoSelect))
    {
        let status = folder_status(session, name.name())?;
        out.push(json!({"path":name.name(),"name":decode_folder_name(name.name()),"kind":folder_kind(name.name(),name.attributes()),"unread":status.unseen.unwrap_or(0),"total":status.exists}));
    }
    Ok(out)
}
fn folder_status<T: Read + Write>(
    session: &mut imap::Session<T>,
    path: &str,
) -> Result<imap::types::Mailbox> {
    // imap 2.4 将 STATUS 放在 unsolicited_responses；返回 Mailbox 的默认零值不是计数。
    for _ in session.unsolicited_responses.try_iter() {}
    let mut mailbox = session
        .status(path, "(MESSAGES UNSEEN)")
        .map_err(|_| "无法读取邮箱文件夹计数")?;
    let mut found = false;
    for response in session.unsolicited_responses.try_iter() {
        if let imap::types::UnsolicitedResponse::Status {
            mailbox: response_path,
            attributes,
        } = response
        {
            if response_path != path
                && !(response_path.eq_ignore_ascii_case("INBOX")
                    && path.eq_ignore_ascii_case("INBOX"))
            {
                continue;
            }
            let (mut messages, mut unseen) = (None, None);
            for attribute in attributes {
                match attribute {
                    imap::types::StatusAttribute::Messages(n) => messages = Some(n),
                    imap::types::StatusAttribute::Unseen(n) => unseen = Some(n),
                    _ => {}
                }
            }
            if let (Some(messages), Some(unseen)) = (messages, unseen) {
                mailbox.exists = messages;
                mailbox.unseen = Some(unseen);
                found = true;
            }
        }
    }
    if !found {
        return Err("服务器未返回完整邮件计数，保留原有缓存".into());
    }
    Ok(mailbox)
}
fn decode_folder_name(name: &str) -> String {
    use base64::Engine;
    let mut result = String::new();
    let mut rest = name;
    while let Some(i) = rest.find('&') {
        result.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if let Some(end) = rest.find('-') {
            let encoded = &rest[..end];
            if encoded.is_empty() {
                result.push('&');
            } else {
                let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
                    .decode(encoded.replace(',', "/"));
                if let Ok(bytes) = bytes {
                    let words: Vec<u16> = bytes
                        .chunks(2)
                        .filter(|b| b.len() == 2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect();
                    result.push_str(&String::from_utf16_lossy(&words));
                } else {
                    result.push('&');
                    result.push_str(encoded);
                    result.push('-');
                }
            }
            rest = &rest[end + 1..];
        } else {
            result.push('&');
            break;
        }
    }
    result.push_str(rest);
    result
}
fn addresses(parsed: &mailparse::ParsedMail<'_>, header: &str) -> Vec<String> {
    parsed
        .headers
        .get_first_value(header)
        .and_then(|s| mailparse::addrparse(&s).ok())
        .map(|a| {
            a.iter()
                .flat_map(|x| match x {
                    mailparse::MailAddr::Single(a) => vec![a.addr.clone()],
                    mailparse::MailAddr::Group(g) => {
                        g.addrs.iter().map(|a| a.addr.clone()).collect()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
fn parts(
    parsed: &mailparse::ParsedMail<'_>,
    path: &str,
    text: &mut String,
    html: &mut String,
    attachments: &mut Vec<(Value, Vec<u8>)>,
) -> Result<()> {
    if path.len() > 80 || attachments.len() > 100 {
        return Err("邮件 MIME 层级或附件数超出限制".into());
    }
    let disposition = parsed.get_content_disposition();
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| parsed.ctype.params.get("name"));
    if filename.is_some() || disposition.disposition == mailparse::DispositionType::Attachment {
        let bytes = parsed.get_body_raw().map_err(|_| "邮件附件解码失败")?;
        let name = filename.map(String::as_str).unwrap_or("附件");
        attachments.push((
            json!({"id":path,"name":name,"size":bytes.len(),"contentType":parsed.ctype.mimetype}),
            bytes,
        ));
    } else if parsed.subparts.is_empty() {
        if parsed.ctype.mimetype == "text/plain" {
            text.push_str(&parsed.get_body().map_err(|_| "邮件正文解码失败")?);
            text.push('\n');
        }
        if parsed.ctype.mimetype == "text/html" {
            html.push_str(&parsed.get_body().map_err(|_| "邮件正文解码失败")?);
        }
    } else {
        for (i, p) in parsed.subparts.iter().enumerate() {
            parts(p, &format!("{path}.{i}"), text, html, attachments)?;
        }
    }
    Ok(())
}
type DecodedMail<'a> = (
    mailparse::ParsedMail<'a>,
    String,
    String,
    Vec<(Value, Vec<u8>)>,
);
fn decode(raw: &[u8]) -> Result<DecodedMail<'_>> {
    if raw.len() > MAX_MAIL {
        return Err("单封邮件超过 25 MB 限制".into());
    }
    let parsed = mailparse::parse_mail(raw).map_err(|_| "邮件 MIME 格式无法解析")?;
    let (mut text, mut html, mut attachments) = (String::new(), String::new(), Vec::new());
    parts(&parsed, "0", &mut text, &mut html, &mut attachments)?;
    if text.trim().is_empty() && !html.is_empty() {
        text = scraper::Html::parse_document(&html)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ");
    }
    Ok((parsed, text, html, attachments))
}
fn message_value(
    account_id: &str,
    folder: &str,
    validity: u32,
    fetch: &imap::types::Fetch,
    raw: &[u8],
) -> Result<Value> {
    let uid = fetch.uid.ok_or("邮件缺少 UID")?;
    let (p, text, _, attachments) = decode(raw)?;
    let date = p
        .headers
        .get_first_value("Date")
        .and_then(|s| mailparse::dateparse(&s).ok())
        .and_then(|s| s.checked_mul(1000))
        .or_else(|| fetch.internal_date().map(|d| d.timestamp_millis()))
        .unwrap_or(0);
    use sha2::{Digest, Sha256};
    let id = format!(
        "mail-{}",
        hex::encode(Sha256::digest(format!(
            "{account_id}\0{folder}\0{validity}\0{uid}"
        )))
    );
    Ok(
        json!({"id":id,"accountId":account_id,"folder":folder,"uid":uid,"subject":p.headers.get_first_value("Subject").unwrap_or_else(||"（无主题）".into()),"from":p.headers.get_first_value("From").unwrap_or_default(),"to":addresses(&p,"To"),"cc":addresses(&p,"Cc"),"date":date,"preview":text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect::<String>(),"seen":fetch.flags().contains(&Flag::Seen),"flagged":fetch.flags().contains(&Flag::Flagged),"hasAttachments":!attachments.is_empty()}),
    )
}

fn sync_inner(app: &AppHandle, account_id: &str, folder: Option<&str>) -> Result<String> {
    let ctx = context(app, account_id)?;
    let _guard = Busy::acquire(format!("{}:{account_id}", ctx.workspace))?;
    let result: Result<String> = (|| {
        let mut session = login(&ctx)?;
        let mut folders = folders_remote(&mut session)?;
        for f in &mut folders {
            f["accountId"] = json!(account_id);
        }
        let requested = folder.unwrap_or("INBOX");
        let folder = folders
            .iter()
            .find(|f| {
                f["path"].as_str().is_some_and(|p| {
                    p == requested
                        || p.eq_ignore_ascii_case("INBOX")
                            && requested.eq_ignore_ascii_case("INBOX")
                })
            })
            .and_then(|f| f["path"].as_str())
            .ok_or("邮箱文件夹不存在，请刷新后重试")?;
        let selected = session.select(folder).map_err(|_| "无法打开邮件文件夹")?;
        let validity = selected
            .uid_validity
            .ok_or("服务器未提供 UIDVALIDITY，无法安全缓存邮件")?;
        let mut items = Vec::new();
        let mut bytes = 0;
        let mut omitted = 0;
        if selected.exists > 0 {
            let range = format!(
                "{}:{}",
                selected.exists.saturating_sub(CACHE_COUNT - 1).max(1),
                selected.exists
            );
            let metadata = session
                .fetch(range, "(UID FLAGS RFC822.SIZE)")
                .map_err(|_| "无法读取邮件列表")?;
            for meta in metadata.iter().rev() {
                let uid = meta.uid.ok_or("邮件缺少 UID")?;
                if meta.size.unwrap_or(u32::MAX) as usize > MAX_MAIL
                    || bytes + meta.size.unwrap_or(0) as usize > MAX_BATCH
                {
                    omitted += 1;
                    continue;
                }
                let cached: Option<(String, Vec<u8>)> = db(app, |s, c| {
                    verify(s, c, &ctx)?;
                    sql(c.query_row("SELECT data,raw FROM mail_messages WHERE account_id=?1 AND folder=?2 AND uid_validity=?3 AND uid=?4",params![account_id,folder,validity,uid],|r|Ok((r.get(0)?,r.get(1)?))).optional())
                })?;
                if let Some((data, raw)) = cached {
                    let mut m: Value = parse(data)?;
                    m["seen"] = json!(meta.flags().contains(&Flag::Seen));
                    m["flagged"] = json!(meta.flags().contains(&Flag::Flagged));
                    bytes += raw.len();
                    items.push((m, raw));
                    continue;
                }
                let fetched = session
                    .uid_fetch(uid.to_string(), "(UID FLAGS INTERNALDATE BODY.PEEK[])")
                    .map_err(|_| "读取邮件内容失败，请重试")?;
                if let Some(f) = fetched.first() {
                    let raw = f.body().ok_or("邮件缺少正文")?;
                    bytes += raw.len();
                    items.push((
                        message_value(account_id, folder, validity, f, raw)?,
                        raw.to_vec(),
                    ));
                }
            }
        }
        let _ = session.logout();
        db(app, |s, c| {
            verify(s, c, &ctx)?;
            let tx = sql(c.transaction())?;
            sql(tx.execute("DELETE FROM mail_folders WHERE account_id=?1", [account_id]))?;
            for f in &folders {
                sql(tx.execute(
                    "INSERT INTO mail_folders VALUES(?1,?2,?3)",
                    params![account_id, f["path"].as_str(), f.to_string()],
                ))?;
            }
            // 仅淘汰窗口外/失效 UID；不可变正文不重复写入，避免每分钟重写整批附件。
            let retained = items
                .iter()
                .filter_map(|(m, _)| m["uid"].as_u64())
                .map(|uid| uid.to_string())
                .collect::<Vec<_>>()
                .join(",");
            sql(tx.execute(
                &format!("DELETE FROM mail_messages WHERE account_id=?1 AND folder=?2 AND (uid_validity<>?3 OR uid NOT IN ({retained}))"),
                params![account_id, folder,validity],
            ))?;
            for (m, raw) in &items {
                sql(tx.execute(
                    "INSERT INTO mail_messages VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE mail_messages.data<>excluded.data",
                    params![
                        m["id"].as_str(),
                        account_id,
                        folder,
                        validity,
                        m["uid"].as_u64(),
                        m["date"].as_i64(),
                        m.to_string(),
                        raw
                    ],
                ))?;
            }
            let mut updated = ctx.account.clone();
            updated.last_sync = Some(now());
            updated.last_error = None;
            sql(tx.execute(
                "UPDATE mail_accounts SET data=?2 WHERE id=?1",
                params![account_id, json!(updated).to_string()],
            ))?;
            sql(tx.commit())?;
            Ok(())
        })?;
        notify(app, &ctx.workspace);
        Ok(if omitted > 0 {
            format!(
                "已同步 {} 封邮件；{} 封因单封 25 MB 或本次 100 MB 限额暂未缓存",
                items.len(),
                omitted
            )
        } else {
            format!("已同步 {} 个文件夹、{} 封邮件", folders.len(), items.len())
        })
    })();
    if let Err(e) = &result {
        let _ = db(app, |s, c| {
            verify(s, c, &ctx)?;
            let mut updated = ctx.account.clone();
            updated.last_error = Some(e.clone());
            sql(c.execute(
                "UPDATE mail_accounts SET data=?2 WHERE id=?1",
                params![account_id, json!(updated).to_string()],
            ))?;
            Ok(())
        });
        notify(app, &ctx.workspace);
    }
    result
}
#[tauri::command]
pub async fn mail_sync(
    app: AppHandle,
    account_id: String,
    folder: Option<String>,
) -> Result<String> {
    blocking(move || sync_inner(&app, &account_id, folder.as_deref())).await
}
#[tauri::command]
pub fn mail_folders(app: AppHandle, account_id: Option<String>) -> Result<Vec<Value>> {
    db(&app, |_, c| {
        let mut q = sql(c.prepare(
            "SELECT data FROM mail_folders WHERE (?1 IS NULL OR account_id=?1) ORDER BY path",
        ))?;
        let rows = sql(q.query_map([account_id], |r| r.get::<_, String>(0)))?
            .map(|r| parse(sql(r)?))
            .collect();
        rows
    })
}
#[tauri::command]
pub fn mail_messages(
    app: AppHandle,
    account_id: String,
    folder: String,
    search: String,
    limit: u32,
    offset: u32,
) -> Result<Value> {
    if search.len() > 500 {
        return Err("搜索词过长".into());
    }
    db(&app, |_, c| {
        query_messages(c, &account_id, &folder, &search, limit, offset)
    })
}

fn query_messages(
    c: &Connection,
    account_id: &str,
    folder: &str,
    search: &str,
    limit: u32,
    offset: u32,
) -> Result<Value> {
    let pattern = format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    // 原始字符串保留 SQL 转义字符，普通 Rust 字符串中的 \' 会丢失反斜杠。
    let filter = r"account_id=?1 AND folder=?2 AND (json_extract(data,'$.subject') LIKE ?3 ESCAPE '\' OR json_extract(data,'$.from') LIKE ?3 ESCAPE '\' OR json_extract(data,'$.preview') LIKE ?3 ESCAPE '\')";
    let total: i64 = sql(c.query_row(
        &format!("SELECT count(*) FROM mail_messages WHERE {filter}"),
        params![account_id, folder, pattern],
        |r| r.get(0),
    ))?;
    let mut q = sql(c.prepare(&format!(
        "SELECT data FROM mail_messages WHERE {filter} ORDER BY date_ms DESC,id LIMIT ?4 OFFSET ?5"
    )))?;
    let messages: Vec<Value> = sql(q.query_map(
        params![account_id, folder, pattern, limit.clamp(1, 200), offset],
        |r| r.get::<_, String>(0),
    ))?
    .map(|r| parse(sql(r)?))
    .collect::<Result<_>>()?;
    Ok(json!({"messages":messages,"total":total}))
}
fn cached(c: &Connection, id: &str) -> Result<(Value, Vec<u8>, u32)> {
    let (data, raw, v): (String, Vec<u8>, u32) = sql(c.query_row(
        "SELECT data,raw,uid_validity FROM mail_messages WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    Ok((parse(data)?, raw, v))
}
#[tauri::command]
pub fn mail_message(app: AppHandle, message_id: String) -> Result<Value> {
    db(&app, |_, c| {
        let (mut m, raw, _) = cached(c, &message_id)?;
        let (_, text, html, attachments) = decode(&raw)?;
        m["text"] = json!(text);
        m["html"] = json!(html);
        m["attachments"] = json!(attachments.iter().map(|(a, _)| a).collect::<Vec<_>>());
        Ok(m)
    })
}
#[tauri::command]
pub fn mail_attachment_save(
    app: AppHandle,
    message_id: String,
    attachment_id: String,
    destination: String,
) -> Result<()> {
    let bytes = db(&app, |_, c| {
        let (_, raw, _) = cached(c, &message_id)?;
        let (_, _, _, attachments) = decode(&raw)?;
        attachments
            .into_iter()
            .find(|(a, _)| a["id"] == attachment_id)
            .map(|(_, b)| b)
            .ok_or_else(|| "附件不存在".into())
    })?;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|_| "无法保存附件，请选择未存在的文件名")?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "附件写入失败".into())
}
fn message_context(app: &AppHandle, id: &str) -> Result<(Context, Value, u32)> {
    db(app, |s, c| {
        let (m, _, v) = cached(c, id)?;
        let a = account(c, m["accountId"].as_str().ok_or("邮件账户无效")?)?;
        Ok((
            Context {
                account: a,
                workspace: s.workspace_id.clone(),
                generation: s.generation.clone(),
            },
            m,
            v,
        ))
    })
}
#[tauri::command]
pub async fn mail_message_flag(
    app: AppHandle,
    message_id: String,
    seen: Option<bool>,
    flagged: Option<bool>,
) -> Result<()> {
    blocking(move||{
        let(ctx,mut m,validity)=message_context(&app,&message_id)?;
        let _guard=Busy::acquire(format!("{}:{}",ctx.workspace,ctx.account.id))?;
        let mut session=login(&ctx)?;
        if session.select(m["folder"].as_str().ok_or("文件夹无效")?).map_err(|_|"无法打开文件夹")?.uid_validity!=Some(validity){return Err("邮件 UID 已变化，请重新同步".into());}
        if session.uid_fetch(m["uid"].to_string(),"(UID)").map_err(|_|"无法核验邮件")?.is_empty(){return Err("邮件已在远端移走或删除，请重新同步".into());}
        for (field,flag,value) in [("seen","\\Seen",seen),("flagged","\\Flagged",flagged)] {if let Some(value)=value {session.uid_store(m["uid"].to_string(),format!("{}FLAGS.SILENT ({flag})",if value{"+"}else{"-"})).map_err(|_|"更新邮件标记失败")?;m[field]=json!(value);}}
        let status=folder_status(&mut session,m["folder"].as_str().ok_or("文件夹无效")?).map_err(|_|"标记已更新，但未读计数刷新失败，请同步邮箱")?;
        let _=session.logout();
        db(&app,|s,c|{verify(s,c,&ctx)?;let tx=sql(c.transaction())?;sql(tx.execute("UPDATE mail_messages SET data=?2 WHERE id=?1",params![message_id,m.to_string()]))?;sql(tx.execute("UPDATE mail_folders SET data=json_set(data,'$.unread',?3,'$.total',?4) WHERE account_id=?1 AND path=?2",params![ctx.account.id,m["folder"].as_str(),status.unseen.unwrap_or(0),status.exists]))?;sql(tx.commit())?;Ok(())})?;
        notify(&app,&ctx.workspace);Ok(())
    }).await
}
#[tauri::command]
pub async fn mail_message_move(
    app: AppHandle,
    message_id: String,
    target_folder: String,
) -> Result<()> {
    blocking(move || {
        let (ctx, m, validity) = message_context(&app, &message_id)?;
        if m["folder"].as_str()==Some(&target_folder){return Err("请选择其他目标文件夹".into());}
        let _guard = Busy::acquire(format!("{}:{}", ctx.workspace, ctx.account.id))?;
        let mut session = login(&ctx)?;
        if session
            .select(m["folder"].as_str().ok_or("文件夹无效")?)
            .map_err(|_| "无法打开文件夹")?
            .uid_validity
            != Some(validity)
        {
            return Err("邮件 UID 已变化，请重新同步".into());
        }
        if session.uid_fetch(m["uid"].to_string(),"(UID)").map_err(|_|"无法核验邮件")?.is_empty(){return Err("邮件已在远端移走或删除，请重新同步".into());}
        if !session
            .capabilities()
            .map_err(|_| "无法读取 IMAP 能力")?
            .has_str("MOVE")
        {
            return Err("服务器不支持安全 MOVE 操作，请在网页邮箱中移动；不会执行全局清除".into());
        }
        session
            .uid_mv(m["uid"].to_string(), &target_folder)
            .map_err(|_| "移动邮件失败，请检查目标文件夹")?;
        let _ = session.logout();
        db(&app, |s, c| {
            verify(s, c, &ctx)?;
            let tx=sql(c.transaction())?;
            sql(tx.execute("DELETE FROM mail_messages WHERE id=?1", [message_id]))?;
            let unread=if m["seen"]==true{0}else{1};
            sql(tx.execute("UPDATE mail_folders SET data=json_set(data,'$.unread',MAX(0,json_extract(data,'$.unread')-?3),'$.total',MAX(0,json_extract(data,'$.total')-1)) WHERE account_id=?1 AND path=?2",params![ctx.account.id,m["folder"].as_str(),unread]))?;
            sql(tx.execute("UPDATE mail_folders SET data=json_set(data,'$.unread',json_extract(data,'$.unread')+?3,'$.total',json_extract(data,'$.total')+1) WHERE account_id=?1 AND path=?2",params![ctx.account.id,target_folder,unread]))?;
            sql(tx.commit())?;
            Ok(())
        })?;
        notify(&app, &ctx.workspace);
        Ok(())
    })
    .await
}

fn validate_draft(d: &Draft) -> Result<()> {
    uuid::Uuid::parse_str(&d.id).map_err(|_| "草稿 ID 无效")?;
    if d.to.len() + d.cc.len() + d.bcc.len() > 100
        || d.subject.len() > 2000
        || d.text.len() > 2 * 1024 * 1024
        || d.attachments.len() > 20
    {
        return Err("草稿超出收件人、正文或附件数量限制".into());
    }
    for a in d.to.iter().chain(&d.cc).chain(&d.bcc) {
        a.parse::<lettre::Address>()
            .map_err(|_| "收件人邮箱格式无效")?;
    }
    let mut size = 0;
    for p in &d.attachments {
        let m = std::fs::metadata(p).map_err(|_| "附件不存在或无法读取")?;
        if !m.is_file() {
            return Err("附件必须是文件".into());
        }
        size += m.len();
    }
    if size > 18 * 1024 * 1024 {
        return Err("附件合计不能超过 18 MB".into());
    }
    Ok(())
}
#[tauri::command]
pub fn mail_drafts(app: AppHandle, account_id: String) -> Result<Vec<Draft>> {
    db(&app, |_, c| {
        let mut q=sql(c.prepare("SELECT data FROM mail_drafts WHERE account_id=?1 AND state='draft' ORDER BY rowid DESC"))?;
        let rows = sql(q.query_map([account_id], |r| r.get::<_, String>(0)))?
            .map(|r| parse(sql(r)?))
            .collect();
        rows
    })
}
#[tauri::command]
pub fn mail_draft_save(app: AppHandle, mut draft: Draft) -> Result<Draft> {
    validate_draft(&draft)?;
    db(&app, |s, c| {
        let _guard = Busy::acquire(format!("{}:{}", s.workspace_id, draft.account_id))?;
        account(c, &draft.account_id)?;
        let old: Option<(String, String)> = sql(c
            .query_row(
                "SELECT account_id,state FROM mail_drafts WHERE id=?1",
                [&draft.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        if old
            .as_ref()
            .is_some_and(|(id, state)| id != &draft.account_id || state != "draft")
        {
            return Err("草稿已发送或正在发送，不能覆盖".into());
        }
        draft.updated_at = Some(now());
        sql(c.execute("INSERT INTO mail_drafts(id,account_id,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![draft.id,draft.account_id,json!(draft).to_string()]))?;
        Ok(draft)
    })
}
#[tauri::command]
pub fn mail_draft_delete(app: AppHandle, draft_id: String) -> Result<()> {
    db(&app, |_, c| {
        sql(c.execute(
            "DELETE FROM mail_drafts WHERE id=?1 AND state='draft'",
            [draft_id],
        ))?;
        Ok(())
    })
}
fn build_message(ctx: &Context, d: &Draft) -> Result<Message> {
    validate_draft(d)?;
    if d.to.is_empty() && d.cc.is_empty() && d.bcc.is_empty() {
        return Err("请填写收件人".into());
    }
    let from = Mailbox::new(
        Some(ctx.account.sender_name.clone()),
        ctx.account.email.parse().map_err(|_| "发件地址无效")?,
    );
    let mut b = Message::builder().from(from).subject(&d.subject);
    for a in &d.to {
        b = b.to(a.parse().map_err(|_| "收件地址无效")?);
    }
    for a in &d.cc {
        b = b.cc(a.parse().map_err(|_| "抄送地址无效")?);
    }
    for a in &d.bcc {
        b = b.bcc(a.parse().map_err(|_| "密送地址无效")?);
    }
    let mut mixed = MultiPart::mixed().singlepart(SinglePart::plain(d.text.clone()));
    let mut attachment_bytes = 0usize;
    for path in &d.attachments {
        let mut f = std::fs::File::open(path).map_err(|_| "无法打开附件")?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut f)
            .take(MAX_MAIL as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "无法读取附件")?;
        attachment_bytes += bytes.len();
        if attachment_bytes > 18 * 1024 * 1024 {
            return Err("附件超出大小限制".into());
        }
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("附件")
            .to_string();
        mixed = mixed.singlepart(Attachment::new(name).body(
            bytes,
            ContentType::parse("application/octet-stream").map_err(|_| "附件类型无效")?,
        ));
    }
    let message = b.multipart(mixed).map_err(|_| "邮件格式无效")?;
    if message.formatted().len() > MAX_MAIL {
        return Err("编码后的邮件超过 25 MB 限制".into());
    }
    Ok(message)
}
#[tauri::command]
pub async fn mail_send(app: AppHandle, draft_id: String) -> Result<Value> {
    blocking(move||{
        let d:Draft=db(&app,|_,c|parse(sql(c.query_row("SELECT data FROM mail_drafts WHERE id=?1 AND state='draft'",[&draft_id],|r|r.get(0)))?))?;
        let ctx=context(&app,&d.account_id)?;
        let _guard=Busy::acquire(format!("{}:{}",ctx.workspace,d.account_id))?;
        let message=build_message(&ctx,&d)?;let transport=smtp(&ctx)?;
        db(&app,|s,c|{verify(s,c,&ctx)?;if sql(c.execute("UPDATE mail_drafts SET state='sending' WHERE id=?1 AND state='draft'",[&draft_id]))?!=1{return Err("该邮件已发送或正在发送".into());}Ok(())})?;
        // 从 SMTP 开始发送后遇到断线，接收结果可能不确定；不自动恢复可发送状态，避免重复投递。
        if let Err(error)=transport.send(&message) {
            if error.is_permanent() || error.is_transient() {
                db(&app,|s,c|{verify(s,c,&ctx)?;sql(c.execute("UPDATE mail_drafts SET state='draft' WHERE id=?1",[&draft_id]))?;Ok(())})?;
                return Err("SMTP 服务器拒绝了本次发送，邮件未被接受；草稿已保留，请检查授权码、发件权限与收件地址后重试".into());
            }
            let _=db(&app,|s,c|{verify(s,c,&ctx)?;sql(c.execute("UPDATE mail_drafts SET state='uncertain' WHERE id=?1",[&draft_id]))?;Ok(())});
            return Err("SMTP 发送未确认，可能连接失败或服务器拒绝。为避免重复发送，此草稿已锁定；请先检查网页邮箱或与收件人确认，再新建邮件".into());
        }
        let saved=db(&app,|s,c|{verify(s,c,&ctx)?;sql(c.execute("UPDATE mail_drafts SET state='sent' WHERE id=?1",[&draft_id]))?;Ok(())});
        if saved.is_err(){return Ok(json!({"message":"SMTP 已接受邮件，但本地状态保存失败。请勿再次发送，可到网页邮箱确认"}));}
        let append=(||{let mut session=login(&ctx)?;let folders=folders_remote(&mut session)?;let sent=folders.iter().find(|f|f["kind"]=="sent").and_then(|f|f["path"].as_str()).ok_or("未找到已发送文件夹")?;session.append_with_flags(sent,message.formatted(),&[Flag::Seen]).map_err(|_|"无法保存发送副本")?;let _=session.logout();Ok::<(),String>(())})();
        notify(&app,&ctx.workspace);
        Ok(json!({"message":if append.is_ok(){"邮件已发送，并保存到已发送文件夹"}else{"邮件已发送，但发送副本未保存到 IMAP 已发送文件夹。请勿重复发送"}}))
    }).await
}
#[tauri::command]
pub fn mail_unread(app: AppHandle) -> Result<Value> {
    db(&app, |_, c| {
        let mut accounts = Vec::new();
        let mut total = 0_i64;
        for a in list_accounts(c)?.into_iter().filter(|a| a.enabled) {
            let unread:i64=sql(c.query_row("SELECT COALESCE(SUM(json_extract(data,'$.unread')),0) FROM mail_folders WHERE account_id=?1 AND json_extract(data,'$.kind')='inbox'",[&a.id],|r|r.get(0)))?;
            total += unread;
            accounts.push(json!({"accountId":a.id,"unread":unread}));
        }
        Ok(json!({"total":total,"accounts":accounts}))
    })
}
#[tauri::command]
pub fn mail_unread_count(app: AppHandle) -> Result<i64> {
    Ok(mail_unread(app)?["total"].as_i64().unwrap_or(0))
}
pub fn start_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            let accounts = db(&app, |s, c| {
                Ok((
                    s.workspace_id.clone(),
                    s.generation.clone(),
                    list_accounts(c)?,
                ))
            });
            if let Ok((workspace, generation, accounts)) = accounts {
                for a in accounts.into_iter().filter(|a| a.enabled) {
                    let key = format!(
                        "{workspace}:{}:{}:{}",
                        generation.display(),
                        a.id,
                        a.revision
                    );
                    if last
                        .get(&key)
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(a.sync_interval_seconds))
                    {
                        continue;
                    }
                    last.insert(key, Instant::now());
                    let handle = app.clone();
                    let workspace = workspace.clone();
                    let generation = generation.clone();
                    let _ = blocking(move || {
                        let current = context(&handle, &a.id)?;
                        if current.workspace != workspace || current.generation != generation {
                            return Err("工作空间已切换".into());
                        }
                        if !current.account.enabled || current.account.revision != a.revision {
                            return Err("邮箱配置已变更".into());
                        }
                        sync_inner(&handle, &a.id, None)
                    })
                    .await;
                }
            }
            last.retain(|_, t| t.elapsed() < Duration::from_secs(3600));
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn cached_messages_list_search_and_pagination_handle_literal_wildcards() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/002_mail.sql"))
            .unwrap();
        c.execute("INSERT INTO mail_accounts VALUES('a',1,'{}')", [])
            .unwrap();
        for (id, folder, subject, date) in [
            ("one", "INBOX", "预算 100%", 1),
            ("two", "INBOX", "file_name", 2),
            ("three", "INBOX", r"目录 C:\work", 3),
            ("other", "Trash", "预算 100%", 4),
        ] {
            let data = serde_json::json!({"id":id,"subject":subject,"from":"sender@example.test","preview":"项目摘要"});
            c.execute(
                "INSERT INTO mail_messages VALUES(?1,'a',?2,1,?3,?3,?4,X'')",
                rusqlite::params![id, folder, date, data.to_string()],
            )
            .unwrap();
        }
        let list = super::query_messages(&c, "a", "INBOX", "", 2, 0).unwrap();
        assert_eq!(list["total"], 3);
        assert_eq!(list["messages"][0]["id"], "three");
        assert_eq!(list["messages"].as_array().unwrap().len(), 2);
        assert_eq!(
            super::query_messages(&c, "a", "INBOX", "", 2, 2).unwrap()["messages"][0]["id"],
            "one"
        );
        for (search, id) in [("%", "one"), ("_", "two"), ("\\", "three")] {
            let result = super::query_messages(&c, "a", "INBOX", search, 200, 0).unwrap();
            assert_eq!(result["total"], 1);
            assert_eq!(result["messages"][0]["id"], id);
        }
        for search in ["sender", "项目摘要"] {
            assert_eq!(
                super::query_messages(&c, "a", "INBOX", search, 200, 0).unwrap()["total"],
                3
            );
        }
        assert_eq!(
            super::query_messages(&c, "a", "INBOX", "不存在", 200, 0).unwrap()["total"],
            0
        );
    }
    use super::*;
    use std::io::Cursor;

    fn test_account() -> Account {
        Account {
            id: uuid::Uuid::now_v7().to_string(),
            revision: 1,
            name: "测试邮箱".into(),
            email: "sender@example.invalid".into(),
            sender_name: "测试发件人".into(),
            imap_host: "imap.example.invalid".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            username: "sender@example.invalid".into(),
            smtp_host: "smtp.example.invalid".into(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            smtp_username: "sender@example.invalid".into(),
            enabled: true,
            sync_interval_seconds: 60,
            has_imap_credential: false,
            has_smtp_credential: false,
            last_sync: None,
            last_error: None,
        }
    }
    fn test_draft(a: &Account) -> Draft {
        Draft {
            id: uuid::Uuid::now_v7().to_string(),
            account_id: a.id.clone(),
            to: vec!["recipient@example.invalid".into()],
            cc: vec![],
            bcc: vec!["private@example.invalid".into()],
            subject: "测试主题".into(),
            text: "仅本地生成，不发送".into(),
            attachments: vec![],
            reply_to_message_id: None,
            updated_at: Some(now()),
        }
    }
    struct Wire {
        read: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }
    impl Read for Wire {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.read.read(b)
        }
    }
    impl Write for Wire {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn wire(responses: &[u8]) -> imap::Session<Wire> {
        imap::Client::new(Wire {
            read: Cursor::new(responses.to_vec()),
            written: Vec::new(),
        })
        .login("fixture", "not-a-secret")
        .map_err(|_| ())
        .unwrap()
    }

    #[test]
    fn account_rejects_plaintext_protocol_headers_and_secrets() {
        let mut a = test_account();
        assert!(validate_account(&a).is_ok());
        a.imap_security = "none".into();
        assert!(validate_account(&a).is_err());
        a.imap_security = "starttls".into();
        a.smtp_host = "https://smtp.example.invalid".into();
        assert!(validate_account(&a).is_err());
        a.smtp_host = "smtp.example.invalid".into();
        a.sender_name = "name\r\nBcc: attacker@example.invalid".into();
        assert!(validate_account(&a).is_err());
        let mut value = json!(test_account());
        value["password"] = json!("fixture-only");
        assert!(serde_json::from_value::<Account>(value).is_err());
    }
    #[test]
    fn imap_folder_counts_and_special_use_are_parsed() {
        let mut s=wire(b"a1 OK login\r\n* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\Sent) \"/\" \"Sent Items\"\r\n* LIST (\\Noselect) \"/\" \"Parent\"\r\na2 OK list\r\n* STATUS INBOX (MESSAGES 231 UNSEEN 8)\r\na3 OK status\r\n* STATUS \"Sent Items\" (MESSAGES 4 UNSEEN 0)\r\na4 OK status\r\n");
        let folders = folders_remote(&mut s).unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0]["unread"], 8);
        assert_eq!(folders[0]["total"], 231);
        assert_eq!(folders[1]["kind"], "sent");
        assert_eq!(decode_folder_name("&XfJT0ZAB-"), "已发送");
        assert_eq!(decode_folder_name("A&-B"), "A&B");
    }
    #[test]
    fn remote_errors_do_not_expose_server_details() {
        let mut s = wire(b"a1 OK login\r\na2 NO private-auth-data\r\n");
        assert_eq!(folders_remote(&mut s).unwrap_err(), "无法读取邮箱文件夹");
    }
    #[test]
    fn mime_decodes_headers_body_and_attachment() {
        let raw=b"From: Example <sender@example.invalid>\r\nTo: reader@example.invalid\r\nSubject: =?UTF-8?B?5rWL6K+V?=\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nhello\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=sample.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--b--\r\n";
        let (p, text, _, files) = decode(raw).unwrap();
        assert_eq!(p.headers.get_first_value("Subject").unwrap(), "测试");
        assert!(text.contains("hello"));
        assert_eq!(files[0].0["name"], "sample.txt");
        assert_eq!(files[0].1, b"hello");
        assert_eq!(addresses(&p, "To"), vec!["reader@example.invalid"]);
        assert!(decode(&vec![b'x'; MAX_MAIL + 1]).is_err());
    }
    #[test]
    fn imap_uid_and_flags_match_mime_cache_identity() {
        let raw=b"From: sender@example.invalid\r\nTo: reader@example.invalid\r\nSubject: testing\r\nDate: Tue, 08 Sep 2026 10:00:00 +0800\r\n\r\nbody";
        let mut response = format!(
            "a1 OK login\r\n* 1 FETCH (UID 42 FLAGS (\\Seen \\Flagged) BODY[] {{{}}}\r\n",
            raw.len()
        )
        .into_bytes();
        response.extend_from_slice(raw);
        response.extend_from_slice(b")\r\na2 OK fetch\r\n");
        let mut s = wire(&response);
        let f = s.uid_fetch("42", "(UID FLAGS BODY.PEEK[])").unwrap();
        let a = message_value("account", "INBOX", 3, &f[0], raw).unwrap();
        let b = message_value("account", "INBOX", 4, &f[0], raw).unwrap();
        assert_eq!(a["uid"], 42);
        assert_eq!(a["seen"], true);
        assert_eq!(a["flagged"], true);
        assert_ne!(a["id"], b["id"]);
        assert_eq!(a["to"], json!(["reader@example.invalid"]));
    }
    #[test]
    fn smtp_message_preserves_bcc_envelope_without_leaking_header() {
        let a = test_account();
        let ctx = Context {
            account: a.clone(),
            workspace: "fixture".into(),
            generation: PathBuf::new(),
        };
        let d = test_draft(&a);
        let message = build_message(&ctx, &d).unwrap();
        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(!raw.contains("private@example.invalid"));
        assert!(message
            .envelope()
            .to()
            .iter()
            .any(|a| a.to_string() == "private@example.invalid"));
        let mut invalid = d.clone();
        invalid.to = vec!["bad\r\nBcc: inject@example.invalid".into()];
        assert!(validate_draft(&invalid).is_err());
    }
    #[test]
    fn mailbox_cache_and_sent_guard_survive_backup_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path().join("source")).unwrap();
        let a = test_account();
        let d = test_draft(&a);
        {
            let c = store.conn.lock().unwrap();
            assert_eq!(
                c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                4
            );
            c.execute(
                "INSERT INTO mail_accounts VALUES(?1,?2,?3)",
                params![a.id, a.revision, json!(a).to_string()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO mail_drafts VALUES(?1,?2,'sent',?3)",
                params![d.id, a.id, json!(d).to_string()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO mail_messages VALUES('m',?1,'INBOX',7,12,0,'{}',?2)",
                params![a.id, b"raw body".as_slice()],
            )
            .unwrap();
        }
        let archive = tmp.path().join("backup.zip");
        store.backup(&archive).unwrap();
        let restored = Store::restore_to(&archive, &tmp.path().join("restored")).unwrap();
        let c = restored.conn.lock().unwrap();
        assert_eq!(account(&c, &a.id).unwrap().name, "测试邮箱");
        assert_eq!(
            c.query_row("SELECT raw FROM mail_messages WHERE id='m'", [], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            b"raw body"
        );
        assert_eq!(
            c.execute(
                "UPDATE mail_drafts SET state='sending' WHERE id=?1 AND state='draft'",
                [&d.id]
            )
            .unwrap(),
            0
        );
        c.execute("DELETE FROM mail_accounts WHERE id=?1", [&a.id])
            .unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM mail_messages", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn account_operation_lock_prevents_duplicate_send_and_releases() {
        let key = uuid::Uuid::now_v7().to_string();
        let guard = Busy::acquire(key.clone()).unwrap();
        assert!(Busy::acquire(key.clone()).is_err());
        drop(guard);
        assert!(Busy::acquire(key).is_ok());
    }
}
