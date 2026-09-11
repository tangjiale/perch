//! 钉钉 CalDAV 只读镜像；先完整读取，再以配置版本和数据代校验原子替换缓存。
use crate::{integrations, storage::Store, AppState};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::PathBuf, sync::OnceLock, time::Instant};
use tauri::{Emitter, Manager, State};
use url::Url;
const KEY: &str = "dingtalk-caldav";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENTS: usize = 10000;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    server_url: String,
    username: String,
    calendar_url: String,
    calendar_name: String,
    todo_calendar_url: String,
    todo_calendar_name: String,
    enabled: bool,
    sync_interval_minutes: u64,
    revision: u64,
    has_credential: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            username: String::new(),
            calendar_url: String::new(),
            calendar_name: String::new(),
            todo_calendar_url: String::new(),
            todo_calendar_name: String::new(),
            enabled: false,
            sync_interval_minutes: 5,
            revision: 0,
            has_credential: false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Calendar {
    url: String,
    name: String,
    #[serde(default)]
    components: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    id: String,
    title: String,
    start: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<String>,
    all_day: bool,
    description: String,
    location: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    id: String,
    title: String,
    description: String,
    location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<String>,
    all_day: bool,
    status: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    config: Config,
    calendars: Vec<Calendar>,
    events: Vec<Event>,
    todos: Vec<Todo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    range_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    range_end: Option<String>,
}
// 按目标站点与账号隔离凭据，配置提交失败时也不会把新密码发往旧服务器。
fn credential_id(config: &Config) -> String {
    let mut digest = Sha256::new();
    digest.update(config.server_url.as_bytes());
    digest.update([0]);
    digest.update(config.username.as_bytes());
    format!("{KEY}:{}", hex::encode(digest.finalize()))
}
fn read(store: &Store) -> Result<Snapshot, String> {
    let conn = store.conn.lock().map_err(|_| "日历存储不可用")?;
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| "读取日历配置失败")?;
    value
        .map(|s| serde_json::from_str(&s).map_err(|_| "日历缓存格式无效".into()))
        .unwrap_or_else(|| Ok(Snapshot::default()))
}
fn write(store: &Store, snapshot: &Snapshot) -> Result<(), String> {
    store.conn.lock().map_err(|_|"日历存储不可用")?.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,serde_json::to_string(snapshot).map_err(|_|"日历缓存序列化失败")?]).map_err(|_|"保存日历缓存失败")?;
    Ok(())
}
fn address(value: &str) -> Result<Url, String> {
    let s = value.trim();
    let s = if s.contains("://") {
        s.to_owned()
    } else {
        format!("https://{s}")
    };
    let url = Url::parse(&s).map_err(|_| "CalDAV 地址无效")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err("CalDAV 地址须为不含账号、查询参数或片段的 HTTPS 地址".into());
    }
    Ok(url)
}
fn same_origin(base: &Url, value: &str) -> Result<Url, String> {
    let target = base.join(value).map_err(|_| "日历发现地址无效")?;
    if target.origin() != base.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
    {
        return Err("日历服务器返回了不安全的跨站地址".into());
    }
    Ok(target)
}
#[tauri::command]
pub fn dingtalk_calendar_state(state: State<AppState>) -> Result<Snapshot, String> {
    read(&*state.store.lock().map_err(|_| "工作空间不可用")?)
}
#[tauri::command]
pub fn dingtalk_calendar_save(
    state: State<AppState>,
    config: Config,
    password: Option<String>,
) -> Result<Snapshot, String> {
    save_config(&state, config, password)
}
fn save_config(
    state: &AppState,
    mut config: Config,
    password: Option<String>,
) -> Result<Snapshot, String> {
    // 配置和凭据作为一组更新，防止同步向旧地址发送刚保存的新密码。
    let _guard = sync_lock()
        .try_lock()
        .map_err(|_| "日历正在同步，请完成后再修改配置")?;
    if !matches!(config.sync_interval_minutes, 0 | 1 | 5 | 10 | 15 | 30 | 60) {
        return Err("同步间隔请选择关闭、1、5、10、15、30 或 60 分钟".into());
    }
    config.username = config.username.trim().to_owned();
    if config.username.len() > 1024
        || config.calendar_name.len() > 1024
        || config.todo_calendar_name.len() > 1024
    {
        return Err("日历配置过长".into());
    }
    if !config.server_url.trim().is_empty() {
        let base = address(&config.server_url)?;
        config.server_url = base.to_string();
        if !config.calendar_url.is_empty() {
            config.calendar_url = same_origin(&base, &config.calendar_url)?.to_string();
        }
        if !config.todo_calendar_url.is_empty() {
            config.todo_calendar_url = same_origin(&base, &config.todo_calendar_url)?.to_string();
        }
    } else if !config.calendar_url.is_empty()
        || !config.todo_calendar_url.is_empty()
        || config.enabled
    {
        return Err("请填写 CalDAV 服务器地址".into());
    }
    if config.enabled && config.username.is_empty() {
        return Err("启用同步前请填写账号".into());
    }
    if password
        .as_ref()
        .is_some_and(|p| p.is_empty() || p.len() > 16384)
    {
        return Err("CalDAV 专用密码不能为空或过长".into());
    }
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    let old = read(&store)?;
    if old.config.revision != config.revision {
        return Err("日历配置已更新，请刷新后重试".into());
    }
    let changed =
        old.config.server_url != config.server_url || old.config.username != config.username;
    if config.enabled && password.is_none() && (changed || !old.config.has_credential) {
        return Err("请先保存 CalDAV 专用密码".into());
    }
    // 凭据与配置无法跨系统提交，先保存旧值用于 SQLite 写入失败时恢复。
    let secret_changed = password.is_some() || (changed && old.config.has_credential);
    let entry = integrations::credential(&store.workspace_id, &credential_id(&config))?;
    let previous = if secret_changed {
        match entry.get_password() {
            Ok(secret) => Some(secret),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => return Err("无法读取原 CalDAV 凭据，未修改配置".into()),
        }
    } else {
        None
    };
    if let Some(secret) = password {
        integrations::credential(&store.workspace_id, &credential_id(&config))?
            .set_password(&secret)
            .map_err(|_| "无法保存 CalDAV 密码到系统钥匙串")?;
        config.has_credential = true;
    } else if changed && old.config.has_credential {
        match integrations::credential(&store.workspace_id, &credential_id(&config))?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err("无法清理旧 CalDAV 凭据".into()),
        }
        config.has_credential = false;
    } else {
        config.has_credential = !changed && old.config.has_credential;
    }
    if config.enabled && !config.has_credential {
        return Err("请先保存 CalDAV 专用密码".into());
    }
    config.revision += 1;
    let event_changed = old.config.calendar_url != config.calendar_url;
    let todo_changed = old.config.todo_calendar_url != config.todo_calendar_url;
    let mut next = if changed { Snapshot::default() } else { old };
    if event_changed {
        next.events.clear();
        next.range_start = None;
        next.range_end = None;
    }
    if todo_changed {
        next.todos.clear();
    }
    if event_changed || todo_changed {
        next.last_sync = None;
    }
    next.config = config;
    next.last_error = None;
    if let Err(error) = write(&store, &next) {
        if secret_changed {
            let restored = match previous {
                Some(secret) => entry.set_password(&secret),
                None => entry.delete_credential(),
            };
            if !matches!(restored, Ok(()) | Err(keyring::Error::NoEntry)) {
                return Err(
                    "日历配置保存失败，且无法恢复原凭据；请先停用日历同步并重新配置密码".into(),
                );
            }
        }
        return Err(error);
    }
    Ok(next)
}
struct Dav {
    client: reqwest::Client,
    base: Url,
    username: String,
    password: String,
}
impl Dav {
    async fn request(
        &self,
        url: &Url,
        method: &str,
        depth: &str,
        body: &str,
    ) -> Result<String, String> {
        let mut target = same_origin(&self.base, url.as_str())?;
        // RFC 6764：只有裸主机入口需要先发现 CalDAV 服务，保留显式配置路径。
        let discovery =
            method == "PROPFIND" && depth == "0" && url == &self.base && url.path() == "/";
        if discovery {
            target = self
                .base
                .join("/.well-known/caldav")
                .map_err(|_| "日历发现地址无效")?;
        }
        let mut visited = HashSet::new();
        let mut fallback = false;
        let mut response = loop {
            if !visited.insert(target.clone()) || visited.len() > 6 {
                return Err("CalDAV 服务地址跳转循环或次数过多".into());
            }
            let response = self
                .client
                .request(
                    reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "请求方法无效")?,
                    target.clone(),
                )
                .basic_auth(&self.username, Some(&self.password))
                .header("Depth", depth)
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(body.to_owned())
                .send()
                .await
                .map_err(|_| "CalDAV 网络连接失败或超时")?;
            let status = response.status().as_u16();
            if discovery
                && !fallback
                && target.path() == "/.well-known/caldav"
                && matches!(status, 404 | 405)
            {
                fallback = true;
                target = self.base.clone();
                continue;
            }
            if matches!(status, 301 | 302 | 307 | 308) {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or("CalDAV 跳转缺少有效目标地址")?;
                let next = target.join(location).map_err(|_| "CalDAV 跳转地址无效")?;
                // 手动处理跳转，保留 PROPFIND/REPORT 与正文；绝不把凭据发到另一站点。
                target = same_origin(&self.base, next.as_str())?;
                continue;
            }
            if status != 207 {
                return Err(match status {
                    401 => "CalDAV 账号或专用密码无效（HTTP 401）".into(),
                    403 => "CalDAV 账号无权读取该日历（HTTP 403）".into(),
                    404 => "未找到 CalDAV 服务入口（HTTP 404），请核对服务器地址".into(),
                    405 => "服务器入口不支持 CalDAV 查询（HTTP 405），请核对服务路径".into(),
                    507 => "日历结果超过服务器上限（HTTP 507），缓存保持不变".into(),
                    _ => format!(
                        "CalDAV {method} 请求返回 HTTP {status}，未收到日历响应；请核对服务入口"
                    ),
                });
            }
            break response;
        };
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "读取 CalDAV 响应失败")? {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err("日历响应超过 16 MiB 安全上限，缓存保持不变".into());
            }
            bytes.extend_from_slice(&chunk)
        }
        String::from_utf8(bytes).map_err(|_| "CalDAV 响应不是有效 UTF-8".into())
    }
    async fn prop(&self, url: &Url, depth: &str) -> Result<String, String> {
        self.request(url,"PROPFIND",depth,r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:current-user-principal/><c:calendar-home-set/><d:resourcetype/><d:displayname/><c:supported-calendar-component-set/></d:prop></d:propfind>"#).await
    }
    async fn discover(&self) -> Result<Vec<Calendar>, String> {
        let first = self.prop(&self.base, "0").await?;
        let mut home = href_prop(&first, "calendar-home-set")?;
        if home.is_none() {
            let principal = href_prop(&first, "current-user-principal")?
                .ok_or("服务器没有返回当前账号的日历身份")?;
            let xml = self
                .prop(&same_origin(&self.base, &principal)?, "0")
                .await?;
            home = href_prop(&xml, "calendar-home-set")?;
        }
        let home = same_origin(&self.base, &home.ok_or("服务器没有返回当前账号的日历目录")?)?;
        let xml = self.prop(&home, "1").await?;
        let doc = parse_xml(&xml)?;
        let mut calendars = Vec::new();
        let mut seen = HashSet::new();
        for response in responses(&doc) {
            check_response(response)?;
            if !response
                .descendants()
                .any(|n| n.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar")))
            {
                continue;
            }
            let supported: Vec<_> = response
                .descendants()
                .filter(|n| n.has_tag_name(("urn:ietf:params:xml:ns:caldav", "comp")))
                .filter_map(|n| n.attribute("name").map(str::to_owned))
                .collect();
            if !supported.is_empty() && !supported.iter().any(|n| n == "VEVENT" || n == "VTODO") {
                continue;
            }
            let href = response
                .children()
                .find(|n| n.has_tag_name(("DAV:", "href")))
                .and_then(|n| n.text())
                .ok_or("日历缺少地址")?;
            let url = same_origin(&self.base, href)?.to_string();
            let name = response
                .descendants()
                .find(|n| n.has_tag_name(("DAV:", "displayname")))
                .and_then(|n| n.text())
                .unwrap_or("未命名日历")
                .to_string();
            if seen.insert(url.clone()) {
                calendars.push(Calendar {
                    url,
                    name,
                    components: supported,
                })
            }
            if calendars.len() > 100 {
                return Err("日历数量超过 100 项上限".into());
            }
        }
        Ok(calendars)
    }
    async fn refresh(&self, old: &Snapshot) -> Result<Snapshot, String> {
        let mut next = old.clone();
        next.calendars = self.discover().await?;
        if !old.config.calendar_url.is_empty() {
            let chosen = next
                .calendars
                .iter()
                .find(|c| {
                    c.url == old.config.calendar_url
                        && (c.components.is_empty() || c.components.iter().any(|v| v == "VEVENT"))
                })
                .ok_or("所选日历不在当前账号日历列表中，请重新选择")?;
            next.config.calendar_name = chosen.name.clone();
            let now = Utc::now();
            let start = now - Duration::days(90);
            let end = now + Duration::days(365);
            next.events = self.events(&chosen.url, start, end).await?;
            next.range_start = Some(start.to_rfc3339());
            next.range_end = Some(end.to_rfc3339());
        }
        if !old.config.todo_calendar_url.is_empty() {
            let chosen = next
                .calendars
                .iter()
                .find(|c| {
                    c.url == old.config.todo_calendar_url
                        && c.components.iter().any(|v| v == "VTODO")
                })
                .ok_or("所选待办目录不在当前账号支持 VTODO 的列表中，请重新选择")?;
            next.config.todo_calendar_name = chosen.name.clone();
            next.todos = self.todos(&chosen.url).await?;
        }
        Ok(next)
    }
    async fn todos(&self, calendar: &str) -> Result<Vec<Todo>, String> {
        // 不加时间范围，保留未排期待办；重复规则不能借用仅适用于事件的 expand。
        let body = r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VTODO"/></c:comp-filter></c:filter></c:calendar-query>"#;
        let xml = self
            .request(&same_origin(&self.base, calendar)?, "REPORT", "1", body)
            .await?;
        let doc = parse_xml(&xml)?;
        let mut todos = Vec::new();
        let mut ids = HashSet::new();
        for response in responses(&doc) {
            check_response(response)?;
            let data = response
                .descendants()
                .find(|n| n.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar-data")))
                .and_then(|n| n.text())
                .ok_or("服务器未返回完整待办内容")?;
            for todo in parse_todos(data)? {
                if !ids.insert(todo.id.clone()) {
                    return Err("服务器返回重复待办，无法确认结果完整".into());
                }
                todos.push(todo);
                if todos.len() > MAX_EVENTS {
                    return Err("待办超过 10000 项上限，缓存保持不变".into());
                }
            }
        }
        todos.sort_by(|a, b| a.start.cmp(&b.start).then(a.id.cmp(&b.id)));
        Ok(todos)
    }
    async fn events(
        &self,
        calendar: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Event>, String> {
        let a = start.format("%Y%m%dT%H%M%SZ");
        let b = end.format("%Y%m%dT%H%M%SZ");
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data><c:expand start="{a}" end="{b}"/></c:calendar-data></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{a}" end="{b}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
        );
        let xml = self
            .request(&same_origin(&self.base, calendar)?, "REPORT", "1", &body)
            .await?;
        let doc = parse_xml(&xml)?;
        let mut events = Vec::new();
        let mut ids = HashSet::new();
        for response in responses(&doc) {
            check_response(response)?;
            let data = response
                .descendants()
                .find(|n| n.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar-data")))
                .and_then(|n| n.text())
                .ok_or("服务器未返回完整日历内容")?;
            for event in parse_ical(data)? {
                if !ids.insert(event.id.clone()) {
                    return Err("服务器返回重复日程实例，无法确认结果完整".into());
                }
                events.push(event);
                if events.len() > MAX_EVENTS {
                    return Err("日程超过 10000 项上限，缓存保持不变".into());
                }
            }
        }
        events.sort_by(|a, b| a.start.cmp(&b.start).then(a.id.cmp(&b.id)));
        Ok(events)
    }
}
fn parse_xml(s: &str) -> Result<roxmltree::Document<'_>, String> {
    let doc = roxmltree::Document::parse_with_options(
        s,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 200000,
        },
    )
    .map_err(|_| "CalDAV XML 无效或超过安全上限")?;
    if !doc.root_element().has_tag_name(("DAV:", "multistatus"))
        || doc.descendants().any(|n| {
            n.is_element()
                && [
                    "number-of-matches-within-limits",
                    "limit",
                    "next-page",
                    "next",
                ]
                .contains(&n.tag_name().name())
        })
    {
        return Err("CalDAV 未返回完整结果或要求不支持的分页，缓存保持不变".into());
    }
    Ok(doc)
}
fn responses<'a>(
    doc: &'a roxmltree::Document<'a>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    doc.descendants()
        .filter(|n| n.has_tag_name(("DAV:", "response")))
}
fn check_response(node: roxmltree::Node<'_, '_>) -> Result<(), String> {
    if node
        .descendants()
        .any(|n| n.has_tag_name(("DAV:", "error")))
    {
        return Err("服务器返回不完整日历结果，缓存保持不变".into());
    }
    for propstat in node
        .children()
        .filter(|n| n.has_tag_name(("DAV:", "propstat")))
    {
        let code = propstat
            .children()
            .find(|n| n.has_tag_name(("DAV:", "status")))
            .and_then(|n| n.text())
            .and_then(|s| s.split_whitespace().nth(1));
        // PROPFIND 可对未实现的可选属性返回 404；权限错误与缺失日历内容不能当作空结果。
        let required = propstat.descendants().any(|n| {
            n.has_tag_name(("urn:ietf:params:xml:ns:caldav", "calendar-data"))
                || n.has_tag_name(("DAV:", "resourcetype"))
        });
        if code != Some("200") && (code != Some("404") || required) {
            return Err("部分日历内容读取失败，缓存保持不变".into());
        }
    }
    for status in node
        .children()
        .filter(|n| n.has_tag_name(("DAV:", "status")))
    {
        if !status
            .text()
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .is_some_and(|s| s.starts_with('2'))
        {
            return Err("部分日历资源读取失败，缓存保持不变".into());
        }
    }
    Ok(())
}
fn href_prop(xml: &str, name: &str) -> Result<Option<String>, String> {
    let doc = parse_xml(xml)?;
    for response in responses(&doc) {
        check_response(response)?;
    }
    Ok(doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .and_then(|n| n.descendants().find(|c| c.has_tag_name(("DAV:", "href"))))
        .and_then(|n| n.text())
        .map(str::to_owned))
}
fn decoded(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}
fn date(prop: &ical::property::Property) -> Result<(String, bool), String> {
    let raw = prop.value.as_deref().ok_or("日程时间为空")?;
    if raw.len() == 8 {
        return NaiveDate::parse_from_str(raw, "%Y%m%d")
            .map(|d| (d.format("%Y-%m-%d").to_string(), true))
            .map_err(|_| "全天日程日期无效".into());
    }
    let naive = NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
        .map_err(|_| "日程时间格式无效")?;
    if raw.ends_with('Z') {
        return Ok((naive.and_utc().to_rfc3339(), false));
    }
    let tz = prop
        .params
        .as_ref()
        .and_then(|p| p.iter().find(|(k, _)| k == "TZID"))
        .and_then(|(_, v)| v.first())
        .ok_or("服务器未展开浮动时区，无法可靠同步该日程")?;
    let zone: chrono_tz::Tz = tz
        .trim_matches('"')
        .parse()
        .map_err(|_| "日程使用不支持的时区，请检查服务器展开支持")?;
    let d = zone
        .from_local_datetime(&naive)
        .single()
        .ok_or("日程时间存在夏令时歧义")?;
    Ok((d.with_timezone(&Utc).to_rfc3339(), false))
}
fn parse_duration(raw: &str) -> Result<i64, String> {
    let raw = raw.strip_prefix('+').unwrap_or(raw);
    let rest = raw.strip_prefix('P').ok_or("日程持续时间格式无效")?;
    let mut time = false;
    let mut digits = String::new();
    let mut total = 0i64;
    let mut any = false;
    for c in rest.chars() {
        if c == 'T' && !time && digits.is_empty() {
            time = true;
            continue;
        }
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let factor = match (time, c) {
            (false, 'W') => 604800,
            (false, 'D') => 86400,
            (true, 'H') => 3600,
            (true, 'M') => 60,
            (true, 'S') => 1,
            _ => return Err("日程持续时间格式无效".into()),
        };
        let n = digits.parse::<i64>().map_err(|_| "日程持续时间格式无效")?;
        digits.clear();
        any = true;
        total = total
            .checked_add(n.checked_mul(factor).ok_or("日程持续时间过长")?)
            .ok_or("日程持续时间过长")?;
    }
    if !any || !digits.is_empty() || total > 366 * 86400 * 100 {
        return Err("日程持续时间无效或过长".into());
    }
    Ok(total)
}
fn parse_ical(data: &str) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    let mut count = 0;
    for calendar in ical::IcalParser::new(std::io::BufReader::new(data.as_bytes())) {
        count += 1;
        let calendar = calendar.map_err(|_| "日历 iCalendar 内容无效")?;
        for item in calendar.events {
            let prop = |name: &str| item.properties.iter().find(|p| p.name == name);
            let value = |name: &str| prop(name).and_then(|p| p.value.as_deref()).unwrap_or("");
            if ["RRULE", "RDATE", "EXRULE", "EXDATE"]
                .iter()
                .any(|name| prop(name).is_some())
            {
                return Err("服务器未展开重复日程；本次同步未覆盖缓存，请确认 CalDAV 支持 calendar-data expand".into());
            }
            if value("STATUS") == "CANCELLED" {
                continue;
            }
            let uid = value("UID");
            if uid.is_empty() {
                return Err("日程缺少唯一标识".into());
            }
            let (start, all_day) = date(prop("DTSTART").ok_or("日程缺少开始时间")?)?;
            let ending = prop("DTEND").map(date).transpose()?;
            if ending
                .as_ref()
                .is_some_and(|(_, is_date)| *is_date != all_day)
            {
                return Err("日程开始与结束的日期类型不一致".into());
            }
            let mut end = ending.map(|v| v.0);
            if end.is_none() {
                let seconds = if let Some(duration) = prop("DURATION") {
                    parse_duration(duration.value.as_deref().ok_or("日程持续时间为空")?)?
                } else if all_day {
                    86400
                } else {
                    0
                };
                if all_day {
                    if seconds % 86400 != 0 {
                        return Err("全天日程持续时间不是整天".into());
                    }
                    let date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                        .map_err(|_| "日程日期无效")?;
                    end = Some(
                        date.checked_add_signed(Duration::seconds(seconds))
                            .ok_or("日程结束日期超出范围")?
                            .format("%Y-%m-%d")
                            .to_string(),
                    );
                } else {
                    end = Some(
                        DateTime::parse_from_rfc3339(&start)
                            .map_err(|_| "日程时间无效")?
                            .checked_add_signed(Duration::seconds(seconds))
                            .ok_or("日程结束时间超出范围")?
                            .to_rfc3339(),
                    );
                }
            }
            if end.as_ref().is_some_and(|e| e < &start) {
                return Err("日程结束时间早于开始时间".into());
            }
            let recurrence = value("RECURRENCE-ID");
            let id = format!(
                "{}:{}:{}",
                uid.len(),
                uid,
                if recurrence.is_empty() {
                    &start
                } else {
                    recurrence
                }
            );
            events.push(Event {
                id,
                title: decoded(value("SUMMARY")),
                start,
                end,
                all_day,
                description: decoded(value("DESCRIPTION")),
                location: decoded(value("LOCATION")),
            });
            if events.len() > MAX_EVENTS {
                return Err("日程超过安全上限".into());
            }
        }
    }
    if count == 0 {
        return Err("日历内容为空或格式无效".into());
    }
    Ok(events)
}
fn parse_todos(data: &str) -> Result<Vec<Todo>, String> {
    let mut todos = Vec::new();
    let mut count = 0;
    for calendar in ical::IcalParser::new(std::io::BufReader::new(data.as_bytes())) {
        count += 1;
        let calendar = calendar.map_err(|_| "待办 iCalendar 内容无效")?;
        for item in calendar.todos {
            let prop = |name: &str| item.properties.iter().find(|p| p.name == name);
            let value = |name: &str| prop(name).and_then(|p| p.value.as_deref()).unwrap_or("");
            if ["RRULE", "RDATE", "EXRULE", "EXDATE", "RECURRENCE-ID"]
                .iter()
                .any(|n| prop(n).is_some())
            {
                return Err("待办包含未展开重复规则或实例，暂不支持；本次同步保留原缓存".into());
            }
            let uid = value("UID");
            if uid.is_empty() {
                return Err("待办缺少 UID".into());
            }
            let begin = prop("DTSTART").map(date).transpose()?;
            let due = prop("DUE").map(date).transpose()?;
            if begin
                .as_ref()
                .zip(due.as_ref())
                .is_some_and(|(a, b)| a.1 != b.1 || b.0 < a.0)
            {
                return Err("待办起止日期类型不同或截止早于开始".into());
            }
            if prop("DURATION").is_some() && (begin.is_none() || due.is_some()) {
                return Err("待办持续时间要求开始日期且不能同时包含截止日期".into());
            }
            let start = begin.as_ref().or(due.as_ref()).map(|v| v.0.clone());
            let all_day = begin.as_ref().or(due.as_ref()).is_some_and(|v| v.1);
            let mut end = due.map(|v| v.0);
            if let Some(duration) = prop("DURATION") {
                let seconds = parse_duration(duration.value.as_deref().unwrap_or(""))?;
                let start = start.as_ref().ok_or("待办缺少开始日期")?;
                end = Some(if all_day {
                    if seconds % 86400 != 0 {
                        return Err("全天待办持续时间不是整天".into());
                    }
                    NaiveDate::parse_from_str(start, "%Y-%m-%d")
                        .map_err(|_| "待办日期无效")?
                        .checked_add_signed(Duration::seconds(seconds))
                        .ok_or("待办结束日期超出范围")?
                        .to_string()
                } else {
                    DateTime::parse_from_rfc3339(start)
                        .map_err(|_| "待办时间无效")?
                        .checked_add_signed(Duration::seconds(seconds))
                        .ok_or("待办结束时间超出范围")?
                        .to_rfc3339()
                });
            } else if all_day {
                // DUE 是包含当天的截止日期，前端采用排他的结束日期。
                end = Some(
                    NaiveDate::parse_from_str(
                        end.as_ref().or(start.as_ref()).ok_or("待办日期缺失")?,
                        "%Y-%m-%d",
                    )
                    .map_err(|_| "待办日期无效")?
                    .succ_opt()
                    .ok_or("待办结束日期超出范围")?
                    .to_string(),
                );
            }
            let status = match value("STATUS") {
                "CANCELLED" => "closed",
                "COMPLETED" => "done",
                _ if prop("COMPLETED").is_some() => "done",
                "IN-PROCESS" => "doing",
                "" | "NEEDS-ACTION" => "todo",
                _ => return Err("待办状态无法识别".into()),
            };
            todos.push(Todo {
                id: format!("todo:{}:{}", uid.len(), uid),
                title: decoded(value("SUMMARY")),
                description: decoded(value("DESCRIPTION")),
                location: decoded(value("LOCATION")),
                start,
                end,
                all_day,
                status: status.into(),
            });
            if todos.len() > MAX_EVENTS {
                return Err("待办超过安全上限".into());
            }
        }
    }
    if count == 0 {
        return Err("待办内容为空或格式无效".into());
    }
    Ok(todos)
}
fn sync_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}
async fn sync_inner(
    state: &AppState,
    expected: Option<(String, PathBuf, u64)>,
) -> Result<Snapshot, String> {
    let _guard = sync_lock()
        .try_lock()
        .map_err(|_| "日历正在同步，请稍后重试")?;
    let (old, workspace, generation) = {
        let s = state.store.lock().map_err(|_| "工作空间不可用")?;
        let old = read(&s)?;
        if expected.as_ref().is_some_and(|(w, g, r)| {
            w != &s.workspace_id || g != &s.generation || *r != old.config.revision
        }) {
            return Err("工作空间或日历配置已切换".into());
        }
        (old, s.workspace_id.clone(), s.generation.clone())
    };
    let result = async {
        let base = address(&old.config.server_url)?;
        if old.config.username.is_empty() {
            return Err("请填写 CalDAV 用户名".into());
        }
        let password = integrations::read_secret(&workspace, &credential_id(&old.config))?;
        if password.is_empty() {
            return Err("请保存 CalDAV 专用密码".into());
        }
        let dav = Dav {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|_| "无法初始化 CalDAV 客户端")?,
            base,
            username: old.config.username.clone(),
            password,
        };
        let mut next = dav.refresh(&old).await?;
        next.last_error = None;
        next.last_sync = Some(Utc::now().to_rfc3339());
        Ok::<_, String>(next)
    }
    .await;
    let s = state.store.lock().map_err(|_| "工作空间不可用")?;
    if s.workspace_id != workspace
        || s.generation != generation
        || read(&s)?.config.revision != old.config.revision
    {
        return Err("工作空间或日历配置已切换，本次结果已丢弃".into());
    }
    match result {
        Ok(next) => {
            write(&s, &next)?;
            Ok(next)
        }
        Err(error) => {
            let mut next = old;
            next.last_error = Some(error.clone());
            write(&s, &next)?;
            Err(error)
        }
    }
}
#[tauri::command]
pub async fn dingtalk_calendar_sync(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Snapshot, String> {
    let result = sync_inner(&state, None).await;
    let _ = app.emit("dingtalk-calendar-updated", ());
    result
}
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut completed: Option<((String, PathBuf, u64), Instant)> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let state = app.state::<AppState>();
            let candidate = (|| {
                let store = state.store.lock().ok()?;
                let s = read(&store).ok()?;
                let c = s.config;
                if !c.enabled
                    || (c.calendar_url.is_empty() && c.todo_calendar_url.is_empty())
                    || c.sync_interval_minutes == 0
                {
                    return None;
                }
                Some((
                    (
                        store.workspace_id.clone(),
                        store.generation.clone(),
                        c.revision,
                    ),
                    c.sync_interval_minutes,
                ))
            })();
            if let Some((key, minutes)) = candidate {
                if completed
                    .as_ref()
                    .is_some_and(|(k, t)| k == &key && t.elapsed().as_secs() < minutes * 60)
                {
                    continue;
                }
                let _ = sync_inner(&state, Some(key.clone())).await;
                let _ = app.emit("dingtalk-calendar-updated", ());
                completed = Some((key, Instant::now()));
            } else {
                completed = None
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(extra: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:test\r\nDTSTART:20260911T080000Z\r\nSUMMARY:例会\\,讨论\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n")
    }
    #[test]
    fn credentials_are_bound_to_remote_identity() {
        let base = Config {
            server_url: "https://calendar.example.test/".into(),
            username: "first".into(),
            ..Config::default()
        };
        let mut other = base.clone();
        other.username = "second".into();
        assert_ne!(credential_id(&base), credential_id(&other));
        other = base.clone();
        other.server_url = "https://other.example.test/".into();
        assert_ne!(credential_id(&base), credential_id(&other));
        other = base.clone();
        other.revision += 1;
        assert_eq!(credential_id(&base), credential_id(&other));
    }
    #[test]
    fn security_rejects_cross_origin_and_embedded_credentials() {
        let base = address("calendar.example/dav/").unwrap();
        assert_eq!(base.scheme(), "https");
        for bad in [
            "http://calendar.example/dav/",
            "https://user:secret@calendar.example/",
            "https://calendar.example/?password=x",
        ] {
            assert!(address(bad).is_err());
        }
        for bad in [
            "//attacker.example/",
            "https://calendar.example:8443/a",
            "/a?secret=x",
            "https://user@calendar.example/a",
        ] {
            assert!(same_origin(&base, bad).is_err());
        }
        assert_eq!(
            same_origin(&base, "../calendar/").unwrap().path(),
            "/calendar/"
        );
    }
    #[test]
    fn recurrence_is_never_silently_lost_and_times_are_preserved() {
        assert!(parse_ical(&event("RRULE:FREQ=DAILY\r\n")).is_err());
        let parsed = parse_ical(&event(
            "DURATION:PT1H30M\r\nRECURRENCE-ID:20260911T080000Z\r\n",
        ))
        .unwrap();
        assert_eq!(parsed[0].title, "例会,讨论");
        assert_eq!(parsed[0].end.as_deref(), Some("2026-09-11T09:30:00+00:00"));
        let all_day = event("").replace("DTSTART:20260911T080000Z", "DTSTART;VALUE=DATE:20260911");
        let e = parse_ical(&all_day).unwrap();
        assert!(e[0].all_day);
        assert_eq!(e[0].end.as_deref(), Some("2026-09-12"));
        let tz = event("").replace(
            "DTSTART:20260911T080000Z",
            "DTSTART;TZID=Asia/Shanghai:20260911T080000",
        );
        assert_eq!(
            parse_ical(&tz).unwrap()[0].start,
            "2026-09-11T00:00:00+00:00"
        );
        assert!(parse_ical(&event("").replace("20260911T080000Z", "20260911T080000")).is_err());
    }
    #[test]
    fn xml_rejects_entities_partial_pages_and_property_failures() {
        assert!(
            parse_xml("<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><x>&e;</x>").is_err()
        );
        assert!(parse_xml(
            "<d:multistatus xmlns:d='DAV:'><d:next-page>/next</d:next-page></d:multistatus>"
        )
        .is_err());
        let xml="<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:propstat><d:prop><c:calendar-data/></d:prop><d:status>HTTP/1.1 403 Forbidden</d:status></d:propstat></d:response></d:multistatus>";
        let doc = parse_xml(xml).unwrap();
        assert!(check_response(responses(&doc).next().unwrap()).is_err());
    }
    #[test]
    fn settings_roundtrip_uses_temporary_database_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().join("workspace")).unwrap();
        let mut s = read(&store).unwrap();
        assert_eq!(s.config.revision, 0);
        assert_eq!(s.config.sync_interval_minutes, 5);
        assert!(!s.config.has_credential);
        s.events = parse_ical(&event("")).unwrap();
        s.config.username = "test-user".into();
        write(&store, &s).unwrap();
        let saved = read(&store).unwrap();
        assert_eq!(saved.events.len(), 1);
        assert_eq!(saved.config.username, "test-user");
        let json = serde_json::to_value(saved).unwrap();
        assert!(json.get("password").is_none());
        assert!(json["config"].get("password").is_none());
    }
    #[test]
    fn enabled_preference_survives_save_and_reopen_without_selected_calendar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        let store = Store::open_at(root.clone()).unwrap();
        let mut snapshot = Snapshot::default();
        // 仅模拟已有凭据的元数据，普通保存不读写真实钥匙串。
        snapshot.config.server_url = "https://calendar.example/".into();
        snapshot.config.username = "fixture".into();
        snapshot.config.has_credential = true;
        write(&store, &snapshot).unwrap();
        let state = AppState {
            store: std::sync::Mutex::new(store),
        };
        let mut config = snapshot.config;
        config.enabled = true;
        let saved = save_config(&state, config, None).unwrap();
        assert!(saved.config.enabled);
        assert!(saved.config.calendar_url.is_empty());
        drop(state);
        let reopened = Store::open_at(root).unwrap();
        let value = read(&reopened).unwrap();
        assert!(value.config.enabled);
        assert_eq!(value.config.revision, 1);
    }
    #[tokio::test]
    async fn failed_sync_preserves_cache_and_stale_generation_never_reads_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().join("workspace")).unwrap();
        let mut cached = Snapshot::default();
        cached.events = parse_ical(&event("")).unwrap();
        cached.config.server_url = "https://calendar.example/".into();
        write(&store, &cached).unwrap();
        let state = AppState {
            store: std::sync::Mutex::new(store),
        };
        let stale = sync_inner(&state, Some(("stale".into(), PathBuf::new(), 0)))
            .await
            .err()
            .unwrap();
        assert!(stale.contains("已切换"));
        // 缺少用户名会在读取钥匙串之前失败；验证失败路径仍保留旧日程。
        let error = sync_inner(&state, None).await.err().unwrap();
        assert!(error.contains("用户名"));
        let after = read(&state.store.lock().unwrap()).unwrap();
        assert_eq!(after.events.len(), 1);
        assert_eq!(after.last_error.as_deref(), Some("请填写 CalDAV 用户名"));
    }
    fn todo(extra: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:todo-test\r\nSUMMARY:我的待办\r\n{extra}END:VTODO\r\nEND:VCALENDAR\r\n")
    }
    #[test]
    fn todo_dates_status_recurrence_and_legacy_snapshot() {
        let legacy: Snapshot = serde_json::from_str(
            r#"{"calendars":[{"url":"https://example.com","name":"旧日历"}]}"#,
        )
        .unwrap();
        assert!(legacy.todos.is_empty());
        assert!(legacy.config.todo_calendar_url.is_empty());
        assert!(legacy.calendars[0].components.is_empty());
        let undated = parse_todos(&todo("")).unwrap();
        assert!(undated[0].start.is_none());
        assert!(undated[0].end.is_none());
        assert_eq!(undated[0].status, "todo");
        let due = parse_todos(&todo("DUE;VALUE=DATE:20260911\r\nSTATUS:IN-PROCESS\r\n")).unwrap();
        assert_eq!(due[0].start.as_deref(), Some("2026-09-11"));
        assert_eq!(due[0].end.as_deref(), Some("2026-09-12"));
        assert!(due[0].all_day);
        assert_eq!(due[0].status, "doing");
        for extra in ["STATUS:COMPLETED\r\n", "COMPLETED:20260911T080000Z\r\n"] {
            assert_eq!(parse_todos(&todo(extra)).unwrap()[0].status, "done");
        }
        assert_eq!(
            parse_todos(&todo("STATUS:CANCELLED\r\n")).unwrap()[0].status,
            "closed"
        );
        assert!(parse_todos(&todo("RRULE:FREQ=DAILY\r\n")).is_err());
        assert!(parse_todos(&todo(
            "DTSTART:20260912T080000Z\r\nDUE:20260911T080000Z\r\n"
        ))
        .is_err());
    }
    #[tokio::test]
    async fn mock_todo_report_preserves_undated_and_rejects_partial_response() {
        let xml = format!("<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:propstat><d:prop><c:calendar-data><![CDATA[{}]]></c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>", todo(""));
        let partial = xml.replace("200 OK", "403 Forbidden");
        let (url, server) = mock(vec![xml, partial]).await;
        let dav = test_dav(url.clone());
        let values = dav.todos(url.as_str()).await.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values[0].start.is_none());
        assert!(dav.todos(url.as_str()).await.is_err());
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("REPORT /dav/ HTTP/1.1"));
        assert!(requests[0].contains("name=\"VTODO\""));
        assert!(!requests[0].contains("time-range"));
        assert!(!requests[0].contains("expand"));
    }
    #[tokio::test]
    async fn two_sources_refresh_is_atomic_when_todos_fail() {
        let home = "<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:propstat><d:prop><c:calendar-home-set><d:href>/home/</d:href></c:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
        let list = "<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:href>/home/mixed/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name='VEVENT'/><c:comp name='VTODO'/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
        let empty = "<d:multistatus xmlns:d='DAV:'/>".to_owned();
        let failure =
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (url, server) = mock(vec![home.into(), list.into(), empty, failure.into()]).await;
        let mut old = Snapshot::default();
        old.config.calendar_url = url.join("/home/mixed/").unwrap().to_string();
        old.config.todo_calendar_url = old.config.calendar_url.clone();
        old.events = parse_ical(&event("")).unwrap();
        old.todos = parse_todos(&todo("")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().join("workspace")).unwrap();
        write(&store, &old).unwrap();
        assert!(test_dav(url).refresh(&old).await.is_err());
        let cached = read(&store).unwrap();
        assert_eq!(cached.events.len(), 1);
        assert_eq!(cached.todos.len(), 1);
        let requests = server.await.unwrap();
        assert!(requests[2].contains("name=\"VEVENT\""));
        assert!(requests[3].contains("name=\"VTODO\""));
    }
    #[test]
    fn todo_selection_change_clears_only_todos_and_rejects_cross_origin() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().join("workspace")).unwrap();
        let mut old = Snapshot::default();
        old.config.server_url = "https://calendar.example/".into();
        old.config.todo_calendar_url = "https://calendar.example/old/".into();
        old.todos = parse_todos(&todo("")).unwrap();
        old.events = parse_ical(&event("")).unwrap();
        write(&store, &old).unwrap();
        let state = AppState {
            store: std::sync::Mutex::new(store),
        };
        let mut config = old.config;
        config.todo_calendar_url = "https://other.example/todos/".into();
        assert!(save_config(&state, config.clone(), None).is_err());
        config.todo_calendar_url = "https://calendar.example/new/".into();
        let saved = save_config(&state, config, None).unwrap();
        assert!(saved.todos.is_empty());
        assert_eq!(saved.events.len(), 1);
    }
    #[tokio::test]
    async fn mock_discovery_keeps_todo_component() {
        let home = "<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:propstat><d:prop><c:calendar-home-set><d:href>/home/</d:href></c:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
        let list = "<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:href>/home/todos/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name='VTODO'/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
        let (url, server) = mock(vec![home.into(), list.into()]).await;
        let calendars = test_dav(url).discover().await.unwrap();
        assert_eq!(calendars.len(), 1);
        assert_eq!(calendars[0].components, ["VTODO"]);
        server.await.unwrap();
    }
    async fn mock(responses: Vec<String>) -> (Url, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/dav/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                loop {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if let Some(end) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..end]);
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(data).unwrap());
                let reply = if response.starts_with("HTTP/1.1 ") {
                    response
                } else {
                    format!("HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response)
                };
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
            requests
        });
        (url, task)
    }
    #[tokio::test]
    async fn mock_report_checks_real_method_range_expand_and_parses_instances() {
        let xml=format!("<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:href>/dav/cal/1.ics</d:href><d:propstat><d:prop><c:calendar-data><![CDATA[{}]]></c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>",event("RECURRENCE-ID:20260911T080000Z\r\n"));
        let (url, server) = mock(vec![xml]).await;
        let dav = Dav {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            base: url.clone(),
            username: "test".into(),
            password: "test-only".into(),
        };
        let a = "2026-09-01T00:00:00Z".parse().unwrap();
        let b = "2026-10-01T00:00:00Z".parse().unwrap();
        let items = dav.events(url.as_str(), a, b).await.unwrap();
        assert_eq!(items.len(), 1);
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("REPORT /dav/ HTTP/1.1"));
        assert!(
            requests[0].contains("<c:expand start=\"20260901T000000Z\" end=\"20261001T000000Z\"/>")
        );
        assert!(requests[0].contains("<c:time-range"));
    }
    #[tokio::test]
    async fn mock_discovery_follows_principal_home_without_guessing_calendar() {
        let wrap = |s: &str| {
            format!("<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:propstat><d:prop>{s}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>")
        };
        let list="<d:multistatus xmlns:d='DAV:' xmlns:c='urn:ietf:params:xml:ns:caldav'><d:response><d:href>/home/main/</d:href><d:propstat><d:prop><d:resourcetype><c:calendar/></d:resourcetype><d:displayname>主日历</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>";
        let (url,server)=mock(vec![wrap("<d:current-user-principal><d:href>/principal/me/</d:href></d:current-user-principal>"),wrap("<c:calendar-home-set><d:href>/home/</d:href></c:calendar-home-set>"),list.into()]).await;
        let dav = Dav {
            client: reqwest::Client::new(),
            base: url,
            username: "test".into(),
            password: "test-only".into(),
        };
        let list = dav.discover().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "主日历");
        let requests = server.await.unwrap();
        assert!(requests[1].starts_with("PROPFIND /principal/me/ HTTP/1.1"));
        assert!(requests[2].starts_with("PROPFIND /home/ HTTP/1.1"));
    }
    fn test_dav(base: Url) -> Dav {
        Dav {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            base,
            username: "fixture".into(),
            password: "fixture-only".into(),
        }
    }
    #[tokio::test]
    async fn root_discovers_well_known_and_preserves_propfind_through_302() {
        let xml = "<d:multistatus xmlns:d='DAV:'/>";
        let (mut base, server) = mock(vec!["HTTP/1.1 302 Found\r\nLocation: /dav/principals/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(), xml.into()]).await;
        base.set_path("/");
        let dav = test_dav(base.clone());
        assert!(dav.prop(&base, "0").await.is_ok());
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("PROPFIND /.well-known/caldav HTTP/1.1"));
        assert!(requests[1].starts_with("PROPFIND /dav/principals/ HTTP/1.1"));
        assert!(requests
            .iter()
            .all(|r| r.contains("<d:current-user-principal/>")
                && r.to_ascii_lowercase().contains("authorization: basic ")));
    }
    #[tokio::test]
    async fn unsupported_discovery_falls_back_but_authentication_does_not() {
        let (mut base, server) = mock(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            "<d:multistatus xmlns:d='DAV:'/>".into(),
        ])
        .await;
        base.set_path("/");
        assert!(test_dav(base.clone()).prop(&base, "0").await.is_ok());
        assert!(server.await.unwrap()[1].starts_with("PROPFIND / HTTP/1.1"));
        let (mut base, server) = mock(vec![
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ])
        .await;
        base.set_path("/");
        assert!(test_dav(base.clone())
            .prop(&base, "0")
            .await
            .unwrap_err()
            .contains("401"));
        assert_eq!(server.await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn redirect_to_another_origin_and_loops_are_rejected() {
        for location in ["https://outside.example.test/", "/dav/"] {
            let (base,server)=mock(vec![format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")]).await;
            assert!(test_dav(base.clone()).prop(&base, "0").await.is_err());
            assert_eq!(server.await.unwrap().len(), 1);
        }
    }
}
