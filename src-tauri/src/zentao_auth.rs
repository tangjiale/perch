use crate::{integrations::*, storage::Store};

const MAX_AUTH_RESPONSE: usize = 64 * 1024;

fn base_url(raw: &str) -> Result<String, String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "禅道服务地址无效")?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("禅道地址需为 HTTP/HTTPS 根路径，不能包含凭据、查询参数或片段".into());
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn normalize(connection: &Value, snapshot: &Value) -> Result<Value, String> {
    crate::data::validate(connection, "connections")?;
    let id = text(connection, "id");
    let name = text(connection, "name").trim();
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        || name.len() > 256
    {
        return Err("连接名称或 ID 无效".into());
    }
    let base = base_url(text(connection, "baseUrl"))?;
    let version = text(connection, "apiVersion");
    if !["v1", "v2"].contains(&version) {
        return Err("请选择 REST v1 或 v2".into());
    }
    let previous = snapshot["connections"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| text(row, "id") == id));
    if let Some(old) = previous {
        if old["revision"] != connection["revision"] {
            return Err("连接已变化，请关闭弹框并重新打开".into());
        }
        if base_url(text(old, "baseUrl"))? != base {
            return Err("已有连接不能改为另一服务地址，请新建连接以保留项目关联".into());
        }
    } else if connection["revision"].as_i64().is_some_and(|r| r > 0) {
        return Err("连接已删除，请重新添加".into());
    }
    let mut result = previous.cloned().unwrap_or_else(|| json!({}));
    if let Some(mode) = connection["authMode"].as_str() {
        if !["account", "token"].contains(&mode) {
            return Err("登录方式无效".into());
        }
        result["authMode"] = json!(mode);
    }
    for (field, value) in [
        ("id", json!(id)),
        ("name", json!(name)),
        ("baseUrl", json!(base)),
        ("apiVersion", json!(version)),
        (
            "syncIntervalMinutes",
            json!(crate::zentao::background::interval_minutes(
                connection
                    .get("syncIntervalMinutes")
                    .or_else(|| previous.and_then(|old| old.get("syncIntervalMinutes")))
            )?),
        ),
        (
            "enabled",
            json!(connection["enabled"].as_bool().unwrap_or(true)),
        ),
        (
            "managementEnabled",
            json!(connection["managementEnabled"].as_bool().unwrap_or(false)),
        ),
    ] {
        result[field] = value;
    }
    Ok(result)
}

