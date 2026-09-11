use crate::integrations::*;
use std::collections::HashSet;
pub mod background;
mod board;
pub mod bugs;
mod create;
mod description;
pub mod media;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 1_000;
async fn checked_zentao(response: reqwest::Response) -> Result<reqwest::Response, String> {
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err("禅道认证失败（HTTP 401）：访问令牌已失效或被服务器拒绝。请进入「设置 → 禅道连接」编辑原连接，重新账号登录或更新访问令牌，然后同步后重试。".into()),
        403 => Err("禅道权限不足（HTTP 403）：请联系管理员确认当前账号对该项目、执行或 BUG 的查看与操作权限，然后同步后重试。".into()),
        status => Err(format!("禅道请求失败（HTTP {status}），请检查连接与服务器；若正在保存，请先同步核对结果，勿直接重复提交。")),
    }
}
static SYNCING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
struct SyncGuard(String);
impl SyncGuard {
    fn acquire(workspace: &str, connection: &str) -> Result<Self, String> {
        let id = format!("{workspace}:{connection}");
        if !SYNCING
            .get_or_init(Default::default)
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id.clone())
        {
            return Err("该禅道连接正在同步，请等待当前同步完成".into());
        }
        Ok(Self(id))
    }
}
impl Drop for SyncGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = SYNCING.get_or_init(Default::default).lock() {
            active.remove(&self.0);
        }
    }
}
struct Adapter<'a> {
    base: String,
    version: String,
    key: Mutex<String>,
    client: reqwest::Client,
    personal: bool,
    auth: Option<crate::zentao_auth::AutoAuth<'a>>,
    refreshed: std::sync::atomic::AtomicBool,
}
fn remote_id(value: &Value) -> Result<String, String> {
    let id = match value {
        Value::String(id) => id.clone(),
        Value::Number(id) => id.to_string(),
        _ => return Err("禅道返回了无效对象 ID".into()),
    };
    if id.is_empty() || id == "0" || !id.bytes().all(|c| c.is_ascii_digit()) {
        return Err("禅道对象 ID 必须是正整数".into());
    }
    let normalized = id.trim_start_matches('0');
    if normalized.is_empty() {
        return Err("禅道对象 ID 必须是正整数".into());
    }
    Ok(normalized.to_owned())
}
fn response_rows(body: &Value, resource: &str) -> Result<Vec<Value>, String> {
    if body["status"] == "fail" || body["status"] == "failed" || body["status"] == "error" {
        return Err("禅道拒绝读取，请检查令牌、权限及接口版本".into());
    }
    body[resource]
        .as_array()
        .or_else(|| body["data"][resource].as_array())
        .cloned()
        .ok_or_else(|| format!("禅道接口缺少 {resource} 列表，请核验当前实例 API 能力"))
}

