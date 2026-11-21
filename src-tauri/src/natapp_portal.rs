//! 适配 NATAPP 官网后台的登录表单、列表请求和免费隧道表单。
use super::{read, snapshot, validate_auth_token, write, NatappManager, Tunnel};
use crate::{credential_vault, storage::Store, AppState};
use futures_util::{stream, StreamExt};
use reqwest::{Client, Response, StatusCode};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    net::Ipv4Addr,
    path::PathBuf,
    time::Duration,
};
use tauri::{webview::Cookie, AppHandle, State};

const ORIGIN: &str = "https://natapp.cn";
const LOGIN: &str = "https://natapp.cn/login";
const LIST: &str = "https://natapp.cn/tunnel/lists";
const LIST_JSON: &str = "https://natapp.cn/tunnel/listsjson";
const PURCHASE: &str = "https://natapp.cn/tunnel/buy";
const FREE: &str = "https://natapp.cn/tunnel/buy/free";
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const UNCERTAIN: &str = "NATAPP 创建结果未确认，请先同步隧道或在控制台查看，再决定是否重试。";
static PORTAL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Deserialize, Serialize)]
pub struct Credentials {
    account: String,
    password: String,
}

#[derive(Deserialize)]
struct PortalConfig {
    #[serde(rename = "csrfToken")]
    csrf_token: String,
    #[serde(rename = "listUrl")]
    list_url: String,
}

#[derive(Deserialize)]
struct RemoteList {
    #[serde(rename = "recordsTotal")]
    total: usize,
    data: Vec<RemoteTunnel>,
}

#[derive(Deserialize)]
struct RemoteTunnel {
    hash_id: String,
    name: String,
    authtoken: String,
    protocol_key: String,
    #[serde(flatten)]
    details: RemoteDetailsFields,
    #[serde(skip)]
    configured_target: Option<TunnelTarget>,
    #[serde(skip)]
    created_at: Option<chrono::NaiveDateTime>,
}