async fn login_token(
    base: &str,
    account: &str,
    password: &str,
    allow_http: bool,
) -> Result<String, String> {
    let base = base_url(base)?;
    if base.starts_with("http:") && !allow_http {
        return Err("此地址使用 HTTP；请改用 HTTPS，或明确允许通过 HTTP 发送登录凭据".into());
    }
    if account.trim().is_empty()
        || account.len() > 256
        || password.is_empty()
        || password.len() > 4096
        || password.chars().all(|c| matches!(c, '*' | '•' | '●'))
    {
        return Err("请输入有效的禅道账号和密码".into());
    }
    // 22.0 的 Token 入口固定为 v1，与项目同步选择的 API 版本无关。
    let response = http()?
        .post(endpoint(&base, "api.php/v1/tokens")?)
        .timeout(std::time::Duration::from_secs(20))
        .json(&json!({"account":account.trim(),"password":password}))
        .send()
        .await
        .map_err(|_| "无法连接禅道登录接口，请检查地址、网络和证书")?;
    match response.status().as_u16() {
        201 => {}
        400 | 401 | 403 => {
            return Err("禅道拒绝登录，请检查账号密码、账号锁定或登录策略；不会自动重试".into())
        }
        404 | 405 => return Err("未找到禅道 Token 接口，请确认服务根路径及实例版本".into()),
        300..=399 => {
            return Err(
                "登录接口发生跳转；请填写最终服务地址，统一登录页面不能直接代替 REST 登录".into(),
            )
        }
        429 => return Err("登录请求过于频繁，请稍后再试".into()),
        _ => return Err("禅道登录服务返回异常，请稍后重试".into()),
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_AUTH_RESPONSE as u64)
    {
        return Err("禅道登录响应超过大小限制".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "禅道登录响应中断")?;
        if bytes.len() + chunk.len() > MAX_AUTH_RESPONSE {
            return Err("禅道登录响应超过大小限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let response: Value = serde_json::from_slice(&bytes).map_err(|_| "禅道登录未返回有效 JSON")?;
    if ["fail", "failed", "error"].contains(&text(&response, "status")) {
        return Err("禅道登录未成功，请检查账号和登录策略".into());
    }
    validate_token(text(&response, "token"))
}

fn validate_token(raw: &str) -> Result<String, String> {
    let token = raw.trim();
    if token.is_empty()
        || token.len() > 8192
        || reqwest::header::HeaderValue::from_str(token).is_err()
    {
        return Err("禅道访问令牌为空或格式无效".into());
    }
    Ok(token.to_string())
}

pub(crate) trait Secrets {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, secret: Option<&str>) -> Result<(), String>;
}

pub(crate) fn login_credential(
    workspace: &str,
    id: &str,
) -> Result<crate::credential_vault::Entry, String> {
    crate::credential_vault::entry(workspace, "com.self.workbench.zentao-login", id)
        .map_err(|_| "系统钥匙串不可用".into())
}

// 登录密码和失败抑制状态只保存在钥匙串，绝不进入工作空间快照或备份。
#[derive(serde::Serialize, serde::Deserialize)]
struct RememberedLogin {
    base: String,
    account: String,
    password: String,
    allow_http: bool,
    blocked: bool,
}

// 只返回界面恢复所需的布尔信息，密码及授权策略仍以钥匙串为准。
fn login_preferences(connection: &Value, logins: &impl Secrets) -> Result<Value, String> {
    let saved = logins.read()?;
    let login = saved
        .as_deref()
        .map(serde_json::from_str::<RememberedLogin>)
        .transpose()
        .map_err(|_| "保存的登录凭据无效，请重新登录")?;
    let matching = login.as_ref().filter(|login| {
        connection["rememberCredentials"] == true
            && login.base == text(connection, "baseUrl")
            && login.account == text(connection, "loginAccount")
            && !login.password.is_empty()
    });
    Ok(json!({
        "hasSavedPassword": matching.is_some(),
        "allowInsecureHttp": matching.is_some_and(|login| login.allow_http)
    }))
}

#[tauri::command]
pub fn zentao_auth_preferences(
    state: State<'_, AppState>,
    workspace_id: String,
    connection_id: String,
) -> Result<Value, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace_id {
        return Err("工作空间已切换，请重新打开连接设置".into());
    }
    let snapshot = store.snapshot()?;
    let connection = snapshot["connections"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| text(row, "id") == connection_id))
        .ok_or("禅道连接不存在")?;
    let logins = login_credential(&workspace_id, &connection_id)?;
    login_preferences(connection, &logins)
}

pub(crate) struct AutoAuth<'a> {
    state: &'a AppState,
    workspace: String,
    generation: std::path::PathBuf,
    connection: Value,
    logins: Box<dyn Secrets + Send + Sync>,
    tokens: Box<dyn Secrets + Send + Sync>,
}

impl<'a> AutoAuth<'a> {
    #[cfg(test)]
    pub(crate) fn for_test(
        state: &'a AppState,
        connection: &Value,
        logins: Box<dyn Secrets + Send + Sync>,
        tokens: Box<dyn Secrets + Send + Sync>,
    ) -> Self {
        let store = state.store.lock().unwrap();
        Self {
            state,
            workspace: store.workspace_id.clone(),
            generation: store.generation.clone(),
            connection: connection.clone(),
            logins,
            tokens,
        }
    }
    pub(crate) fn verify_account(&self, account: &str) -> Result<(), String> {
        if self.connection["rememberCredentials"] == true
            && text(&self.connection, "loginAccount") != account
        {
            return Err("当前令牌账号与保存的登录账号不一致，请在设置中重新登录".into());
        }
        Ok(())
    }
    pub(crate) fn new(
        state: &'a AppState,
        workspace: &str,
        generation: &Path,
        connection: &Value,
    ) -> Result<Self, String> {
        Ok(Self {
            state,
            workspace: workspace.into(),
            generation: generation.into(),
            connection: connection.clone(),
            logins: Box::new(login_credential(workspace, text(connection, "id"))?),
            tokens: Box::new(credential(workspace, text(connection, "id"))?),
        })
    }