// 禅道越界页会重置到第一页，不能将“读到空页”作为官方分页的结束条件。
// v1 使用 page/total/limit；v2 使用 pager.pageID/pageTotal/recTotal/recPerPage。
fn response_has_more(
    body: &Value,
    resource: &str,
    requested: usize,
) -> Result<Option<bool>, String> {
    let payload = if body.get(resource).is_some() {
        body
    } else {
        &body["data"]
    };
    let invalid = || format!("禅道 {resource} 分页信息无效，本次同步未发布");
    let number = |value: &Value| -> Result<u64, String> {
        value
            .as_u64()
            .or_else(|| value.as_str()?.parse().ok())
            .ok_or_else(invalid)
    };
    let (current, pages) =
        if let Some(pager) = payload.get("pager").filter(|value| !value.is_null()) {
            let current = number(&pager["pageID"])?;
            let pages = if let Some(pages) = pager.get("pageTotal") {
                number(pages)?
            } else {
                let size = number(&pager["recPerPage"])?;
                if size == 0 {
                    return Err(invalid());
                }
                number(&pager["recTotal"])?.div_ceil(size)
            };
            (current, pages)
        } else if ["page", "total", "limit"]
            .iter()
            .any(|field| !payload[*field].is_null())
        {
            let size = number(&payload["limit"])?;
            if size == 0 {
                return Err(invalid());
            }
            (
                number(&payload["page"])?,
                number(&payload["total"])?.div_ceil(size),
            )
        } else {
            // 无分页元数据的兼容接口仍读到空页，并保留重复检测，避免悄悄漏数据。
            return Ok(None);
        };
    if current != requested as u64 || (pages > 0 && current > pages) {
        return Err(format!(
            "禅道 {resource} 返回页码与请求不一致，请检查接口分页支持后重试"
        ));
    }
    Ok(Some(current < pages))
}
impl<'a> Adapter<'a> {
    fn new(connection: &Value, key: String) -> Result<Self, String> {
        let version = text(connection, "apiVersion");
        if !["v1", "v2"].contains(&version) {
            return Err("请选择实例实际支持的 API v1 或 v2；22.0 不默认具有 v2 能力".into());
        }
        let base = endpoint(
            text(connection, "baseUrl").trim_end_matches('/'),
            &format!("api.php/{version}"),
        )?
        .to_string();
        Ok(Self {
            base,
            version: version.into(),
            key: Mutex::new(key),
            client: http()?,
            personal: false,
            auth: None,
            refreshed: std::sync::atomic::AtomicBool::new(false),
        })
    }
    fn personal(connection: &Value, key: String) -> Result<Self, String> {
        // 22.0 v1 明确支持 involved；不能把 v2 忽略筛选参数后的可见项目当成个人项目。
        let mut config = connection.clone();
        config["apiVersion"] = json!("v1");
        let mut adapter = Self::new(&config, key)?;
        adapter.personal = true;
        Ok(adapter)
    }
    fn with_auth(mut self, auth: crate::zentao_auth::AutoAuth<'a>) -> Self {
        self.auth = Some(auth);
        self
    }
    fn token(&self) -> Result<String, String> {
        self.key
            .lock()
            .map(|key| key.clone())
            .map_err(|_| "无法读取禅道认证状态".into())
    }
    async fn read(&self, url: url::Url) -> Result<reqwest::Response, String> {
        let response = self
            .client
            .get(url.clone())
            .header("Token", self.token()?)
            .send()
            .await
            .map_err(|_| "禅道连接失败，本次操作未提交")?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return checked_zentao(response).await;
        }
        let Some(auth) = &self.auth else {
            return checked_zentao(response).await;
        };
        if self
            .refreshed
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            auth.block()?;
            return Err("禅道认证失败（HTTP 401），已暂停自动登录；请在设置中重新登录".into());
        }
        let token = auth.refresh().await?;
        *self.key.lock().map_err(|_| "无法更新禅道认证状态")? = token;
        let response = self
            .client
            .get(url)
            .header("Token", self.token()?)
            .send()
            .await
            .map_err(|_| "禅道重试读取失败，请稍后重试")?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            auth.block()?;
        }
        checked_zentao(response).await
    }
    async fn account(&self) -> Result<String, String> {
        let response = self.read(endpoint(&self.base, "user")?).await?;
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "禅道账号信息读取失败")?;
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("禅道账号响应过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes).map_err(|_| "禅道账号响应无效")?;
        let account = body["profile"]["account"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 256 && *s != "guest")
            .ok_or("无法识别令牌所属账号，请重新配置禅道连接")?;
        if let Some(auth) = &self.auth {
            auth.verify_account(account)?;
        }
        Ok(account.to_owned())
    }
    async fn all(&self, path: &str, resource: &str) -> Result<Vec<Value>, String> {
        self.read_pages(path, resource, false).await
    }
    async fn read_pages(
        &self,
        path: &str,
        resource: &str,
        single_execution: bool,
    ) -> Result<Vec<Value>, String> {
        let mut output = vec![];
        let mut ids = HashSet::new();
        for page in 1..=MAX_PAGES {
            let mut url = endpoint(&self.base, path)?;
            {
                let mut query = url.query_pairs_mut();
                if self.personal && resource == "projects" {
                    query.append_pair("involved", "1");
                }
                if self.personal && resource == "executions" {
                    query.append_pair("fields", "PM,desc");
                }
                let (page_name, size_name) = if self.version == "v2" {
                    ("pageID", "recPerPage")
                } else {
                    ("page", "limit")
                };
                query
                    .append_pair(page_name, &page.to_string())
                    .append_pair(size_name, &PAGE_SIZE.to_string())
                    .append_pair(
                        if self.version == "v2" {
                            "orderBy"
                        } else {
                            "order"
                        },
                        "id_asc",
                    );
                query.append_pair(
                    if self.version == "v2" && resource == "projects" {
                        "browseType"
                    } else {
                        "status"
                    },
                    "all",
                );
            }
            let response = self.read(url).await?;
            if response
                .content_length()
                .is_some_and(|n| n > 16 * 1024 * 1024)
            {
                return Err("禅道单页响应超过 16 MB 限制".into());
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| "禅道响应中断，本次同步未发布")?;
                if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                    return Err("禅道单页响应超过 16 MB 限制".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let body: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "禅道响应不是有效 JSON，请核验 API 版本")?;
            let rows = response_rows(&body, resource)?;
            let more = response_has_more(&body, resource, page)?;
            let single_page = single_execution && page == 1 && rows.len() == 1 && more.is_none();
            if rows.is_empty() {
                if more == Some(true) {
                    return Err(format!(
                        "禅道 {resource} 在最后一页前返回空列表，本次同步未发布"
                    ));
                }
                return Ok(output);
            }
            for row in rows {
                if !ids.insert(remote_id(&row["id"])?) {
                    return Err(format!(
                        "禅道 {resource} 分页返回重复对象，请核验分页协议后重试"
                    ));
                }
                output.push(row);
            }
            if more == Some(false) || single_page {
                return Ok(output);
            }
        }
        Err(format!("禅道 {resource} 超过分页上限，本次同步未发布"))
    }
}
#[derive(Default)]
struct RemoteSnapshot {
    projects: Vec<Value>,
    items: Vec<(String, String, Value)>,
}
#[cfg(test)]
async fn fetch(adapter: &Adapter<'_>) -> Result<RemoteSnapshot, String> {
    fetch_for_account(adapter, None).await
}
async fn fetch_for_account(
    adapter: &Adapter<'_>,
    account: Option<&str>,
) -> Result<RemoteSnapshot, String> {
    let projects = adapter.all("projects", "projects").await?;
    let mut result = RemoteSnapshot {
        projects,
        ..Default::default()
    };
    let mut external = HashSet::new();
    let mut execution_catalog: Option<Vec<Value>> = if account.is_some() {
        Some(adapter.all("executions", "executions").await?)
    } else {
        None
    };
    for project in &result.projects {
        let pid = remote_id(&project["id"])?;
        // 不启用多执行的项目只返回一个默认执行，22.0 此分支没有创建 pager。
        let single_execution = matches!(&project["multiple"], Value::Bool(false))
            || project["multiple"] == 0
            || project["multiple"] == "0";
        let mut executions = if account.is_some() {
            matching_parent(
                execution_catalog.as_deref().unwrap_or_default(),
                "project",
                &pid,
            )?
        } else {
            adapter
                .read_pages(
                    &format!("projects/{pid}/executions"),
                    "executions",
                    single_execution,
                )
                .await?
        };
        if let Some(conflict) = executions
            .iter()
            .find_map(|row| validate_parent(row, "project", &pid).err())
        {
            if execution_catalog.is_none() {
                execution_catalog = Some(adapter.all("executions", "executions").await.map_err(|error| format!("项目 #{pid} 的执行列表归属不一致；独立列表核验失败：{error}；{conflict}"))?);
            }
            executions = matching_parent(
                execution_catalog.as_deref().unwrap_or_default(),
                "project",
                &pid,
            )?;
            if executions.is_empty() {
                return Err(format!("项目 #{pid} 的执行列表返回了其他项目数据，独立列表未找到可核验的执行；{conflict}。请检查项目权限或 API 版本"));
            }
        }
        for execution in executions {
            let eid = remote_id(&execution["id"])?;
            validate_parent(&execution, "project", &pid)?;
            if let Some(account) = account {
                if !responsible_for(&execution, account)? {
                    continue;
                }
            }
            if !external.insert(format!("execution:{eid}")) {
                return Err("执行在多个项目中重复，请重新同步".into());
            }
            result
                .items
                .push((pid.clone(), "execution".into(), execution));
        }
    }
    Ok(result)
}
fn responsible_for(execution: &Value, account: &str) -> Result<bool, String> {
    let pm = execution
        .get("PM")
        .ok_or("禅道执行响应缺少负责人 PM，无法安全筛选个人执行")?;
    if pm.is_null() {
        return Ok(false);
    }
    let owner = if pm.is_object() {
        pm["account"].as_str()
    } else {
        pm.as_str()
    };
    let owner = owner.ok_or("禅道执行负责人格式无效，无法安全筛选")?;
    Ok(owner == account)
}
fn validate_parent(row: &Value, field: &str, expected: &str) -> Result<(), String> {
    let value = &row[field];
    if value.is_null() {
        return Ok(());
    }
    let id = if value.is_object() {
        remote_id(&value["id"])?
    } else {
        remote_id(value)?
    };
    if id != expected {
        let object = remote_id(&row["id"]).unwrap_or_else(|_| "未知".into());
        let relation = if field == "execution" {
            "执行"
        } else {
            "项目"
        };
        return Err(format!("禅道对象 #{object} 的{relation}归属不一致：请求 #{expected}，返回 #{id}；本次同步未写入"));
    }
    Ok(())
}
fn matching_parent(rows: &[Value], field: &str, expected: &str) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    for row in rows {
        // 独立目录不能靠缺省字段推断关系，必须存在明确的关联 ID。
        let value = &row[field];
        if value.is_null() {
            return Err(format!("禅道独立目录缺少 {field} 关联，无法安全核验"));
        }
        let id = remote_id(if value.is_object() {
            &value["id"]
        } else {
            value
        })?;
        if id == expected {
            output.push(row.clone());
        }
    }
    Ok(output)
}
fn existing<'a>(
    snapshot: &'a Value,
    list: &str,
    connection: &str,
    kind: &str,
    id: &str,
) -> Option<&'a Value> {
    snapshot[list].as_array()?.iter().find(|row| {
        text(row, "source") == "zentao"
            && text(row, "connectionId") == connection
            && text(row, "remoteId") == id
            && (kind == "project" || text(row, "remoteType") == kind)
    })
}
fn owner_identity(value: &Value) -> (String, String) {
    if let Some(account) = value.as_str() {
        return (account.to_owned(), account.to_owned());
    }
    let account = text(value, "account").to_owned();
    let name = text(value, "realname");
    (
        account.clone(),
        if name.is_empty() {
            account
        } else {
            name.to_owned()
        },
    )
}
fn merge(
    snapshot: &Value,
    connection: &str,
    remote: RemoteSnapshot,
) -> Result<Vec<(&'static str, Value)>, String> {
    let mut changes = vec![];
    let mut project_ids = HashMap::new();
    for project in remote.projects {
        let id = remote_id(&project["id"])?;
        let mut value = existing(snapshot, "projects", connection, "project", &id).cloned().unwrap_or_else(|| json!({"id":uuid::Uuid::now_v7().to_string(),"source":"zentao","status":"todo","owner":""}));
        value["name"] = project["name"].clone();
        value["description"] = json!(project["desc"].as_str().unwrap_or(""));
        value["remoteId"] = json!(id);
        value["remoteStatus"] = project["status"].clone();
        // 项目计划日期直接使用项目 begin/end，不从执行排期推算。
        for (local, remote) in [("plannedStartDate", "begin"), ("plannedEndDate", "end")] {
            value[local] = project[remote]
                .as_str()
                .and_then(|raw| raw.get(..10))
                .and_then(|raw| chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok())
                .filter(|date| date.format("%Y").to_string() != "0000")
                .map(|date| json!(date.to_string()))
                .unwrap_or(Value::Null);
        }
        let (account, owner) = owner_identity(&project["PM"]);
        value["owner"] = json!(owner);
        value["remoteOwnerAccount"] = json!(account);
        value["connectionId"] = json!(connection);
        project_ids.insert(id, text(&value, "id").to_string());
        changes.push(("project", value));
    }
    for (project, kind, row) in remote.items {
        let id = remote_id(&row["id"])?;
        let mut task = existing(snapshot, "tasks", connection, &kind, &id).cloned().unwrap_or_else(|| json!({"id":uuid::Uuid::now_v7().to_string(),"status":"todo","notes":"","priority":"normal","sortOrder":now()}));
        task["title"] = row["name"].clone();
        if let Some(description) = row["desc"].as_str() {
            // 旧版个人备注先保留；已跟随远端描述的备注随同步更新。
            if text(&task, "notes").is_empty() || task["notes"] == task["remoteDescription"] {
                task["notes"] = json!(description);
            }
            task["remoteDescription"] = json!(description);
        }
        task["source"] = json!("zentao");
        task["projectId"] = json!(project_ids.get(&project).ok_or("远端项目关联无法解析")?);
        task["remoteId"] = json!(id);
        task["remoteType"] = json!(kind);
        task["connectionId"] = json!(connection);
        task["remoteStatus"] = row["status"].clone();
        if let Some(status) = match text(&row, "status") {
            "wait" => Some("todo"),
            "doing" => Some("doing"),
            "done" => Some("done"),
            "closed" | "cancel" => Some("closed"),
            _ => None,
        } {
            task["status"] = json!(status);
        }
        let (account, owner) = owner_identity(&row["PM"]);
        task["remoteExecutionOwner"] = json!(owner);
        task["remoteExecutionOwnerAccount"] = json!(account);
        task["remotePriority"] = row["pri"].clone();
        let (begin, end) = (&row["begin"], &row["end"]);
        task["remoteBegin"] = begin.clone();
        task["remoteEnd"] = end.clone();
        if let Some(schedule) = remote_date_range(begin, end) {
            task["schedule"] = schedule;
            task["remoteScheduleManaged"] = json!(true);
        } else if task["remoteScheduleManaged"] == true {
            task["schedule"] = Value::Null;
            task["remoteScheduleManaged"] = json!(false);
        }
        changes.push(("task", task));
    }
    Ok(changes)
}
fn remote_date_range(begin: &Value, end: &Value) -> Option<Value> {
    let parse = |value: &Value| {
        let value = value.as_str()?;
        let date = chrono::NaiveDate::parse_from_str(value.get(..10)?, "%Y-%m-%d").ok()?;
        (date.format("%Y").to_string() != "0000").then_some(date)
    };
    let start = parse(begin)?;
    let end = parse(end)?;
    if end < start {
        return None;
    }
    // 禅道结束日包含当天；FullCalendar 的全天事件结束日为排他边界。
    Some(
        json!({"kind":"all_day","timezone":"Asia/Shanghai","start":start.to_string(),"end":end.succ_opt()?.to_string()}),
    )
}