#[derive(Default, Deserialize)]
struct RemoteDetailsFields {
    url: Option<String>,
    protocol: Option<String>,
    #[serde(rename = "type")]
    plan: Option<String>,
    is_online: Option<Value>,
    active: Option<Value>,
    flow: Option<Value>,
    flow_limit: Option<Value>,
    end_time: Option<String>,
    trial_expires_at: Option<Value>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoteDetails {
    endpoint: Option<String>,
    protocol: Option<String>,
    plan: Option<String>,
    status: Option<RemoteStatus>,
    used_bytes: Option<u64>,
    limit_bytes: Option<u64>,
    created_at: Option<chrono::NaiveDateTime>,
    expires_on: Option<chrono::NaiveDate>,
    permanent: bool,
    trial_expires_at: Option<i64>,
    synced_at: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum RemoteStatus {
    Online,
    Offline,
    Disabled,
}

fn number(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

impl RemoteTunnel {
    fn public_details(&self, synced_at: &str) -> RemoteDetails {
        let details = &self.details;
        let public_text = |text: Option<&str>, max_len| {
            text.map(str::trim)
                .filter(|text| {
                    !text.is_empty()
                        && text.len() <= max_len
                        && !text.chars().any(char::is_control)
                        && !text
                            .to_ascii_lowercase()
                            .contains(&self.authtoken.to_ascii_lowercase())
                })
                .map(str::to_owned)
        };
        let endpoint = public_text(details.url.as_deref(), 512).filter(|value| {
            let addr = if value.contains("://") {
                url::Url::parse(value)
            } else {
                url::Url::parse(&format!("{}://{value}", self.protocol_key))
            };
            addr.is_ok_and(|addr| {
                matches!(addr.scheme(), "http" | "https" | "tcp" | "udp")
                    && super::plain_endpoint(&addr)
                    && addr.port() != Some(0)
            })
        });
        let status = match (
            number(details.active.as_ref()),
            number(details.is_online.as_ref()),
        ) {
            (Some(0), _) => Some(RemoteStatus::Disabled),
            (_, Some(1)) => Some(RemoteStatus::Online),
            (_, Some(0)) => Some(RemoteStatus::Offline),
            _ => None,
        };
        let expires_on = details.end_time.as_deref().and_then(|date| {
            let date = date.trim().replace(['年', '月'], "-").replace('日', "");
            chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()
        });
        RemoteDetails {
            endpoint,
            protocol: public_text(details.protocol.as_deref(), 24),
            plan: public_text(details.plan.as_deref(), 60),
            status,
            used_bytes: number(details.flow.as_ref()).and_then(|value| u64::try_from(value).ok()),
            limit_bytes: number(details.flow_limit.as_ref())
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| *value > 0),
            created_at: self.created_at,
            expires_on,
            permanent: details.end_time.as_deref() == Some("-"),
            trial_expires_at: number(details.trial_expires_at.as_ref())
                .filter(|value| (1..=253_402_300_799).contains(value)),
            synced_at: synced_at.to_owned(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Protocol {
    Http,
    Tcp,
    Udp,
}

impl Protocol {
    fn value(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTunnelInput {
    name: String,
    protocol: Protocol,
    #[serde(flatten)]
    target: TunnelTarget,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TunnelTarget {
    local_ip: Ipv4Addr,
    local_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inspection: Option<InspectionConfig>,
}

const DEFAULT_INSPECTION_PORT: u16 = 4040;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InspectionConfig {
    enabled: bool,
    port: u16,
}

impl TunnelTarget {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.local_port == 0 {
            return Err("本地端口须为 1–65535".into());
        }
        if self
            .inspection
            .as_ref()
            .is_some_and(|config| config.enabled && config.port == 0)
        {
            return Err("流量控制台端口须为 1–65535".into());
        }
        if self.local_ip.is_loopback() && self.inspection_port() == Some(self.local_port) {
            return Err("流量控制台端口不能与本地映射端口相同".into());
        }
        Ok(())
    }

    pub(super) fn inspection_port(&self) -> Option<u16> {
        self.inspection
            .as_ref()
            .filter(|config| config.enabled && config.port != 0)
            .map(|config| config.port)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureTargetInput {
    id: String,
    revision: u64,
    name: String,
    previous: TunnelTarget,
    target: TunnelTarget,
}

struct RemoteEditForm {
    target: TunnelTarget,
    created_at: Option<chrono::NaiveDateTime>,
    fields: Vec<(String, String)>,
    close_inspection_value: Option<String>,
}

impl RemoteEditForm {
    fn parse(page: &str) -> Result<Self, String> {
        form_token(page, "#buy_form", "")?;
        let document = Html::parse_document(page);
        let form = document
            .select(&Selector::parse("form#buy_form").unwrap())
            .next()
            .ok_or("NATAPP 隧道配置表单已变化")?;
        if form
            .select(&Selector::parse("fieldset[disabled]").unwrap())
            .next()
            .is_some()
            || document
                .select(&Selector::parse("[form='buy_form']").unwrap())
                .next()
                .is_some()
        {
            return Err("NATAPP 隧道配置表单已变化，请使用控制台修改".into());
        }
        let controls = Selector::parse("input[name], textarea[name], select[name]").unwrap();
        let options = Selector::parse("option").unwrap();
        let mut fields = Vec::new();
        // 按官网表单提交规则采集现有值，保留证书、访问密码及 IP 黑白名单等配置。
        for element in form.select(&controls) {
            if element.attr("disabled").is_some() || element.attr("form").is_some() {
                continue;
            }
            let name = element.attr("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let value = match element.value().name() {
                "textarea" => element.text().collect(),
                "select" => {
                    let mut selected: Vec<_> = element
                        .select(&options)
                        .filter(|option| option.attr("selected").is_some())
                        .collect();
                    let enabled = |option: &scraper::ElementRef<'_>| {
                        option.attr("disabled").is_none()
                            && !option
                                .parent()
                                .and_then(scraper::ElementRef::wrap)
                                .is_some_and(|parent| {
                                    parent.value().name() == "optgroup"
                                        && parent.attr("disabled").is_some()
                                })
                    };
                    if element.attr("multiple").is_none() {
                        let single = selected
                            .pop()
                            .or_else(|| element.select(&options).find(enabled));
                        selected.clear();
                        selected.extend(single);
                    }
                    for option in selected.into_iter().filter(enabled) {
                        let value = option
                            .attr("value")
                            .map(str::to_owned)
                            .unwrap_or_else(|| option.text().collect());
                        fields.push((name.to_owned(), value));
                    }
                    continue;
                }
                _ => match element.attr("type").unwrap_or("text") {
                    "checkbox" | "radio" => {
                        if element.attr("checked").is_none() {
                            continue;
                        }
                        element.attr("value").unwrap_or("on").to_owned()
                    }
                    "button" | "submit" | "reset" => continue,
                    "hidden" | "text" | "password" | "number" | "url" | "email" => {
                        element.attr("value").unwrap_or_default().to_owned()
                    }
                    _ => return Err("NATAPP 隧道配置表单已变化，请使用控制台修改".into()),
                },
            };
            fields.push((name.to_owned(), value));
        }
        let field = |key: &str| {
            let mut values = fields.iter().filter(|(name, _)| name == key);
            let value = values
                .next()
                .filter(|_| values.next().is_none())
                .ok_or("NATAPP 配置表单缺少必填字段或字段重复")?;
            Ok::<_, &str>(value.1.as_str())
        };
        field("_token")?;
        let checkbox_selector =
            Selector::parse("input[type='checkbox'][name='isCloseInspect']").unwrap();
        let port_selector = Selector::parse("input[name='inspect_port']").unwrap();
        let (inspection, close_inspection_value) = match (
            form.select(&checkbox_selector).next(),
            form.select(&port_selector).next(),
        ) {
            (Some(checkbox), Some(input)) => {
                let raw_port = input.attr("value").unwrap_or_default().trim();
                let enabled = checkbox.attr("checked").is_none() && raw_port != "disabled";
                let port = if enabled && !raw_port.is_empty() {
                    raw_port
                        .parse::<u16>()
                        .map_err(|_| "NATAPP 返回了无效的流量控制台端口")?
                } else {
                    DEFAULT_INSPECTION_PORT
                };
                (
                    Some(InspectionConfig { enabled, port }),
                    Some(checkbox.attr("value").unwrap_or("on").to_owned()),
                )
            }
            _ => (None, None),
        };
        let target = TunnelTarget {
            local_ip: field("local_ip")?
                .trim()
                .parse()
                .map_err(|_| "NATAPP 返回了不支持的本地地址")?,
            local_port: field("local_port")?
                .trim()
                .parse()
                .map_err(|_| "NATAPP 返回了无效的本地端口")?,
            inspection,
        };
        // 创建时间在配置页的只读字段中，列表接口和编辑提交字段均不包含。
        let labels = Selector::parse(".form-group > label").unwrap();
        let static_value = Selector::parse("p.form-control-static").unwrap();
        let created_at = document.select(&labels).find_map(|label| {
            let text = label.text().collect::<String>();
            if text.trim().trim_end_matches([':', '：']).trim() != "创建时间" {
                return None;
            }
            let group = label.parent().and_then(scraper::ElementRef::wrap)?;
            let value = group
                .select(&static_value)
                .next()?
                .text()
                .collect::<String>();
            chrono::NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S").ok()
        });
        Ok(Self {
            target,
            created_at,
            fields,
            close_inspection_value,
        })
    }

    fn other_fields(&self) -> impl Iterator<Item = &(String, String)> {
        self.fields.iter().filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "_token" | "local_ip" | "local_port" | "isCloseInspect" | "inspect_port"
            )
        })
    }

    fn fields_for_target(&self, target: &TunnelTarget) -> Vec<(String, String)> {
        let mut fields: Vec<_> = self
            .fields
            .iter()
            .map(|(name, value)| {
                let value = match name.as_str() {
                    "local_ip" => target.local_ip.to_string(),
                    "local_port" => target.local_port.to_string(),
                    _ => value.clone(),
                };
                (name.clone(), value)
            })
            .collect();
        if let (Some(config), Some(close)) = (&target.inspection, &self.close_inspection_value) {
            fields.retain(|(name, _)| !matches!(name.as_str(), "isCloseInspect" | "inspect_port"));
            fields.push(("inspect_port".into(), config.port.to_string()));
            if !config.enabled {
                fields.push(("isCloseInspect".into(), close.clone()));
            }
        }
        fields
    }
}

async fn read_edit_form(client: &Client, url: &str) -> Result<RemoteEditForm, String> {
    let page = body(
        client
            .get(url)
            .send()
            .await
            .map_err(|_| "无法读取 NATAPP 隧道配置")?,
    )
    .await?;
    RemoteEditForm::parse(&page)
}

async fn read_portal_configs(client: &Client, tunnels: &mut [RemoteTunnel]) {
    stream::iter(tunnels)
        .for_each_concurrent(4, |item| async move {
            let url = format!("{ORIGIN}/tunnel/edit/{}", item.hash_id);
            if let Ok(form) = read_edit_form(client, &url).await {
                item.created_at = form.created_at;
                item.configured_target = Some(form.target);
            }
        })
        .await;
}

async fn save_target(
    client: &Client,
    url: &str,
    form: RemoteEditForm,
    target: &TunnelTarget,
) -> Result<TunnelTarget, String> {
    let mut target = target.clone();
    if form.target.inspection.is_none() {
        if target
            .inspection
            .as_ref()
            .is_some_and(|config| config.enabled)
        {
            return Err("当前隧道不支持本地流量控制台".into());
        }
        target.inspection = None;
    } else if target.inspection.is_none() {
        target.inspection = form.target.inspection.clone();
    }
    if let Some(config) = target.inspection.as_mut().filter(|config| !config.enabled) {
        config.port = DEFAULT_INSPECTION_PORT;
    }
    if form.target == target {
        return Ok(target);
    }
    let fields = form.fields_for_target(&target);
    // 请求结果不明确时只读回查，避免重复提交；只有回读一致才算保存成功。
    let _ = client
        .post(url)
        .header("Origin", ORIGIN)
        .header("Referer", url)
        .form(&fields)
        .send()
        .await;
    let verified = read_edit_form(client, url)
        .await
        .map_err(|_| "NATAPP 隧道配置未确认，请先在官网控制台核对")?;
    if verified.other_fields().ne(form.other_fields()) {
        return Err("NATAPP 的其他配置发生变化，请在控制台核对后再启动".into());
    }
    if verified.target != target {
        return Err("NATAPP 未保存所选配置，请在官网控制台核对".into());
    }
    Ok(verified.target)
}

fn vault(workspace: &str) -> Result<credential_vault::Entry, String> {
    credential_vault::entry(workspace, "com.self.workbench", "natapp-account")
        .map_err(|e| e.to_string())
}

fn saved_credentials(workspace: &str) -> Result<Option<String>, String> {
    match vault(workspace)?.get_password() {
        Ok(raw) => Ok(Some(raw)),
        Err(credential_vault::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn credentials(workspace: &str, input: Option<Credentials>) -> Result<Credentials, String> {
    let mut value = match input {
        Some(value) => value,
        None => {
            let raw = saved_credentials(workspace)?.ok_or("请先连接 NATAPP 账号")?;
            serde_json::from_str(&raw).map_err(|_| "NATAPP 账号配置无法读取")?
        }
    };
    value.account = value.account.trim().into();
    if value.account.is_empty()
        || value.account.len() > 320
        || value.password.is_empty()
        || value.password.len() > 1024
        || value.account.chars().any(char::is_control)
    {
        return Err("请输入有效的 NATAPP 账号和密码".into());
    }
    Ok(value)
}

fn current_generation(state: &AppState, workspace: &str) -> Result<PathBuf, String> {
    let store = state.store.lock().map_err(|_| "无法读取当前工作空间")?;
    if store.workspace_id != workspace {
        return Err("工作空间已切换，请重新打开 NATAPP 工具".into());
    }
    Ok(store.generation.clone())
}

pub(super) fn local_tunnel(
    store: &Store,
    workspace_id: &str,
    id: &str,
    revision: u64,
) -> Result<Tunnel, String> {
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开 NATAPP 工具".into());
    }
    let tunnel = read(store)?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or("隧道不存在")?;
    if tunnel.revision != revision {
        return Err("隧道配置已变化，请关闭弹窗后重新编辑".into());
    }
    Ok(tunnel)
}

async fn bound_edit_url(
    client: &Client,
    workspace_id: &str,
    tunnel: &Tunnel,
) -> Result<Option<String>, String> {
    let token = credential_vault::decrypt(
        workspace_id,
        &format!("natapp:{}", tunnel.id),
        &tunnel.encrypted_token,
    )?;
    let owned = tunnels(client).await?;
    let matched = owned.iter().find(|remote| {
        remote.authtoken == token
            && tunnel
                .remote_id
                .as_ref()
                .is_none_or(|remote_id| remote_id == &remote.hash_id)
    });
    Ok(matched.map(|remote| format!("{ORIGIN}/tunnel/edit/{}", remote.hash_id)))
}

struct TargetEdit {
    manager: NatappManager,
    id: String,
}

impl Drop for TargetEdit {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.manager.inner.lock() {
            inner.target_edits.remove(&self.id);
        }
    }
}

async fn body(mut response: Response) -> Result<String, String> {
    if !response.status().is_success() {
        return Err(format!("NATAPP 返回 HTTP {}", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取 NATAPP 响应失败")? {
        if bytes.len() + chunk.len() > RESPONSE_LIMIT {
            return Err("NATAPP 响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "NATAPP 响应编码无法读取".into())
}

fn form_token(page: &str, form: &str, expected_action: &str) -> Result<String, String> {
    let document = Html::parse_document(page);
    let selector = Selector::parse(form).expect("固定表单选择器");
    let form = document
        .select(&selector)
        .next()
        .ok_or("NATAPP 网页表单结构已变化")?;
    if form
        .attr("method")
        .is_none_or(|method| !method.eq_ignore_ascii_case("post"))
        || form.attr("action").unwrap_or_default() != expected_action
    {
        return Err("NATAPP 网页表单地址已变化".into());
    }
    let token = Selector::parse("input[name='_token']").expect("固定令牌选择器");
    form.select(&token)
        .next()
        .and_then(|input| input.attr("value"))
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_owned)
        .ok_or_else(|| "NATAPP 网页缺少会话令牌".into())
}

async fn login(value: &Credentials) -> Result<Client, String> {
    let (client, _) = login_response(value).await?;
    Ok(client)
}

async fn login_response(value: &Credentials) -> Result<(Client, Response), String> {
    let client = Client::builder()
        .cookie_store(true)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .user_agent("Mozilla/5.0 Perch NATAPP")
        .build()
        .map_err(|_| "无法初始化 NATAPP 连接")?;
    let page = body(
        client
            .get(LOGIN)
            .send()
            .await
            .map_err(|_| "无法连接 NATAPP 官网")?,
    )
    .await?;
    let csrf = form_token(&page, "#login_form", LOGIN)?;
    let response = client
        .post(LOGIN)
        .header("Origin", ORIGIN)
        .header("Referer", LOGIN)
        .form(&[
            ("login", value.account.as_str()),
            ("password", value.password.as_str()),
            ("_token", csrf.as_str()),
            ("agree", "on"),
        ])
        .send()
        .await
        .map_err(|_| "NATAPP 登录请求失败")?;
    let redirect = response
        .headers()
        .get("Location")
        .and_then(|header| header.to_str().ok());
    if response.status().is_redirection()
        && matches!(
            redirect,
            Some("/member/dashborad" | "https://natapp.cn/member/dashborad")
        )
    {
        return Ok((client, response));
    }
    Err("NATAPP 登录未通过，请核对账号密码；如需额外验证，请通过官网链接打开 NATAPP。".into())
}

fn console_cookies(headers: &reqwest::header::HeaderMap) -> Result<Vec<Cookie<'static>>, String> {
    let mut cookies = Vec::new();
    for header in headers.get_all(reqwest::header::SET_COOKIE) {
        let Ok(value) = header.to_str() else {
            continue;
        };
        let Ok(mut cookie) = Cookie::parse(value.to_owned()) else {
            continue;
        };
        if !matches!(cookie.name(), "laravel_session" | "XSRF-TOKEN")
            || cookie.domain().is_some_and(|domain| domain != "natapp.cn")
            || cookie.path().is_some_and(|path| path != "/")
            || cookie.value().is_empty()
            || cookie.max_age().is_some_and(|age| age.whole_seconds() <= 0)
            || cookie
                .expires_datetime()
                .is_some_and(|expiry| expiry.unix_timestamp() <= chrono::Utc::now().timestamp())
        {
            continue;
        }
        if cookie.name() == "laravel_session" {
            cookie.set_http_only(true);
        }
        cookie.set_domain("natapp.cn");
        cookie.set_path("/");
        cookie.set_secure(true);
        cookies.retain(|existing: &Cookie<'_>| existing.name() != cookie.name());
        cookies.push(cookie);
    }
    if !cookies
        .iter()
        .any(|cookie| cookie.name() == "laravel_session")
    {
        return Err("NATAPP 未返回控制台登录会话，请通过官网链接打开 NATAPP".into());
    }
    Ok(cookies)
}

async fn tunnels(client: &Client) -> Result<Vec<RemoteTunnel>, String> {
    let page = body(
        client
            .get(LIST)
            .send()
            .await
            .map_err(|_| "无法打开 NATAPP 隧道列表")?,
    )
    .await?;
    let config: PortalConfig = {
        let document = Html::parse_document(&page);
        let selector = Selector::parse("script#tunnel-console-config[type='application/json']")
            .expect("固定列表选择器");
        let element = document
            .select(&selector)
            .next()
            .ok_or("NATAPP 隧道列表结构已变化")?;
        serde_json::from_str(&element.text().collect::<String>())
            .map_err(|_| "无法读取 NATAPP 隧道列表配置")?
    };
    if config.list_url != LIST_JSON || config.csrf_token.is_empty() {
        return Err("NATAPP 隧道列表接口已变化".into());
    }
    let response = client
        .post(LIST_JSON)
        .header("Origin", ORIGIN)
        .header("Referer", LIST)
        .header("X-CSRF-TOKEN", config.csrf_token)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Accept", "application/json")
        .form(&[("draw", "1")])
        .send()
        .await
        .map_err(|_| "无法读取 NATAPP 隧道列表")?;
    let page = body(response).await?;
    let list: RemoteList = serde_json::from_str(&page).map_err(|_| "NATAPP 隧道列表响应无效")?;
    if list.total != list.data.len() {
        return Err("NATAPP 隧道列表不完整，请使用控制台查看".into());
    }
    let mut ids = HashSet::new();
    let mut tokens = HashSet::new();
    for item in &list.data {
        if item.hash_id.is_empty()
            || item.hash_id.len() > 80
            || !item.hash_id.bytes().all(|b| b.is_ascii_alphanumeric())
            || !ids.insert(item.hash_id.as_str())
            || !tokens.insert(item.authtoken.as_str())
            || item.name.trim().is_empty()
            || item.name.len() > 120
            || item.name.chars().any(char::is_control)
        {
            return Err("NATAPP 返回的隧道信息无效".into());
        }
        validate_auth_token(&item.authtoken)?;
    }
    Ok(list.data)
}

fn import(
    store: &Store,
    manager: &NatappManager,
    remote: Vec<RemoteTunnel>,
) -> Result<Value, String> {
    let mut items = read(store)?;
    let active = manager.inner.lock().map_err(|_| "无法读取本地隧道状态")?;
    let mut token_index = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let token = credential_vault::decrypt(
            &store.workspace_id,
            &format!("natapp:{}", item.id),
            &item.encrypted_token,
        )?;
        token_index.entry(token).or_insert(index);
    }
    let mut changed = false;
    let synced_at = chrono::Utc::now().to_rfc3339();
    for item in remote {
        let mut remote_details = item.public_details(&synced_at);
        let by_id = items
            .iter()
            .position(|local| local.remote_id.as_deref() == Some(&item.hash_id));
        let by_token = token_index.get(&item.authtoken).copied();
        if let Some(index) = by_token {
            let associated_id = items[index].remote_id.as_deref();
            if by_id.is_some_and(|id_index| id_index != index)
                || associated_id.is_some_and(|id| id != item.hash_id)
            {
                return Err("NATAPP 隧道与本地令牌关联冲突，请核对本地配置".into());
            }
        }
        if let Some(index) = by_id.or(by_token) {
            let local = &mut items[index];
            if remote_details.created_at.is_none() {
                remote_details.created_at = local
                    .remote_details
                    .as_ref()
                    .and_then(|details| details.created_at);
            }
            local.remote_details = Some(remote_details);
            if let Some(target) = item.configured_target {
                local.configured_target = Some(target);
            }
            if local.remote_id.as_deref() != Some(&item.hash_id) {
                local.remote_id = Some(item.hash_id);
            }
            changed = true;
            if active
                .processes
                .get(&local.id)
                .is_some_and(|process| matches!(process.status.as_str(), "running" | "starting"))
            {
                continue;
            }
            if by_token != Some(index) {
                local.encrypted_token = credential_vault::encrypt(
                    &store.workspace_id,
                    &format!("natapp:{}", local.id),
                    &item.authtoken,
                )?;
                local.revision += 1;
            }
            continue;
        }
        let id = uuid::Uuid::now_v7().to_string();
        let encrypted_token = credential_vault::encrypt(
            &store.workspace_id,
            &format!("natapp:{id}"),
            &item.authtoken,
        )?;
        items.push(Tunnel {
            id,
            name: item.name,
            revision: 1,
            encrypted_token,
            remote_id: Some(item.hash_id),
            remote_details: Some(remote_details),
            configured_target: item.configured_target,
        });
        changed = true;
    }
    drop(active);
    if changed {
        write(store, &items)?;
    }
    snapshot(store, manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "使用测试账号验证官网登录及只读列表，账号密码通过标准输入提供"]
    async fn read_live_natapp_tunnels() {
        use std::io::{self, BufRead};
        println!("请通过标准输入提供账号和密码（各占一行）");
        let value = {
            let mut lines = io::stdin().lock().lines();
            Credentials {
                account: lines
                    .next()
                    .expect("缺少测试账号")
                    .expect("无法读取测试账号"),
                password: lines
                    .next()
                    .expect("缺少测试密码")
                    .expect("无法读取测试密码"),
            }
        };
        let (client, response) = login_response(&value).await.expect("NATAPP 登录失败");
        drop(value);
        let result = tunnels(&client).await.expect("NATAPP 隧道列表读取失败");
        if let Some(tunnel) = result.first() {
            let url = format!("{ORIGIN}/tunnel/edit/{}", tunnel.hash_id);
            let form = read_edit_form(&client, &url)
                .await
                .expect("NATAPP 映射配置读取失败");
            assert!(
                form.target.validate().is_ok(),
                "NATAPP 返回了无效的映射配置"
            );
            println!("已读取现有隧道的地址与端口配置");
        }
        let cookies = console_cookies(response.headers()).expect("NATAPP 控制台会话无效");
        let mut cookie_header = reqwest::header::HeaderValue::from_str(
            &cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
                .collect::<Vec<_>>()
                .join("; "),
        )
        .expect("无法载入会话");
        cookie_header.set_sensitive(true);
        let console = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap();
        for (url, marker) in [
            (LIST, "tunnel-console-config"),
            (PURCHASE, "/tunnel/buy/free"),
        ] {
            let response = console
                .get(url)
                .header(reqwest::header::COOKIE, cookie_header.clone())
                .send()
                .await
                .expect("NATAPP 控制台请求失败");
            let page = body(response).await.expect("控制台会话未通过");
            assert!(page.contains(marker), "未打开指定的已登录页面");
        }
        println!("已核验 {} 条 NATAPP 隧道", result.len());
        println!("独立会话已打开我的隧道和购买页");
    }

    fn remote(token: &str) -> Vec<RemoteTunnel> {
        vec![RemoteTunnel {
            hash_id: "fixture01".into(),
            name: "测试隧道".into(),
            authtoken: token.into(),
            protocol_key: "tcp".into(),
            details: RemoteDetailsFields::default(),
            configured_target: None,
            created_at: None,
        }]
    }

    #[test]
    fn sync_reuses_local_tunnel_and_keeps_rotated_token_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        let manager = NatappManager::default();
        let token = "0000000000000001";
        let id = uuid::Uuid::now_v7().to_string();
        let purpose = format!("natapp:{id}");
        write(
            &store,
            &[Tunnel {
                id: id.clone(),
                name: "本地别名".into(),
                revision: 1,
                encrypted_token: credential_vault::encrypt(&store.workspace_id, &purpose, token)
                    .unwrap(),
                remote_id: None,
                remote_details: None,
                configured_target: None,
            }],
        )
        .unwrap();

        import(&store, &manager, remote(token)).unwrap();
        import(&store, &manager, remote(token)).unwrap();
        let rotated = "0000000000000002";
        let view = import(&store, &manager, remote(rotated)).unwrap();
        let tunnels = read(&store).unwrap();
        assert_eq!(tunnels.len(), 1);
        assert_eq!(tunnels[0].id, id);
        assert_eq!(tunnels[0].name, "本地别名");
        assert_eq!(tunnels[0].revision, 2);
        assert_eq!(tunnels[0].remote_id.as_deref(), Some("fixture01"));
        assert_ne!(tunnels[0].encrypted_token, rotated);
        assert_eq!(
            credential_vault::decrypt(&store.workspace_id, &purpose, &tunnels[0].encrypted_token)
                .unwrap(),
            rotated
        );
        assert!(!view.to_string().contains(rotated));
    }

    #[test]
    fn signup_forms_are_pinned_to_the_observed_action() {
        let fixture = r#"<form id="buy_form" method="POST" action=""><input name="_token" value="fixture-csrf"></form>"#;
        assert_eq!(
            form_token(fixture, "#buy_form", "").unwrap(),
            "fixture-csrf"
        );
        assert!(form_token(fixture, "#buy_form", LOGIN).is_err());
        assert!(form_token(fixture, "#login_form", "").is_err());
    }

    #[test]
    fn target_update_preserves_existing_form_settings() {
        let fixture = r#"<form id="buy_form" method="POST" action="">
            <input name="_token" value="sample-token">
            <input name="name" value="现有隧道">
            <input name="local_ip" value="127.0.0.1">
            <input name="local_port" value="80">
            <input type="checkbox" name="https" checked>
            <input type="checkbox" name="custom_robots">
            <input name="host_header" disabled>
            <input type="password" name="auth_pass" value="sample-only">
            <textarea name="ip_white_list">192.0.2.1
192.0.2.2</textarea>
            <select name="subdomain">
                <option value="placeholder" disabled>请选择</option>
                <option value="sample-domain">示例域名</option>
            </select>
        </form>"#;
        let form = RemoteEditForm::parse(fixture).unwrap();
        let target = TunnelTarget {
            local_ip: "192.0.2.100".parse().unwrap(),
            local_port: 8080,
            inspection: None,
        };
        let fields: HashMap<_, _> = form.fields_for_target(&target).into_iter().collect();
        assert_eq!(fields["local_ip"], "192.0.2.100");
        assert_eq!(fields["local_port"], "8080");
        assert_eq!(fields["https"], "on");
        assert_eq!(fields["auth_pass"], "sample-only");
        assert_eq!(fields["ip_white_list"], "192.0.2.1\n192.0.2.2");
        assert_eq!(fields["subdomain"], "sample-domain");
        assert!(!fields.contains_key("custom_robots"));
        assert!(!fields.contains_key("host_header"));
        assert!(RemoteEditForm::parse(
            &fixture.replace("name=\"host_header\" disabled", "name=\"local_port\"")
        )
        .is_err());
    }
}

#[tauri::command]
pub fn natapp_account_configured(
    state: State<AppState>,
    workspace_id: String,
) -> Result<bool, String> {
    current_generation(&state, &workspace_id)?;
    Ok(saved_credentials(&workspace_id)?.is_some())
}

#[tauri::command]
pub async fn natapp_open_console(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, NatappManager>,
    workspace_id: String,
    purchase: bool,
) -> Result<(), String> {
    let _portal = PORTAL_LOCK.lock().await;
    let generation = current_generation(&state, &workspace_id)?;
    let value = credentials(&workspace_id, None)?;
    let (_, response) = login_response(&value).await?;
    drop(value);
    let cookies = console_cookies(response.headers())?;
    let url = if purchase { PURCHASE } else { LIST }
        .parse()
        .map_err(|_| "NATAPP 控制台地址无效")?;
    let _operation = manager
        .operation
        .lock()
        .map_err(|_| "无法锁定 NATAPP 工具")?;
    if current_generation(&state, &workspace_id)? != generation {
        return Err("工作空间已切换，请重新打开 NATAPP 控制台".into());
    }
    super::open_console(&app, url, cookies)
}

#[tauri::command]
pub async fn natapp_disconnect_account(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    let _portal = PORTAL_LOCK.lock().await;
    current_generation(&state, &workspace_id)?;
    super::close_console(&app)?;
    match vault(&workspace_id)?.delete_credential() {
        Ok(()) | Err(credential_vault::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub async fn natapp_sync_tunnels(
    app: AppHandle,
    state: State<'_, AppState>,
    manager: State<'_, NatappManager>,
    workspace_id: String,
    login_input: Option<Credentials>,
) -> Result<Value, String> {
    let _portal = PORTAL_LOCK.lock().await;
    let generation = current_generation(&state, &workspace_id)?;
    let changed_account = login_input.is_some();
    let value = credentials(&workspace_id, login_input)?;
    let client = login(&value).await?;
    let mut remote = tunnels(&client).await?;
    read_portal_configs(&client, &mut remote).await;
    let _op = manager
        .operation
        .lock()
        .map_err(|_| "无法锁定 NATAPP 工具")?;
    let store = state.store.lock().map_err(|_| "无法读取当前工作空间")?;
    if store.generation != generation {
        return Err("工作空间已切换，隧道同步已取消".into());
    }
    let result = import(&store, &manager, remote)?;
    if changed_account {
        super::close_console(&app)?;
        let raw = serde_json::to_string(&value).map_err(|_| "无法编码 NATAPP 账号配置")?;
        vault(&workspace_id)?
            .set_password(&raw)
            .map_err(|_| "隧道已同步，但 NATAPP 账号加密保存失败")?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn natapp_read_target(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    revision: u64,
) -> Result<Option<TunnelTarget>, String> {
    let _portal = PORTAL_LOCK.lock().await;
    let (tunnel, generation) = {
        let store = state.store.lock().map_err(|_| "无法读取当前工作空间")?;
        (
            local_tunnel(&store, &workspace_id, &id, revision)?,
            store.generation.clone(),
        )
    };
    let client = login(&credentials(&workspace_id, None)?).await?;
    let target = match bound_edit_url(&client, &workspace_id, &tunnel).await? {
        Some(url) => Some(read_edit_form(&client, &url).await?.target),
        None => None,
    };
    let store = state.store.lock().map_err(|_| "无法读取当前工作空间")?;
    if store.generation != generation {
        return Err("工作空间已切换，请重新编辑隧道".into());
    }
    local_tunnel(&store, &workspace_id, &id, revision)?;
    Ok(target)
}

#[tauri::command]
pub async fn natapp_configure_target(
    state: State<'_, AppState>,
    manager: State<'_, NatappManager>,
    workspace_id: String,
    value: ConfigureTargetInput,
) -> Result<Value, String> {
    let _portal = PORTAL_LOCK.lock().await;
    value.target.validate()?;
    let name = value.name.trim();
    if name.is_empty() || name.len() > 120 || name.chars().any(char::is_control) {
        return Err("请输入 1–120 字符的隧道名称".into());
    }
    let (_edit, tunnel, generation) = {
        let _op = manager
            .operation
            .lock()
            .map_err(|_| "无法锁定 NATAPP 工具")?;
        let store = state.store.lock().map_err(|_| "无法读取当前工作空间")?;
        let tunnel = local_tunnel(&store, &workspace_id, &value.id, value.revision)?;
        let mut inner = manager.inner.lock().map_err(|_| "无法读取本地隧道状态")?;
        inner.ensure_target_idle(&value.id)?;
        if inner
            .processes
            .get(&value.id)
            .is_some_and(|process| matches!(process.status.as_str(), "running" | "starting"))
        {
            return Err("请先停止隧道再修改配置".into());
        }
        inner.target_edits.insert(value.id.clone());
        (
            TargetEdit {
                manager: manager.inner().clone(),
                id: value.id.clone(),
            },
            tunnel,
            store.generation.clone(),
        )
    };
    let client = login(&credentials(&workspace_id, None)?).await?;
    let url = bound_edit_url(&client, &workspace_id, &tunnel)
        .await?
        .ok_or("当前 NATAPP 账号下找不到对应隧道，请先同步隧道并确认账号")?;
    let form = read_edit_form(&client, &url).await?;
    let created_at = form.created_at;
    if form.target != value.previous && form.target != value.target {
        return Err("NATAPP 隧道配置已变化，请关闭弹窗后重新编辑".into());
    }
    let target = save_target(&client, &url, form, &value.target).await?;
    let _op = manager
        .operation
        .lock()
        .map_err(|_| "官网配置已保存，请刷新本地隧道")?;
    let store = state
        .store
        .lock()
        .map_err(|_| "官网配置已保存，请刷新本地隧道")?;
    if store.generation != generation {
        return Err("官网配置已保存，工作空间已切换".into());
    }
    let mut items = read(&store)?;
    let local = items
        .iter_mut()
        .find(|item| item.id == value.id && item.revision == value.revision)
        .ok_or("官网配置已保存，本地隧道已变化，请刷新")?;
    local.name = name.to_owned();
    local.configured_target = Some(target);
    if let Some(details) = &mut local.remote_details {
        details.created_at = created_at.or(details.created_at);
    }
    local.revision += 1;
    write(&store, &items).map_err(|_| "官网配置已保存，本地更新失败，请刷新隧道")?;
    snapshot(&store, &manager)
}

#[tauri::command]
pub async fn natapp_create_free_tunnel(
    state: State<'_, AppState>,
    manager: State<'_, NatappManager>,
    workspace_id: String,
    value: FreeTunnelInput,
) -> Result<Value, String> {
    let _portal = PORTAL_LOCK.lock().await;
    let generation = current_generation(&state, &workspace_id)?;
    value.target.validate()?;
    let name = value.name.trim();
    if !(1..=16).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err("请输入 1–16 字的隧道名称".into());
    }
    let protocol = value.protocol.value();
    if protocol != "http" && value.target.inspection.is_some() {
        return Err("只有 Web 隧道支持本地流量控制台".into());
    }
    let client = login(&credentials(&workspace_id, None)?).await?;
    let previous = tunnels(&client).await?;
    if previous
        .iter()
        .any(|item| item.name == name && item.protocol_key == protocol)
    {
        return Err("已有同名、同协议的隧道，请先同步列表".into());
    }
    let page = body(
        client
            .get(FREE)
            .send()
            .await
            .map_err(|_| "无法打开 NATAPP 免费隧道表单")?,
    )
    .await?;
    let csrf = form_token(&page, "#buy_form", "")?;
    if current_generation(&state, &workspace_id)? != generation {
        return Err("工作空间已切换，隧道创建已取消".into());
    }
    let result = client
        .post(FREE)
        .header("Origin", ORIGIN)
        .header("Referer", FREE)
        .form(&[
            ("_token", csrf.as_str()),
            ("name", name),
            ("protocol", protocol),
            ("local_port", &value.target.local_port.to_string()),
        ])
        .send()
        .await;
    // 提交可能已到达服务端。只读列表核验，绝不重发创建请求。
    let mut remote = tunnels(&client).await.map_err(|_| UNCERTAIN)?;
    let mut created = remote.iter_mut().filter(|item| {
        item.name == name
            && item.protocol_key == protocol
            && !previous.iter().any(|old| old.hash_id == item.hash_id)
    });
    let Some(created) = created.next().filter(|_| created.next().is_none()) else {
        if result
            .as_ref()
            .is_ok_and(|response| response.status() == StatusCode::UNPROCESSABLE_ENTITY)
        {
            return Err("NATAPP 拒绝了免费隧道配置，请在控制台查看提示".into());
        }
        return Err(UNCERTAIN.into());
    };
    if current_generation(&state, &workspace_id)? != generation {
        return Err("隧道已创建，工作空间已切换，请回到原工作空间同步后核对地址和端口".into());
    }
    let url = format!("{ORIGIN}/tunnel/edit/{}", created.hash_id);
    let target_error = |detail: String| {
        format!("隧道已创建，但配置未确认。请先同步隧道，再打开编辑核对。{detail}")
    };
    let edit = read_edit_form(&client, &url).await.map_err(target_error)?;
    created.created_at = edit.created_at;
    let target = save_target(&client, &url, edit, &value.target)
        .await
        .map_err(target_error)?;
    created.configured_target = Some(target);
    let _op = manager
        .operation
        .lock()
        .map_err(|_| "隧道已创建，请同步到本地")?;
    let store = state.store.lock().map_err(|_| "隧道已创建，请同步到本地")?;
    if store.generation != generation {
        return Err("隧道已创建，工作空间已切换，请回到原工作空间同步".into());
    }
    import(&store, &manager, remote).map_err(|_| "隧道已创建，本地保存失败，请重新同步隧道".into())
}