    fn check(&self, store: &Store) -> Result<(), String> {
        if store.workspace_id != self.workspace || store.generation != self.generation {
            return Err("工作空间已切换，请重新操作禅道连接".into());
        }
        let current = get(
            &store.snapshot()?,
            "connections",
            text(&self.connection, "id"),
        )?;
        if current["revision"] != self.connection["revision"] {
            return Err("禅道连接已变化，请重新操作".into());
        }
        Ok(())
    }

    pub(crate) async fn refresh(&self) -> Result<String, String> {
        if self.connection["rememberCredentials"] != true {
            return Err(
                "禅道认证失败（HTTP 401）：请重新登录；可开启记住登录凭据以自动更新令牌".into(),
            );
        }
        let mut login = {
            let store = self.state.store.lock().map_err(|e| e.to_string())?;
            self.check(&store)?;
            prepare_refresh(self.logins.as_ref(), &self.connection)?
        };
        let token = login_token(
            &login.base,
            &login.account,
            &login.password,
            login.allow_http,
        )
        .await
        .map_err(|error| {
            format!("禅道自动登录失败，已暂停自动重试；请在设置中重新登录：{error}")
        })?;
        verify_login_account(&login.base, &token, &login.account).await?;
        let store = self.state.store.lock().map_err(|e| e.to_string())?;
        self.check(&store)?;
        self.tokens.write(Some(&token))?;
        login.blocked = false;
        self.logins.write(Some(
            &serde_json::to_string(&login).map_err(|_| "无法保存登录状态")?,
        ))?;
        Ok(token)
    }

    pub(crate) fn block(&self) -> Result<(), String> {
        let store = self.state.store.lock().map_err(|e| e.to_string())?;
        self.check(&store)?;
        if self.connection["rememberCredentials"] == true {
            if let Some(raw) = self.logins.read()? {
                let mut login: RememberedLogin =
                    serde_json::from_str(&raw).map_err(|_| "保存的登录凭据无效，请重新登录")?;
                login.blocked = true;
                self.logins.write(Some(
                    &serde_json::to_string(&login).map_err(|_| "无法保存登录状态")?,
                ))?;
            }
        }
        Ok(())
    }
}

fn prepare_refresh(
    entry: &(impl Secrets + ?Sized),
    connection: &Value,
) -> Result<RememberedLogin, String> {
    let raw = entry
        .read()?
        .ok_or("未找到保存的登录凭据，请重新登录并开启记住登录凭据")?;
    let mut login: RememberedLogin =
        serde_json::from_str(&raw).map_err(|_| "保存的登录凭据无效，请重新登录")?;
    if login.base != base_url(text(connection, "baseUrl"))?
        || login.account != text(connection, "loginAccount")
    {
        return Err("保存的登录凭据与当前连接不匹配，请重新登录".into());
    }
    if login.blocked {
        return Err("禅道自动登录已暂停，避免反复登录锁定账号；请在设置中重新登录".into());
    }
    // 先持久化一次尝试标记，网络失败或应用退出后也不会反复提交密码。
    login.blocked = true;
    entry.write(Some(
        &serde_json::to_string(&login).map_err(|_| "无法保存登录状态")?,
    ))?;
    Ok(login)
}