fn execution_patch(old: &Value, value: &Value) -> Result<Value, String> {
    let mut patch = json!({});
    for (local, remote) in [("title", "name"), ("notes", "desc")] {
        if old[local] != value[local] {
            patch[remote] = json!(text(value, local));
        }
    }
    if old["status"] != value["status"] {
        patch["status"] = json!(match text(value, "status") {
            "todo" => "wait",
            "doing" => "doing",
            "done" => "done",
            "closed" => "closed",
            _ => return Err("无效的任务状态".into()),
        });
    }
    if old["schedule"] != value["schedule"] {
        let schedule = &value["schedule"];
        if schedule["kind"] != "all_day" {
            return Err("禅道执行只支持全天排期，请填写开始和结束日期".into());
        }
        let start = chrono::NaiveDate::parse_from_str(text(schedule, "start"), "%Y-%m-%d")
            .map_err(|_| "禅道执行需要有效的开始日期")?;
        let end = chrono::NaiveDate::parse_from_str(text(schedule, "end"), "%Y-%m-%d")
            .map_err(|_| "禅道执行需要有效的结束日期")?;
        if end <= start {
            return Err("结束日期不能早于开始日期".into());
        }
        patch["begin"] = json!(start.to_string());
        patch["end"] = json!(end.pred_opt().ok_or("结束日期无效")?.to_string());
    }
    Ok(patch)
}

fn verify_execution_scope(remote: &Value, project: &str, account: &str) -> Result<(), String> {
    if owner_identity(&remote["PM"]).0 != account {
        return Err("该执行负责人不是当前账号，请重新同步核对".into());
    }
    let remote_project = if remote["project"].is_object() {
        &remote["project"]["id"]
    } else {
        &remote["project"]
    };
    if remote_id(remote_project).ok().as_deref() != Some(project) {
        return Err("禅道执行所属项目已变化或无法核验，请重新同步核对".into());
    }
    Ok(())
}

fn verify_execution_edit(
    base: &str,
    old: &Value,
    remote: &Value,
    patch: &Value,
    project: &str,
    account: &str,
) -> Result<(), String> {
    verify_execution_scope(remote, project, account)?;
    if remote["status"] == "closed"
        && !(old["remoteStatus"] == "closed"
            && patch.get("status").is_some_and(|status| status != "closed"))
    {
        return Err("禅道执行已关闭，请重新同步后在禅道管理".into());
    }
    for (field, baseline) in [
        ("name", &old["title"]),
        ("status", &old["remoteStatus"]),
        ("begin", &old["remoteBegin"]),
        ("end", &old["remoteEnd"]),
        ("desc", &old["remoteDescription"]),
    ] {
        if patch.get(field).is_some()
            && !baseline.is_null()
            && !description::field_matches(base, field, &remote[field], baseline)
        {
            let label = match field {
                "name" => "名称",
                "status" => "状态",
                "begin" => "开始日期",
                "end" => "结束日期",
                "desc" => "备注",
                _ => "内容",
            };
            return Err(format!(
                "禅道执行的{label}已被修改，请先重新同步并重新打开任务后再保存"
            ));
        }
    }
    Ok(())
}

fn apply_execution_result(
    base: &str,
    value: &mut Value,
    old: &Value,
    result: &Value,
    patch: &Value,
) -> Result<(), String> {
    for (field, expected) in patch.as_object().ok_or("无效执行修改")? {
        if !description::field_matches(base, field, &result[field], expected) {
            return Err("禅道已处理请求，但返回内容与修改不一致，请重新同步核对".into());
        }
    }
    let status = match text(result, "status") {
        "wait" => "todo",
        "doing" => "doing",
        "done" => "done",
        "closed" | "cancel" => "closed",
        _ => return Err("禅道返回无法映射的状态，请重新同步核对".into()),
    };
    if text(result, "name").trim().is_empty() {
        return Err("禅道返回名称无效，请重新同步核对".into());
    }
    value["title"] = result["name"].clone();
    value["status"] = json!(status);
    value["remoteStatus"] = result["status"].clone();
    value["remoteBegin"] = result["begin"].clone();
    value["remoteEnd"] = result["end"].clone();
    value["schedule"] = remote_date_range(&result["begin"], &result["end"]).unwrap_or(Value::Null);
    value["remoteScheduleManaged"] = json!(!value["schedule"].is_null());
    if let Some(description) = result["desc"].as_str() {
        if patch.get("desc").is_some()
            || text(old, "notes").is_empty()
            || old["notes"] == old["remoteDescription"]
        {
            value["notes"] = json!(description);
        }
        value["remoteDescription"] = json!(description);
    }
    let (account, owner) = owner_identity(&result["PM"]);
    value["remoteExecutionOwnerAccount"] = json!(account);
    value["remoteExecutionOwner"] = json!(owner);
    Ok(())
}