async fn verify_login_account(base: &str, token: &str, account: &str) -> Result<(), String> {
    let response = http()?
        .get(endpoint(base, "api.php/v1/user")?)
        .header("Token", token)
        .send()
        .await
        .map_err(|_| "无法核验重新登录的账号，请在设置中重新登录")?;
    if !response.status().is_success() {
        return Err("重新登录的令牌未通过账号核验，请在设置中重新登录".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "账号核验响应中断")?;
        if bytes.len() + chunk.len() > MAX_AUTH_RESPONSE {
            return Err("账号核验响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| "账号核验响应无效")?;
    if body["profile"]["account"] != account {
        return Err("重新登录的账号不匹配，请在设置中重新登录".into());
    }
    Ok(())
}
impl Secrets for crate::credential_vault::Entry {
    fn read(&self) -> Result<Option<String>, String> {
        match self.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取系统钥匙串，请检查访问权限".into()),
        }
    }
    fn write(&self, secret: Option<&str>) -> Result<(), String> {
        match secret {
            Some(secret) => self
                .set_password(secret)
                .map_err(|_| "无法保存禅道令牌".into()),
            None => match self.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err("无法清除禅道令牌".into()),
            },
        }
    }
}

fn save_connection(
    store: &Store,
    mut connection: Value,
    secret: Option<&str>,
    secrets: &impl Secrets,
) -> Result<Value, String> {
    let previous = secrets.read()?;
    let next = secret
        .or(previous.as_deref())
        .ok_or("请通过账号登录或填写访问令牌")?;
    validate_token(next)?;
    connection["hasCredential"] = json!(true);
    if secret.is_some() {
        secrets.write(Some(next))?;
    }
    match store.save("connection", connection) {
        Ok(saved) => Ok(saved),
        Err(error) => {
            // SQLite 与钥匙串无法共用事务；数据库失败时恢复原凭据。
            if secret.is_some() && secrets.write(previous.as_deref()).is_err() {
                return Err("连接未保存，且原令牌恢复失败；请重新登录此连接".into());
            }
            Err(error)
        }
    }
}

#[tauri::command]
// 保持已发布的平铺 IPC 参数；新增选项不改变旧客户端的登录调用。
#[allow(clippy::too_many_arguments)]
pub async fn zentao_connect(
    state: State<'_, AppState>,
    connection: Value,
    workspace_id: String,
    account: Option<String>,
    password: Option<String>,
    token: Option<String>,
    allow_insecure_http: Option<bool>,
    remember_credentials: Option<bool>,
) -> Result<Value, String> {
    let (connection, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace_id {
            return Err("工作空间已切换，请重新打开连接设置".into());
        }
        (
            normalize(&connection, &store.snapshot()?)?,
            store.generation.clone(),
        )
    };
    let explicit_login = account.is_some();
    let login_account = account.as_deref().map(str::trim).map(str::to_owned);
    let explicit_token = token.as_ref().is_some_and(|token| !token.trim().is_empty());
    let remember = remember_credentials.unwrap_or(connection["rememberCredentials"] == true)
        && !explicit_token;
    let remembered = if remember && explicit_login {
        Some(RememberedLogin {
            base: text(&connection, "baseUrl").into(),
            account: account.as_deref().unwrap_or_default().trim().into(),
            password: password.clone().unwrap_or_default(),
            allow_http: allow_insecure_http.unwrap_or(false),
            blocked: false,
        })
    } else {
        None
    };
    let secret = match (account, password, token) {
        (Some(account), Some(password), None) => Some(
            login_token(
                text(&connection, "baseUrl"),
                &account,
                &password,
                allow_insecure_http.unwrap_or(false),
            )
            .await?,
        ),
        (None, None, token) => token
            .filter(|v| !v.trim().is_empty())
            .map(|v| validate_token(&v))
            .transpose()?,
        _ => return Err("请选择账号登录或访问令牌中的一种认证方式".into()),
    };
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace_id || store.generation != generation {
        return Err("数据目录已切换，请重新打开连接设置".into());
    }
    let mut connection = normalize(&connection, &store.snapshot()?)?;
    if let Some(account) = login_account {
        connection["authMode"] = json!("account");
        connection["loginAccount"] = json!(account);
    } else if explicit_token {
        connection["authMode"] = json!("token");
    }
    let entry = credential(&workspace_id, text(&connection, "id"))?;
    let login_entry = login_credential(&workspace_id, text(&connection, "id"))?;
    save_with_login(
        &store,
        &mut connection,
        secret.as_deref(),
        remember,
        remembered,
        &entry,
        &login_entry,
    )
}

pub(crate) fn save_existing(state: State<AppState>, value: Value) -> Result<Value, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut connection = normalize(&value, &store.snapshot()?)?;
    let tokens = credential(&store.workspace_id, text(&connection, "id"))?;
    let logins = login_credential(&store.workspace_id, text(&connection, "id"))?;
    let remember = value["rememberCredentials"] == true;
    save_with_login(
        &store,
        &mut connection,
        None,
        remember,
        None,
        &tokens,
        &logins,
    )
}

pub(crate) fn delete_connection(
    state: State<AppState>,
    id: String,
    revision: Option<i64>,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let tokens = credential(&store.workspace_id, &id)?;
    let logins = login_credential(&store.workspace_id, &id)?;
    let token = tokens.read()?;
    let login = logins.read()?;
    logins.write(None)?;
    if let Err(error) = tokens.write(None) {
        logins.write(login.as_deref())?;
        return Err(error);
    }
    if let Err(error) = store.delete("connection", &id, revision) {
        tokens.write(token.as_deref())?;
        logins.write(login.as_deref())?;
        return Err(error);
    }
    Ok(())
}

fn save_with_login(
    store: &Store,
    connection: &mut Value,
    token: Option<&str>,
    remember: bool,
    remembered: Option<RememberedLogin>,
    tokens: &impl Secrets,
    logins: &impl Secrets,
) -> Result<Value, String> {
    let previous = logins.read()?;
    let next = if remember {
        if let Some(login) = remembered {
            connection["loginAccount"] = json!(login.account);
            Some(serde_json::to_string(&login).map_err(|_| "无法保存登录凭据")?)
        } else {
            let raw = previous
                .as_deref()
                .ok_or("请先使用账号密码登录，再开启记住登录凭据")?;
            let login: RememberedLogin =
                serde_json::from_str(raw).map_err(|_| "保存的登录凭据无效，请重新登录")?;
            if login.base != text(connection, "baseUrl")
                || login.account != text(connection, "loginAccount")
            {
                return Err("登录凭据与当前连接不匹配，请重新登录".into());
            }
            Some(raw.to_owned())
        }
    } else {
        // 账号是非秘密表单偏好；关闭记住凭据仅清理真正的密码。
        None
    };
    connection["rememberCredentials"] = json!(remember);
    logins.write(next.as_deref())?;
    match save_connection(store, connection.clone(), token, tokens) {
        Ok(saved) => Ok(saved),
        Err(error) => {
            if logins.write(previous.as_deref()).is_err() {
                return Err("连接未保存且登录凭据恢复失败，请重新登录".into());
            }
            Err(error)
        }
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct TestSecrets(pub std::sync::Arc<Mutex<Option<String>>>);
#[cfg(test)]
impl Secrets for TestSecrets {
    fn read(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, secret: Option<&str>) -> Result<(), String> {
        *self.0.lock().unwrap() = secret.map(str::to_owned);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn automatic_sync_connection_normalization_preserves_saved_frequency() {
        let mut connection = draft();
        let empty = json!({"connections": []});
        assert_eq!(
            normalize(&connection, &empty).unwrap()["syncIntervalMinutes"],
            5
        );
        connection["syncIntervalMinutes"] = json!(15);
        let saved = normalize(&connection, &empty).unwrap();
        assert_eq!(saved["syncIntervalMinutes"], 15);
        connection
            .as_object_mut()
            .unwrap()
            .remove("syncIntervalMinutes");
        assert_eq!(
            normalize(&connection, &json!({"connections": [saved]})).unwrap()
                ["syncIntervalMinutes"],
            15
        );
        connection["syncIntervalMinutes"] = json!(3);
        assert!(normalize(&connection, &empty).is_err());
    }

    fn server(status: u16, body: String) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/zentao", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = vec![];
            loop {
                let mut buf = [0; 2048];
                let count = socket.read(&mut buf).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..count]);
                if let Some(index) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..index]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= index + 4 + length {
                        break;
                    }
                }
            }
            let _ = write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: /sso\r\nConnection: close\r\n\r\n{body}", body.len());
            request
        });
        (base, handle)
    }

    #[tokio::test]
    async fn official_login_uses_fixed_v1_post_and_json_credentials() {
        let (base, handle) = server(201, json!({"token":"fixture-token"}).to_string());
        assert_eq!(
            login_token(&base, " tester ", "fixture-password", true)
                .await
                .unwrap(),
            "fixture-token"
        );
        let request = handle.join().unwrap();
        let split = request.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
        let headers = String::from_utf8_lossy(&request[..split]);
        assert!(headers.starts_with("POST /zentao/api.php/v1/tokens HTTP/1.1"));
        assert!(!headers.contains("fixture-password"));
        let body: Value = serde_json::from_slice(&request[split + 4..]).unwrap();
        assert_eq!(
            body,
            json!({"account":"tester","password":"fixture-password"})
        );
    }

    #[tokio::test]
    async fn authentication_failures_are_bounded_and_do_not_echo_remote_secrets() {
        for (status, body, expected) in [
            (400, "fixture-password".into(), "拒绝登录"),
            (302, "".into(), "跳转"),
            (404, "".into(), "未找到"),
            (429, "".into(), "频繁"),
            (201, "<html>fixture-password</html>".into(), "JSON"),
            (
                201,
                json!({"status":"fail","token":"fixture-token"}).to_string(),
                "未成功",
            ),
            (201, json!({"token":""}).to_string(), "令牌"),
            (201, "x".repeat(MAX_AUTH_RESPONSE + 1), "大小限制"),
        ] {
            let (base, handle) = server(status, body);
            let error = login_token(&base, "tester", "fixture-password", true)
                .await
                .unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("fixture-password") && !error.contains("fixture-token"));
            handle.join().unwrap();
        }
        assert!(
            login_token("http://127.0.0.1:1", "tester", "password", false)
                .await
                .unwrap_err()
                .contains("HTTP")
        );
        assert!(base_url("https://user:pass@example.com").is_err());
        assert!(base_url("https://example.com?token=secret").is_err());
        assert!(validate_token("a\r\nb").is_err());
    }

    struct MemorySecrets {
        value: RefCell<Option<String>>,
        fail: bool,
    }
    impl Secrets for MemorySecrets {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.value.borrow().clone())
        }
        fn write(&self, secret: Option<&str>) -> Result<(), String> {
            if self.fail {
                return Err("模拟钥匙串故障".into());
            }
            *self.value.borrow_mut() = secret.map(str::to_string);
            Ok(())
        }
    }
    fn draft() -> Value {
        json!({"id":"test-connection","name":"测试禅道","baseUrl":"https://example.test/zentao/","apiVersion":"v2","enabled":true,"managementEnabled":false})
    }
    #[test]
    fn connection_and_credentials_preserve_previous_state_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let secrets = MemorySecrets {
            value: RefCell::new(None),
            fail: false,
        };
        let connection = normalize(&draft(), &store.snapshot().unwrap()).unwrap();
        assert!(save_connection(&store, connection.clone(), None, &secrets).is_err());
        let saved = save_connection(
            &store,
            connection.clone(),
            Some("fixture-token-old"),
            &secrets,
        )
        .unwrap();
        assert_eq!(saved["hasCredential"], true);
        assert!(save_connection(&store, connection, Some("fixture-token-new"), &secrets).is_err());
        assert_eq!(
            secrets.read().unwrap().as_deref(),
            Some("fixture-token-old")
        );
        let updated = save_connection(&store, saved, None, &secrets).unwrap();
        assert_eq!(updated["revision"], 2);
        assert!(!store
            .snapshot()
            .unwrap()
            .to_string()
            .contains("fixture-token"));
        let failing = MemorySecrets {
            value: RefCell::new(None),
            fail: true,
        };
        let mut new = draft();
        new["id"] = json!("another");
        assert!(save_connection(&store, new, Some("fixture-token"), &failing).is_err());
        assert_eq!(
            store.snapshot().unwrap()["connections"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn remembered_password_is_private_and_disable_clears_only_login_secret() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let tokens = TestSecrets::default();
        let logins = TestSecrets::default();
        let mut connection = normalize(&draft(), &store.snapshot().unwrap()).unwrap();
        let remembered = RememberedLogin {
            base: text(&connection, "baseUrl").into(),
            account: "tester".into(),
            password: "fixture-password".into(),
            allow_http: false,
            blocked: false,
        };
        let saved = save_with_login(
            &store,
            &mut connection,
            Some("fixture-token"),
            true,
            Some(remembered),
            &tokens,
            &logins,
        )
        .unwrap();
        assert_eq!(saved["rememberCredentials"], true);
        assert_eq!(saved["loginAccount"], "tester");
        assert!(!store
            .snapshot()
            .unwrap()
            .to_string()
            .contains("fixture-password"));
        // 冲突保存必须恢复原密码和令牌。
        let before = logins.read().unwrap();
        assert!(save_with_login(
            &store,
            &mut connection,
            Some("replacement-token"),
            false,
            None,
            &tokens,
            &logins
        )
        .is_err());
        assert_eq!(logins.read().unwrap(), before);
        assert_eq!(tokens.read().unwrap().as_deref(), Some("fixture-token"));
        let mut saved = saved;
        let disabled =
            save_with_login(&store, &mut saved, None, false, None, &tokens, &logins).unwrap();
        assert_eq!(disabled["rememberCredentials"], false);
        assert_eq!(disabled["loginAccount"], "tester");
        assert!(logins.read().unwrap().is_none());
        assert_eq!(tokens.read().unwrap().as_deref(), Some("fixture-token"));
    }

    #[test]
    fn preferences_only_expose_matching_saved_password_presence_and_http_consent() {
        let login = RememberedLogin {
            base: "http://example.test".into(),
            account: "tester".into(),
            password: "fixture-password".into(),
            allow_http: true,
            blocked: false,
        };
        let logins = TestSecrets::default();
        logins
            .write(Some(&serde_json::to_string(&login).unwrap()))
            .unwrap();
        let mut connection = json!({"baseUrl":"http://example.test","loginAccount":"tester","rememberCredentials":true});
        let preferences = login_preferences(&connection, &logins).unwrap();
        assert_eq!(
            preferences,
            json!({"hasSavedPassword":true,"allowInsecureHttp":true})
        );
        assert!(!preferences.to_string().contains("fixture-password"));
        connection["loginAccount"] = json!("other-user");
        assert_eq!(
            login_preferences(&connection, &logins).unwrap(),
            json!({"hasSavedPassword":false,"allowInsecureHttp":false})
        );
        connection["loginAccount"] = json!("tester");
        connection["rememberCredentials"] = json!(false);
        assert_eq!(
            login_preferences(&connection, &logins).unwrap()["hasSavedPassword"],
            false
        );
    }

    #[test]
    fn ordinary_save_preserves_saved_password_and_private_http_consent() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let tokens = TestSecrets::default();
        let logins = TestSecrets::default();
        let mut connection = normalize(&draft(), &store.snapshot().unwrap()).unwrap();
        connection["authMode"] = json!("account");
        let remembered = RememberedLogin {
            base: text(&connection, "baseUrl").into(),
            account: "tester".into(),
            password: "fixture-password".into(),
            allow_http: true,
            blocked: false,
        };
        let mut saved = save_with_login(
            &store,
            &mut connection,
            Some("fixture-token"),
            true,
            Some(remembered),
            &tokens,
            &logins,
        )
        .unwrap();
        let previous = logins.read().unwrap();
        saved["name"] = json!("新名称");
        let saved =
            save_with_login(&store, &mut saved, None, true, None, &tokens, &logins).unwrap();
        assert_eq!(saved["authMode"], "account");
        assert_eq!(saved["loginAccount"], "tester");
        assert_eq!(logins.read().unwrap(), previous);
        assert_eq!(tokens.read().unwrap().as_deref(), Some("fixture-token"));
        assert!(saved.get("allowInsecureHttp").is_none());
        assert!(!saved.to_string().contains("fixture-password"));
    }

    #[test]
    fn refresh_rejects_different_base_account_and_previous_failure() {
        let logins = TestSecrets::default();
        let raw = json!({"base":"https://example.test","account":"tester","password":"fixture-password","allow_http":false,"blocked":false}).to_string();
        logins.write(Some(&raw)).unwrap();
        assert!(prepare_refresh(
            &logins,
            &json!({"baseUrl":"https://other.test","loginAccount":"tester"})
        )
        .is_err());
        assert!(prepare_refresh(
            &logins,
            &json!({"baseUrl":"https://example.test","loginAccount":"other"})
        )
        .is_err());
        let connection = json!({"baseUrl":"https://example.test","loginAccount":"tester"});
        assert!(prepare_refresh(&logins, &connection).is_ok());
        assert!(prepare_refresh(&logins, &connection)
            .err()
            .unwrap()
            .contains("暂停"));
    }

    #[test]
    fn changed_address_revision_and_secret_fields_are_rejected() {
        let saved = json!({"id":"test-connection","name":"原连接","baseUrl":"https://example.test/zentao","apiVersion":"v1","revision":3});
        let snapshot = json!({"connections":[saved.clone()]});
        assert!(normalize(&draft(), &snapshot).is_err());
        let mut changed = saved.clone();
        changed["baseUrl"] = json!("https://another.test");
        assert!(normalize(&changed, &snapshot)
            .unwrap_err()
            .contains("新建连接"));
        changed = saved;
        changed["password"] = json!("fixture-password");
        assert!(normalize(&changed, &snapshot).is_err());
        let mut missing = draft();
        missing["revision"] = json!(2);
        assert!(normalize(&missing, &json!({"connections":[]})).is_err());
    }
}