impl Adapter<'_> {
    async fn execution_request(&self, id: &str, patch: Option<&Value>) -> Result<Value, String> {
        let url = endpoint(&self.base, &format!("executions/{id}"))?;
        let response = if let Some(patch) = patch {
            self.client
                .put(url)
                .json(patch)
                .header("Token", self.token()?)
                .send()
                .await
                .map_err(|_| "禅道写入结果未知，请先重新同步核对，勿直接重复提交")?
        } else {
            self.read(url).await?
        };
        let response = checked_zentao(response).await?;
        let mut stream = response.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "禅道响应读取失败；若为保存操作，请先同步核对结果")?;
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err("禅道响应过大，请同步核对结果".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| "禅道响应无效，请同步核对结果")?;
        if remote_id(&body["id"]).ok().as_deref() != Some(id) {
            return Err("禅道未返回有效执行结果，请同步核对，修改未保存到本地".into());
        }
        Ok(body)
    }
}

pub async fn save_task(state: State<'_, AppState>, mut value: Value) -> Result<Value, String> {
    let placement = board::take_placement(&mut value)?;
    let create_project = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let snapshot = store.snapshot()?;
        board::validate_placement(&snapshot, &value, &placement)?;
        create::new_remote_project(&snapshot, &value)?.map(|project| {
            (
                project,
                store.workspace_id.clone(),
                store.generation.clone(),
            )
        })
    };
    if let Some((project, workspace, generation)) = create_project {
        return create::save_new(&state, value, project, &workspace, &generation).await;
    }
    let (old, connection, project, workspace, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let snapshot = store.snapshot()?;
        let old = snapshot["tasks"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["id"] == value["id"]));
        if old.is_none_or(|old| old["source"] != "zentao") {
            if value["source"] == "zentao" {
                return Err("禅道执行只能通过同步创建".into());
            }
            return board::save(&store, value, &placement);
        }
        let old = old.unwrap().clone();
        store.validate_save("task", value.clone())?;
        let connection = get(&snapshot, "connections", text(&old, "connectionId"))?;
        let project = get(&snapshot, "projects", text(&old, "projectId"))?;
        if project["source"] != "zentao" || project["connectionId"] != old["connectionId"] {
            return Err("禅道项目关联无效，请重新同步".into());
        }
        let project = remote_id(&project["remoteId"])?;
        (
            old,
            connection,
            project,
            store.workspace_id.clone(),
            store.generation.clone(),
        )
    };
    let connection_id = text(&old, "connectionId");
    let _guard = SyncGuard::acquire(&workspace, connection_id)?;
    let patch = execution_patch(&old, &value)?;
    if !patch.as_object().unwrap().is_empty() {
        if connection["enabled"] == false {
            return Err("该禅道连接已停用，无法同步修改".into());
        }
        if old["remoteType"] != "execution" {
            return Err("只支持修改禅道执行，请重新同步清理旧任务".into());
        }
        let id = remote_id(&old["remoteId"])?;
        let key = credential(&workspace, connection_id)?
            .get_password()
            .map_err(|_| "请先保存禅道令牌")?;
        let adapter = Adapter::personal(&connection, key)?.with_auth(
            crate::zentao_auth::AutoAuth::new(&state, &workspace, &generation, &connection)?,
        );
        let account = adapter.account().await?;
        let remote = adapter.execution_request(&id, None).await?;
        verify_execution_edit(
            text(&connection, "baseUrl"),
            &old,
            &remote,
            &patch,
            &project,
            &account,
        )?;
        {
            let store = state.store.lock().map_err(|e| e.to_string())?;
            if store.workspace_id != workspace || store.generation != generation {
                return Err("数据目录已切换，未提交修改".into());
            }
            let latest = get(&store.snapshot()?, "connections", connection_id)?;
            if latest["revision"] != connection["revision"] {
                return Err("连接已变化，未提交修改".into());
            }
            store.validate_save("task", value.clone())?;
            board::validate_placement(&store.snapshot()?, &value, &placement)?;
        }
        let result = adapter.execution_request(&id, Some(&patch)).await?;
        verify_execution_scope(&result, &project, &account)
            .map_err(|error| format!("禅道已处理请求，但{error}"))?;
        apply_execution_result(
            text(&connection, "baseUrl"),
            &mut value,
            &old,
            &result,
            &patch,
        )?;
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace || store.generation != generation {
        return Err("数据目录已切换；禅道可能已更新，请重新同步核对".into());
    }
    board::save(&store, value, &placement)
        .map_err(|error| format!("本地保存失败，禅道可能已更新，请重新同步核对：{error}"))
}
#[tauri::command]
pub async fn zentao_sync(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<String, String> {
    sync_inner(&state, connection_id, None, None).await
}

async fn sync_inner(
    state: &AppState,
    connection_id: String,
    expected: Option<(&str, &std::path::Path)>,
    held_guard: Option<&SyncGuard>,
) -> Result<String, String> {
    let (connection, workspace, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if expected.is_some_and(|(workspace, generation)| {
            store.workspace_id != workspace || store.generation != generation
        }) {
            return Err("工作空间已切换，取消本次自动同步".into());
        }
        (
            get(&store.snapshot()?, "connections", &connection_id)?,
            store.workspace_id.clone(),
            store.generation.clone(),
        )
    };
    if expected.is_some()
        && background::interval_minutes(connection.get("syncIntervalMinutes"))? == 0
    {
        return Err("自动同步已关闭".into());
    }
    if connection["enabled"] == false {
        return Err("该禅道连接已停用".into());
    }
    let _guard = if held_guard.is_none() {
        Some(SyncGuard::acquire(&workspace, &connection_id)?)
    } else {
        None
    };
    let key = credential(&workspace, &connection_id)?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter = Adapter::personal(&connection, key)?.with_auth(
        crate::zentao_auth::AutoAuth::new(state, &workspace, &generation, &connection)?,
    );
    let account = adapter.account().await?;
    let remote = fetch_for_account(&adapter, Some(&account)).await?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace || store.generation != generation {
        return Err("数据目录已切换，请重新同步".into());
    }
    let snapshot = store.snapshot()?;
    let mut latest = get(&snapshot, "connections", &connection_id)?;
    if latest["revision"] != connection["revision"] {
        return Err("连接配置已变化，请重新同步".into());
    }
    // 发布前读取最新个人字段；远端快照与 lastSync 在同一事务提交。
    let mut changes = merge(&snapshot, &connection_id, remote)?;
    let count = changes.len();
    latest["lastSync"] = json!(now());
    latest
        .as_object_mut()
        .unwrap()
        .remove("lastSyncReconciledTasks");
    latest["syncAccount"] = json!(account);
    latest["syncScope"] = json!("involved-projects-owned-executions");
    changes.push(("connection", latest));
    store.save_zentao_snapshot(
        changes,
        vec![(
            "connection",
            connection_id.clone(),
            connection["revision"].as_i64().ok_or("连接缺少版本")?,
        )],
        &connection_id,
    )?;
    Ok(format!("同步完成，共 {count} 条项目与执行"))
}
#[cfg(test)]
mod tests {
    #[test]
    fn project_planned_dates_follow_remote_and_clear_missing_values() {
        let old = json!({"projects":[{"id":"local","remoteId":"1","remoteType":"project","connectionId":"c","plannedStartDate":"2020-01-01","plannedEndDate":"2020-01-02"}],"tasks":[]});
        let remote = RemoteSnapshot {
            projects: vec![
                json!({"id":1,"name":"计划项目","begin":"2026-09-21","end":"2026-10-22","PM":"me"}),
            ],
            items: vec![],
        };
        let changes = merge(&old, "c", remote).unwrap();
        assert_eq!(changes[0].1["plannedStartDate"], "2026-09-21");
        assert_eq!(changes[0].1["plannedEndDate"], "2026-10-22");
        let remote = RemoteSnapshot {
            projects: vec![json!({"id":1,"name":"未设置","begin":"0000-00-00","end":"2026-02-30"})],
            items: vec![],
        };
        let changes = merge(&old, "c", remote).unwrap();
        assert!(changes[0].1["plannedStartDate"].is_null());
        assert!(changes[0].1["plannedEndDate"].is_null());
    }
    use super::*;
    use crate::zentao_auth::{AutoAuth, Secrets, TestSecrets};
    use std::io::{Read, Write};
    #[tokio::test]
    async fn authentication_errors_explain_recovery_without_remote_body() {
        for (status, hint) in [(401, "访问令牌"), (403, "权限不足"), (500, "先同步核对")]
        {
            let response = reqwest::Response::from(
                tauri::http::Response::builder()
                    .status(status)
                    .body("private-server-response")
                    .unwrap(),
            );
            let error = checked_zentao(response).await.unwrap_err();
            assert!(error.contains(hint));
            assert!(!error.contains("private-server-response"));
            assert!(!error.contains("模型"));
        }
    }
    pub(super) fn auth_server(
        responses: Vec<(u16, Value)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = vec![];
            for (status, body) in responses {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "请求未到达");
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("模拟接口失败: {error}"),
                    }
                };
                // macOS 接受的连接可能继承非阻塞标记；完整读取请求前显式恢复阻塞。
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = vec![];
                loop {
                    let mut buffer = [0; 4096];
                    let n = stream.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                    assert!(n > 0);
                }
                requests.push(String::from_utf8(request).unwrap());
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (base, handle)
    }

    fn auth_fixture(base: &str) -> (tempfile::TempDir, AppState, Value, TestSecrets, TestSecrets) {
        let root = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(root.path().to_path_buf()).unwrap();
        let connection = store.save("connection", json!({"id":"auth-test","name":"测试","baseUrl":base,"apiVersion":"v1","rememberCredentials":true,"loginAccount":"tester"})).unwrap();
        let tokens = TestSecrets::default();
        tokens.write(Some("old-token")).unwrap();
        let logins = TestSecrets::default();
        logins.write(Some(&json!({"base":base,"account":"tester","password":"fixture-password","allow_http":true,"blocked":false}).to_string())).unwrap();
        (
            root,
            AppState {
                store: Mutex::new(store),
            },
            connection,
            logins,
            tokens,
        )
    }

    #[tokio::test]
    async fn expired_token_refreshes_once_and_reuses_new_token_for_later_requests() {
        let profile = json!({"profile":{"account":"tester"}});
        let (base, server) = auth_server(vec![
            (401, json!({})),
            (201, json!({"token":"new-token"})),
            (200, profile.clone()),
            (200, profile.clone()),
            (200, profile),
        ]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        let adapter = Adapter::personal(&connection, "old-token".into())
            .unwrap()
            .with_auth(AutoAuth::for_test(
                &state,
                &connection,
                Box::new(logins.clone()),
                Box::new(tokens.clone()),
            ));
        assert_eq!(adapter.account().await.unwrap(), "tester");
        assert_eq!(adapter.account().await.unwrap(), "tester");
        assert_eq!(tokens.read().unwrap().as_deref(), Some("new-token"));
        assert_eq!(
            serde_json::from_str::<Value>(&logins.read().unwrap().unwrap()).unwrap()["blocked"],
            false
        );
        let requests = server.join().unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        assert!(requests[0].to_lowercase().contains("token: old-token"));
        assert!(requests[2..]
            .iter()
            .all(|r| r.to_lowercase().contains("token: new-token")));
        let snapshot = state.store.lock().unwrap().snapshot().unwrap().to_string();
        assert!(!snapshot.contains("fixture-password") && !snapshot.contains("new-token"));
    }

    #[tokio::test]
    async fn failed_login_is_persistently_suppressed_and_never_replaces_old_token() {
        let (base, server) =
            auth_server(vec![(401, json!({})), (403, json!({})), (401, json!({}))]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        for _ in 0..2 {
            let adapter = Adapter::personal(&connection, "old-token".into())
                .unwrap()
                .with_auth(AutoAuth::for_test(
                    &state,
                    &connection,
                    Box::new(logins.clone()),
                    Box::new(tokens.clone()),
                ));
            assert!(adapter.account().await.unwrap_err().contains("暂停"));
        }
        assert_eq!(tokens.read().unwrap().as_deref(), Some("old-token"));
        assert_eq!(
            server
                .join()
                .unwrap()
                .iter()
                .filter(|r| r.starts_with("POST "))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn refreshed_token_with_different_identity_is_not_saved_or_used() {
        let (base, server) = auth_server(vec![
            (401, json!({})),
            (201, json!({"token":"new-token"})),
            (200, json!({"profile":{"account":"another-user"}})),
        ]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        let adapter = Adapter::personal(&connection, "old-token".into())
            .unwrap()
            .with_auth(AutoAuth::for_test(
                &state,
                &connection,
                Box::new(logins.clone()),
                Box::new(tokens.clone()),
            ));
        assert!(adapter.account().await.unwrap_err().contains("账号不匹配"));
        assert_eq!(tokens.read().unwrap().as_deref(), Some("old-token"));
        assert_eq!(
            serde_json::from_str::<Value>(&logins.read().unwrap().unwrap()).unwrap()["blocked"],
            true
        );
        assert_eq!(server.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn second_unauthorized_response_stops_refresh_loop() {
        let (base, server) = auth_server(vec![
            (401, json!({})),
            (201, json!({"token":"new-token"})),
            (200, json!({"profile":{"account":"tester"}})),
            (401, json!({})),
        ]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        let adapter = Adapter::personal(&connection, "old-token".into())
            .unwrap()
            .with_auth(AutoAuth::for_test(
                &state,
                &connection,
                Box::new(logins.clone()),
                Box::new(tokens),
            ));
        assert!(adapter.account().await.unwrap_err().contains("401"));
        assert_eq!(
            serde_json::from_str::<Value>(&logins.read().unwrap().unwrap()).unwrap()["blocked"],
            true
        );
        assert_eq!(
            server
                .join()
                .unwrap()
                .iter()
                .filter(|r| r.starts_with("POST "))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn changed_connection_and_network_failure_never_send_saved_password() {
        let (base, server) = auth_server(vec![(401, json!({}))]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        let adapter = Adapter::personal(&connection, "old-token".into())
            .unwrap()
            .with_auth(AutoAuth::for_test(
                &state,
                &connection,
                Box::new(logins.clone()),
                Box::new(tokens),
            ));
        let mut edited = connection.clone();
        edited["name"] = json!("修改后的连接");
        state
            .store
            .lock()
            .unwrap()
            .save("connection", edited)
            .unwrap();
        assert!(adapter.account().await.unwrap_err().contains("连接已变化"));
        assert_eq!(server.join().unwrap().len(), 1);
        // 模拟服务已关闭后的网络异常，不应触发认证请求或设置失败抑制。
        assert!(adapter.account().await.is_err());
        assert_eq!(
            serde_json::from_str::<Value>(&logins.read().unwrap().unwrap()).unwrap()["blocked"],
            false
        );
    }

    #[tokio::test]
    async fn forbidden_and_uncertain_writes_do_not_trigger_login_or_replay() {
        let (base, server) = auth_server(vec![(403, json!({})), (401, json!({}))]);
        let (_root, state, connection, logins, tokens) = auth_fixture(&base);
        let adapter = Adapter::personal(&connection, "old-token".into())
            .unwrap()
            .with_auth(AutoAuth::for_test(
                &state,
                &connection,
                Box::new(logins.clone()),
                Box::new(tokens),
            ));
        assert!(adapter.account().await.unwrap_err().contains("403"));
        assert!(adapter
            .execution_request("1", Some(&json!({"status":"doing"})))
            .await
            .unwrap_err()
            .contains("401"));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("PUT "));
        assert!(!requests.iter().any(|r| r.starts_with("POST ")));
        assert_eq!(
            serde_json::from_str::<Value>(&logins.read().unwrap().unwrap()).unwrap()["blocked"],
            false
        );
    }

    #[test]
    fn execution_description_expansion_keeps_conflict_and_result_checks() {
        let base = "https://example.test/zentao";
        let raw = r#"<p>原文</p><img src="{708.png}" />"#;
        let expanded = r#"<p>原文</p><img src="/zentao/file-read-708.png" />"#;
        let old = json!({"title":"原名","remoteStatus":"doing","remoteDescription":raw});
        let remote = json!({"name":"原名","status":"doing","desc":expanded,"project":3,"PM":{"account":"me"}});
        let patch = json!({"desc":raw.replace("原文", "新文")});
        assert!(verify_execution_edit(base, &old, &remote, &patch, "3", "me").is_ok());
        let mut changed = remote.clone();
        changed["desc"] = json!(expanded.replace("原文", "他人修改"));
        assert!(
            verify_execution_edit(base, &old, &changed, &patch, "3", "me")
                .unwrap_err()
                .contains("备注已被修改")
        );
        changed = remote.clone();
        changed["status"] = json!("done");
        assert!(verify_execution_edit(
            base,
            &old,
            &changed,
            &json!({"status":"closed"}),
            "3",
            "me"
        )
        .unwrap_err()
        .contains("状态已被修改"));
        let mut result = remote.clone();
        result["desc"] = json!(expanded.replace("原文", "新文"));
        let mut saved = old.clone();
        apply_execution_result(base, &mut saved, &old, &result, &patch).unwrap();
        assert_eq!(saved["remoteDescription"], result["desc"]);
        assert_eq!(saved["notes"], result["desc"]);
        result["desc"] = json!(expanded.replace("原文", "错误返回"));
        assert!(apply_execution_result(base, &mut saved, &old, &result, &patch).is_err());
    }

    #[test]
    fn execution_edit_rejects_scope_closed_and_field_conflicts() {
        let old = json!({"title":"原名","remoteStatus":"wait","remoteDescription":"原文"});
        let remote =
            json!({"name":"原名","status":"wait","desc":"原文","project":3,"PM":{"account":"me"}});
        let patch = json!({"name":"新名"});
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &remote,
            &patch,
            "3",
            "me"
        )
        .is_ok());
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &remote,
            &patch,
            "3",
            "other"
        )
        .unwrap_err()
        .contains("负责人"));
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &remote,
            &patch,
            "4",
            "me"
        )
        .unwrap_err()
        .contains("项目"));
        let mut changed = remote.clone();
        changed["status"] = json!("closed");
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &changed,
            &patch,
            "3",
            "me"
        )
        .unwrap_err()
        .contains("关闭"));
        changed = remote.clone();
        changed["name"] = json!("其他人改名");
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &changed,
            &patch,
            "3",
            "me"
        )
        .unwrap_err()
        .contains("已被修改"));
        changed = remote.clone();
        changed["project"] = json!({"id":"3"});
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &changed,
            &patch,
            "3",
            "me"
        )
        .is_ok());
        changed["project"] = Value::Null;
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &changed,
            &patch,
            "3",
            "me"
        )
        .is_err());
    }
    #[test]
    fn successful_write_applies_complete_remote_snapshot_and_preserves_legacy_notes() {
        let old =
            json!({"title":"原名","status":"todo","notes":"个人备注","remoteDescription":"旧描述"});
        let mut value = old.clone();
        let result = json!({"name":"新名","status":"doing","begin":"2026-09-01","end":"2026-09-09","desc":"远端新描述","PM":{"account":"me","realname":"我"}});
        apply_execution_result(
            "https://example.test/zentao",
            &mut value,
            &old,
            &result,
            &json!({"name":"新名"}),
        )
        .unwrap();
        assert_eq!(value["status"], "doing");
        assert_eq!(value["remoteStatus"], "doing");
        assert_eq!(value["schedule"]["end"], "2026-09-10");
        assert_eq!(value["notes"], "个人备注");
        assert_eq!(value["remoteDescription"], "远端新描述");
        let mut following = old.clone();
        following["notes"] = following["remoteDescription"].clone();
        apply_execution_result(
            "https://example.test/zentao",
            &mut value,
            &following,
            &result,
            &json!({"name":"新名"}),
        )
        .unwrap();
        assert_eq!(value["notes"], "远端新描述");
        assert!(apply_execution_result(
            "https://example.test/zentao",
            &mut value,
            &old,
            &result,
            &json!({"name":"未生效"})
        )
        .is_err());
    }
    #[test]
    fn known_closed_execution_can_be_reopened_but_unexpected_close_cannot() {
        let remote = json!({"status":"closed","project":3,"PM":{"account":"me"}});
        let old = json!({"remoteStatus":"closed"});
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &remote,
            &json!({"status":"doing"}),
            "3",
            "me"
        )
        .is_ok());
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &old,
            &remote,
            &json!({"name":"编辑"}),
            "3",
            "me"
        )
        .is_err());
        assert!(verify_execution_edit(
            "https://example.test/zentao",
            &json!({"remoteStatus":"doing"}),
            &remote,
            &json!({"status":"wait"}),
            "3",
            "me"
        )
        .is_err());
    }
    #[test]
    fn execution_patch_maps_status_and_inclusive_dates() {
        let old = json!({"title":"旧名","notes":"旧备注","status":"todo"});
        for (status, expected) in [
            ("todo", "wait"),
            ("doing", "doing"),
            ("done", "done"),
            ("closed", "closed"),
        ] {
            let mut original = old.clone();
            original["status"] = json!("other");
            let value = json!({"title":"新名","notes":"内容","status":status,"schedule":{"kind":"all_day","start":"2026-09-08","end":"2026-09-11"}});
            let patch = execution_patch(&original, &value).unwrap();
            assert_eq!(
                patch,
                json!({"name":"新名","desc":"内容","status":expected,"begin":"2026-09-08","end":"2026-09-10"})
            );
        }
        assert!(execution_patch(&old, &json!({"schedule":{"kind":"timed"}})).is_err());
        assert!(execution_patch(
            &old,
            &json!({"schedule":{"kind":"all_day","start":"2026-09-08"}})
        )
        .is_err());
        assert!(execution_patch(&old, &old)
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty());
    }
    #[tokio::test]
    async fn writes_execution_endpoint_and_rejects_failure_body() {
        let (url, server) = server(vec![
            json!({"id":42,"name":"新的名称","status":"doing"}),
            json!({"status":"fail"}),
        ]);
        let adapter =
            Adapter::personal(&json!({"baseUrl":url,"apiVersion":"v2"}), "test".into()).unwrap();
        let patch = json!({"name":"新的名称","status":"doing"});
        assert_eq!(
            adapter.execution_request("42", Some(&patch)).await.unwrap()["id"],
            42
        );
        assert!(adapter.execution_request("42", Some(&patch)).await.is_err());
        let requests = server.join().unwrap();
        assert!(requests
            .iter()
            .all(|request| request.starts_with("PUT /api.php/v1/executions/42 ")));
        assert!(requests.iter().all(|request| !request.contains("/tasks")));
    }
    pub(super) fn server(responses: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = vec![];
            for value in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut buffer = [0; 8192];
                let len = stream.read(&mut buffer).unwrap();
                requests.push(String::from_utf8_lossy(&buffer[..len]).to_string());
                let body = value.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            requests
        });
        (root, thread)
    }
    #[tokio::test]
    async fn paginates_projects_and_executions_without_fetching_tasks() {
        let (url, server) = server(vec![
            json!({"projects":[{"id":1}]}),
            json!({"projects":[]}),
            json!({"executions":[{"id":2,"project":1}]}),
            json!({"executions":[{"id":3,"project":1}]}),
            json!({"executions":[]}),
        ]);
        let adapter =
            Adapter::new(&json!({"baseUrl":url,"apiVersion":"v1"}), "test".into()).unwrap();
        assert_eq!(fetch(&adapter).await.unwrap().items.len(), 2);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 5);
        assert!(requests[1].contains("page=2"));
        assert!(requests[4].contains("page=3"));
        assert!(requests.iter().all(|r| !r.contains("/tasks")));
    }
    #[tokio::test]
    async fn rejects_duplicate_pages_and_application_failure() {
        let (url, server) = server(vec![
            json!({"projects":[{"id":1}]}),
            json!({"projects":[{"id":1}]}),
        ]);
        let adapter =
            Adapter::new(&json!({"baseUrl":url,"apiVersion":"v2"}), "test".into()).unwrap();
        assert!(adapter
            .all("projects", "projects")
            .await
            .unwrap_err()
            .contains("重复"));
        assert!(server.join().unwrap()[1].contains("pageID=2"));
        assert!(response_rows(&json!({"status":"fail","projects":[]}), "projects").is_err());
    }
    fn page_body(
        version: &str,
        resource: &str,
        page: u64,
        total: u64,
        size: u64,
        rows: Value,
    ) -> Value {
        let mut body = if version == "v1" {
            json!({"page":page,"total":total,"limit":size})
        } else {
            json!({"status":"success","pager":{"pageID":page,"recTotal":total,"recPerPage":size,"pageTotal":total.div_ceil(size)}})
        };
        body[resource] = rows;
        body
    }
    #[tokio::test]
    async fn official_pagination_stops_at_last_page_and_persists_execution_links() {
        for version in ["v1", "v2"] {
            let (url, server) = server(vec![
                page_body(
                    version,
                    "projects",
                    1,
                    1,
                    1,
                    json!([{"id":1,"name":"项目"}]),
                ),
                page_body(
                    version,
                    "executions",
                    1,
                    2,
                    1,
                    json!([{"id":2,"name":"开发","project":1,"status":"doing"}]),
                ),
                page_body(
                    version,
                    "executions",
                    2,
                    2,
                    1,
                    json!([{"id":3,"name":"完成","project":1,"status":"done"}]),
                ),
            ]);
            let adapter =
                Adapter::new(&json!({"baseUrl":url,"apiVersion":version}), "test".into()).unwrap();
            let remote = fetch(&adapter).await.unwrap();
            assert_eq!(remote.items.len(), 2);
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests[2].contains(if version == "v1" {
                "page=2"
            } else {
                "pageID=2"
            }));
            assert!(requests.iter().all(|r| !r.contains("/tasks")));
            let root = tempfile::tempdir().unwrap();
            let store = crate::storage::Store::open_at(root.path().to_path_buf()).unwrap();
            store
                .save_batch(
                    merge(&store.snapshot().unwrap(), "c", remote).unwrap(),
                    vec![],
                    true,
                )
                .unwrap();
            drop(store);
            let store = crate::storage::Store::open_at(root.path().to_path_buf()).unwrap();
            let snapshot = store.snapshot().unwrap();
            assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 2);
            for task in snapshot["tasks"].as_array().unwrap() {
                assert_eq!(task["projectId"], snapshot["projects"][0]["id"]);
                assert_eq!(task["remoteType"], "execution");
            }
        }
    }
    #[tokio::test]
    async fn single_execution_project_without_pager_does_not_fetch_tasks() {
        for version in ["v1", "v2"] {
            let (url, server) = server(vec![
                page_body(
                    version,
                    "projects",
                    1,
                    1,
                    100,
                    json!([{"id":1,"multiple":"0"}]),
                ),
                json!({"executions":[{"id":2,"project":1}]}),
            ]);
            let adapter =
                Adapter::new(&json!({"baseUrl":url,"apiVersion":version}), "test".into()).unwrap();
            assert_eq!(fetch(&adapter).await.unwrap().items.len(), 1);
            assert_eq!(server.join().unwrap().len(), 2);
        }
    }
    #[test]
    fn pagination_accepts_empty_string_numbers_and_nested_envelopes_but_rejects_invalid_metadata() {
        assert_eq!(
            response_has_more(
                &json!({"projects":[],"pager":{"pageID":1,"pageTotal":0}}),
                "projects",
                1
            )
            .unwrap(),
            Some(false)
        );
        assert_eq!(
            response_has_more(
                &json!({"data":{"projects":[],"page":"2","total":"2","limit":"1"}}),
                "projects",
                2
            )
            .unwrap(),
            Some(false)
        );
        assert_eq!(
            response_has_more(
                &json!({"tasks":[],"pager":{"pageID":"1","recTotal":"2","recPerPage":"1"}}),
                "tasks",
                1
            )
            .unwrap(),
            Some(true)
        );
        assert!(response_has_more(
            &json!({"projects":[],"pager":{"pageID":1,"pageTotal":2}}),
            "projects",
            2
        )
        .unwrap_err()
        .contains("页码"));
        assert!(response_has_more(
            &json!({"projects":[],"page":1,"total":2,"limit":0}),
            "projects",
            1
        )
        .is_err());
        assert!(response_has_more(
            &json!({"projects":[],"pager":{"pageID":1,"pageTotal":-1}}),
            "projects",
            1
        )
        .is_err());
    }
    #[tokio::test]
    async fn empty_page_before_declared_end_is_rejected() {
        let (url, server) = server(vec![page_body("v2", "projects", 1, 2, 1, json!([]))]);
        let adapter =
            Adapter::new(&json!({"baseUrl":url,"apiVersion":"v2"}), "test".into()).unwrap();
        assert!(adapter
            .all("projects", "projects")
            .await
            .unwrap_err()
            .contains("空列表"));
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[test]
    fn merge_updates_remote_status_and_retains_notes_and_manual_schedule() {
        let snapshot = json!({"projects":[],"tasks":[{"id":"local-task","source":"zentao","connectionId":"c","remoteType":"execution","remoteId":"3","projectId":"old","status":"doing","notes":"personal","priority":"high","schedule":{"kind":"all_day","start":"2026-09-01"}}]});
        let data = RemoteSnapshot {
            projects: vec![json!({"id":8,"name":"New"})],
            items: vec![(
                "8".into(),
                "execution".into(),
                json!({"id":3,"name":"Remote","status":"done"}),
            )],
        };
        let changes = merge(&snapshot, "c", data).unwrap();
        let task = &changes[1].1;
        assert_eq!(task["id"], "local-task");
        assert_eq!(task["status"], "done");
        assert_eq!(task["notes"], "personal");
        assert_eq!(task["priority"], "high");
        assert_eq!(task["schedule"], snapshot["tasks"][0]["schedule"]);
        assert_eq!(task["remoteStatus"], "done");
        assert_eq!(task["projectId"], changes[0].1["id"]);
    }
    #[test]
    fn remote_status_and_inclusive_dates_are_applied_to_executions() {
        let data = RemoteSnapshot {
            projects: vec![json!({"id":1,"name":"Project"})],
            items: vec![
                (
                    "1".into(),
                    "execution".into(),
                    json!({"id":2,"name":"开发执行","status":"doing","begin":"2026-09-08","end":"2026-09-10"}),
                ),
                (
                    "1".into(),
                    "execution".into(),
                    json!({"id":3,"name":"另一执行","status":"wait","begin":"2026-09-09","end":"2026-09-09"}),
                ),
            ],
        };
        let changes = merge(&json!({"projects":[],"tasks":[]}), "c", data).unwrap();
        assert_eq!(changes[1].1["status"], "doing");
        assert_eq!(changes[1].1["schedule"]["start"], "2026-09-08");
        assert_eq!(changes[1].1["schedule"]["end"], "2026-09-11");
        assert_eq!(changes[2].1["status"], "todo");
        assert_eq!(changes[2].1["schedule"]["end"], "2026-09-10");
        let root = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(root.path().to_path_buf()).unwrap();
        store.save_batch(changes, vec![], true).unwrap();
        drop(store);
        let store = crate::storage::Store::open_at(root.path().to_path_buf()).unwrap();
        let snapshot = store.snapshot().unwrap();
        let execution = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["remoteType"] == "execution")
            .unwrap();
        assert_eq!(execution["schedule"]["end"], "2026-09-11");
        assert_eq!(execution["status"], "doing");
        assert!(remote_date_range(&json!("2026-09-10"), &json!("2026-09-08")).is_none());
        assert!(remote_date_range(&json!("0000-00-00"), &json!("2026-09-08")).is_none());
        assert!(remote_date_range(&Value::Null, &json!("2026-09-08")).is_none());
        assert_eq!(
            remote_date_range(&json!("2028-02-28"), &json!("2028-02-29")).unwrap()["end"],
            "2028-03-01"
        );
    }
    #[tokio::test]
    async fn selector_fallback_recovers_executions_without_cross_project_import() {
        for version in ["v1", "v2"] {
            let (url, server) = server(vec![
                page_body(
                    version,
                    "projects",
                    1,
                    1,
                    100,
                    json!([{"id":1,"name":"项目"}]),
                ),
                page_body(
                    version,
                    "executions",
                    1,
                    1,
                    100,
                    json!([{"id":9,"project":8}]),
                ),
                page_body(
                    version,
                    "executions",
                    1,
                    2,
                    100,
                    json!([{"id":2,"project":{"id":"001"}},{"id":9,"project":8}]),
                ),
            ]);
            let adapter =
                Adapter::new(&json!({"baseUrl":url,"apiVersion":version}), "test".into()).unwrap();
            let remote = fetch(&adapter).await.unwrap();
            assert_eq!(remote.items.len(), 1);
            assert_eq!(remote.items[0].2["id"], 2);
            assert_eq!(remote.items[0].0, "1");
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests.iter().all(|r| !r.contains("/tasks")));
        }
    }

    #[test]
    fn project_conflicts_remain_errors_with_object_identifiers() {
        let error = validate_parent(&json!({"id":3,"project":8}), "project", "1").unwrap_err();
        assert!(
            error.contains("对象 #3") && error.contains("请求 #1") && error.contains("返回 #8")
        );
        assert!(matching_parent(&[json!({"id":3})], "project", "2").is_err());
        assert_eq!(remote_id(&json!("0002")).unwrap(), "2");
        assert!(remote_id(&json!("000")).is_err());
    }

    #[tokio::test]
    async fn personal_scope_uses_token_account_involved_projects_and_owned_executions() {
        let (url, server) = server(vec![
            json!({"profile":{"account":"alice"}}),
            page_body(
                "v1",
                "projects",
                1,
                1,
                100,
                json!([{"id":30,"name":"参与项目","PM":"project-owner"}]),
            ),
            page_body(
                "v1",
                "executions",
                1,
                6,
                100,
                json!([
                    {"id":31,"name":"进行中的执行","status":"doing","project":30,"PM":{"account":"alice","realname":"甲"}},
                    {"id":32,"project":30,"PM":{"account":"bob","realname":"alice"}},
                    {"id":33,"project":30,"PM":null},
                    {"id":34,"project":8,"PM":{"account":"alice"}},
                    {"id":35,"name":"已关闭的执行","project":30,"status":"closed","PM":{"account":"alice"}},
                    {"id":36,"name":"他人关闭的执行","project":30,"status":"closed","PM":{"account":"bob"}}
                ]),
            ),
        ]);
        let adapter =
            Adapter::personal(&json!({"baseUrl":url,"apiVersion":"v2"}), "test".into()).unwrap();
        let account = adapter.account().await.unwrap();
        let remote = fetch_for_account(&adapter, Some(&account)).await.unwrap();
        assert_eq!(remote.projects.len(), 1);
        assert_eq!(remote.items.len(), 2);
        assert_eq!(remote.items[0].2["id"], 31);
        let changes = merge(&json!({"tasks":[],"projects":[]}), "connection", remote).unwrap();
        assert_eq!(changes[0].1["owner"], "project-owner");
        assert_eq!(changes[1].1["remoteExecutionOwnerAccount"], "alice");
        assert_eq!(changes[1].1["remoteExecutionOwner"], "甲");
        assert_eq!(changes[2].1["status"], "closed");
        assert_eq!(changes[2].1["remoteStatus"], "closed");
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(temporary.path().to_path_buf()).unwrap();
        store.save_batch(changes, vec![], true).unwrap();
        let snapshot = store.snapshot().unwrap();
        let closed_id =
            existing(&snapshot, "tasks", "connection", "execution", "35").unwrap()["id"].clone();
        let again = RemoteSnapshot {
            projects: vec![json!({"id":30,"name":"参与项目","PM":"project-owner"})],
            items: vec![(
                "30".into(),
                "execution".into(),
                json!({"id":35,"name":"已关闭的执行","project":30,"status":"closed","PM":{"account":"alice"}}),
            )],
        };
        let changes = merge(&snapshot, "connection", again).unwrap();
        store
            .save_zentao_snapshot(changes, vec![], "connection")
            .unwrap();
        let snapshot = store.snapshot().unwrap();
        let closed = existing(&snapshot, "tasks", "connection", "execution", "35").unwrap();
        assert_eq!(closed["id"], closed_id);
        assert_eq!(closed["status"], "closed");
        assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 1);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api.php/v1/user "));
        assert!(requests[1].contains("involved=1"));
        assert!(requests[2].contains("fields=PM"));
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|r| !r.contains("/tasks")));
    }
    #[test]
    fn execution_scope_never_matches_display_names_or_unknown_owners() {
        assert!(
            !responsible_for(&json!({"PM":{"account":"bob","realname":"alice"}}), "alice").unwrap()
        );
        assert!(!responsible_for(&json!({"PM":null}), "alice").unwrap());
        assert!(responsible_for(&json!({}), "alice").is_err());
        assert!(responsible_for(&json!({"PM":{"realname":"alice"}}), "alice").is_err());
    }
    #[test]
    fn sync_lock_releases_and_parent_changes_abort() {
        let lock = SyncGuard::acquire("test", "conn").unwrap();
        assert!(SyncGuard::acquire("test", "conn").is_err());
        drop(lock);
        assert!(SyncGuard::acquire("test", "conn").is_ok());
        assert!(validate_parent(&json!({"project":{"id":9}}), "project", "8").is_err());
        assert!(remote_id(&json!("../tokens")).is_err());
    }
}
