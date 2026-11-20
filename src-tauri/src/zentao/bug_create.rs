//! 新建禅道 BUG：核验远端选项，持久登记附件上传及创建意图。
use super::*;
use crate::{
    bug_steps::{HtmlSteps, StepsFormat},
    storage::Store,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Draft {
    id: uuid::Uuid,
    title: String,
    steps: String,
    steps_format: Option<StepsFormat>,
    bug_type: String,
    severity: u8,
    priority: u8,
    deadline: Option<String>,
    project_id: String,
    product_id: String,
    execution_id: Option<String>,
    assigned_to: String,
    #[serde(default = "trunk")]
    opened_build: Vec<String>,
    #[serde(default)]
    mailto: Vec<String>,
}

fn trunk() -> Vec<String> {
    vec!["trunk".into()]
}

fn id_number(value: &str) -> Result<u32, String> {
    remote_id(&json!(value))?
        .parse()
        .map_err(|_| "禅道对象编号超出范围".into())
}

fn valid_account(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.contains(',')
        && !value.chars().any(char::is_control)
}

impl Draft {
    fn parse(value: Value) -> Result<Self, String> {
        let mut draft: Self = serde_json::from_value(value)
            .map_err(|_| "新建禅道 BUG 的表单字段无效，请检查必填项和编号格式")?;
        draft.title = draft.title.trim().into();
        if draft.id.is_nil()
            || draft.project_id.trim().is_empty()
            || draft.title.is_empty()
            || draft.title.chars().count() > 255
            || draft.title.chars().any(char::is_control)
        {
            return Err("请填写有效的 BUG 标题并选择禅道项目".into());
        }
        if draft.steps_format.is_none()
            && (draft.steps.len() > 64 * 1024 || draft.steps.contains('\0'))
        {
            return Err("重现步骤过长或包含无效字符".into());
        }
        if !(1..=4).contains(&draft.severity) || !(1..=4).contains(&draft.priority) {
            return Err("严重程度和优先级必须是 1 至 4".into());
        }
        if !matches!(
            draft.bug_type.as_str(),
            "codeerror"
                | "config"
                | "install"
                | "security"
                | "performance"
                | "standard"
                | "automation"
                | "designdefect"
                | "others"
        ) {
            return Err("请选择受支持的禅道 BUG 类型".into());
        }
        if let Some(date) = draft.deadline.as_deref() {
            let valid = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_ok_and(|day| day.format("%Y-%m-%d").to_string() == date);
            if !valid || date.starts_with("0000-") {
                return Err("截止日期必须是有效的 YYYY-MM-DD 日期".into());
            }
        }
        if !valid_account(&draft.assigned_to)
            || draft.mailto.len() > 100
            || !draft.mailto.iter().all(|account| valid_account(account))
        {
            return Err("指派或抄送账号无效".into());
        }
        draft.mailto.sort_unstable();
        draft.mailto.dedup();
        draft.product_id = id_number(&draft.product_id)?.to_string();
        if let Some(execution) = &mut draft.execution_id {
            *execution = id_number(execution)?.to_string();
        }
        if !(1..=20).contains(&draft.opened_build.len()) {
            return Err("请填写 1 至 20 个影响版本".into());
        }
        for build in &mut draft.opened_build {
            if build != "trunk" {
                *build = id_number(build)?.to_string();
            }
        }
        draft.opened_build.sort_unstable();
        draft.opened_build.dedup();
        Ok(draft)
    }

    fn payload(&self, project: &str, version: &str) -> Result<Value, String> {
        let steps = if self.steps_format.is_some() {
            // 富文本请求的摘要覆盖原内容，实际提交时换成清理后的 HTML。
            json!({"htmlSha256": format!("{:x}", Sha256::digest(self.steps.as_bytes()))})
        } else {
            // 旧草稿的回执依赖原纯文本 payload 摘要。
            json!(self
                .steps
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\n', "<br />"))
        };
        let mut payload = json!({
            "title": self.title, "steps": steps, "type": self.bug_type,
            "severity": self.severity, "pri": self.priority, "project": id_number(project)?,
            "execution": self.execution_id.as_deref().map(id_number).transpose()?.unwrap_or(0),
            "assignedTo": self.assigned_to, "openedBuild": self.opened_build
        });
        if version == "v2" {
            payload["productID"] = json!(id_number(&self.product_id)?);
        }
        if let Some(deadline) = &self.deadline {
            payload["deadline"] = json!(deadline);
        }
        if !self.mailto.is_empty() {
            payload["mailto"] = json!(self.mailto);
        }
        Ok(payload)
    }
}

struct Context {
    workspace: String,
    generation: std::path::PathBuf,
    project: Value,
    connection: Value,
    remote_project: String,
}

impl Context {
    fn load(state: &AppState, workspace: &str, project: &str) -> Result<Self, String> {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace {
            return Err("工作空间已切换，请重新打开新建 BUG".into());
        }
        let snapshot = store.snapshot()?;
        let project = get(&snapshot, "projects", project)?;
        if project["source"] != "zentao" {
            return Err("请先选择已同步的禅道项目".into());
        }
        let remote_project = remote_id(&project["remoteId"])?;
        let connection = get(&snapshot, "connections", text(&project, "connectionId"))?;
        if connection["enabled"] == false {
            return Err("该禅道连接已停用".into());
        }
        Ok(Self {
            workspace: workspace.into(),
            generation: store.generation.clone(),
            project,
            connection,
            remote_project,
        })
    }

    fn check(&self, store: &Store) -> Result<Value, String> {
        if store.workspace_id != self.workspace || store.generation != self.generation {
            return Err("工作空间已切换，请在原工作空间核对；请勿重复提交".into());
        }
        let snapshot = store.snapshot()?;
        let project = get(&snapshot, "projects", text(&self.project, "id"))?;
        let connection = get(&snapshot, "connections", text(&self.connection, "id"))?;
        if project != self.project || connection != self.connection {
            return Err("项目或连接已变化，请刷新后在原草稿继续核对".into());
        }
        Ok(snapshot)
    }

    fn adapter<'a>(&self, state: &'a AppState) -> Result<Adapter<'a>, String> {
        let token = credential(&self.workspace, text(&self.connection, "id"))?
            .get_password()
            .map_err(|_| "请先保存禅道令牌")?;
        // 两版都沿用个人同步的 v1 账号与关联目录。v2 创建／详情按连接配置单独路由。
        Ok(Adapter::personal(&self.connection, token)?.with_auth(
            crate::zentao_auth::AutoAuth::new(
                state,
                &self.workspace,
                &self.generation,
                &self.connection,
            )?,
        ))
    }
}

fn reference(value: &Value) -> Result<String, String> {
    remote_id(if value.is_object() {
        &value["id"]
    } else {
        value
    })
}

fn unavailable(row: &Value) -> bool {
    row["deleted"] == true
        || row["deleted"] == "1"
        || row["deleted"] == 1
        || matches!(text(row, "status"), "closed" | "cancel" | "canceled")
}

async fn detail(adapter: &Adapter<'_>, path: &str) -> Result<Value, String> {
    validation::body(adapter.read(endpoint(&adapter.base, path)?).await?, "读取").await
}

fn named_option(row: &Value) -> Result<Value, String> {
    let name = row["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .ok_or("禅道选项缺少有效名称")?;
    Ok(json!({"id": remote_id(&row["id"])?, "name": name}))
}

async fn project_products(
    adapter: &Adapter<'_>,
    project_id: &str,
) -> Result<(Vec<Value>, bool), String> {
    let mut url = endpoint(&adapter.base, &format!("projects/{project_id}"))?;
    url.query_pairs_mut().append_pair("fields", "products");
    let project = validation::body(adapter.read(url).await?, "读取").await?;
    if remote_id(&project["id"])? != project_id || unavailable(&project) {
        return Err("禅道项目无法核验或已关闭".into());
    }
    let products = project["products"]
        .as_array()
        .ok_or("禅道项目缺少关联产品数据")?;
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for row in products.iter().filter(|row| !unavailable(row)) {
        let option = named_option(row)?;
        let id = text(&option, "id");
        // 多分支产品可能重复列在项目关联表中，界面按产品 ID 选择。
        if seen.insert(id.to_owned()) {
            options.push(option);
        }
    }
    let multiple = &project["multiple"];
    Ok((
        options,
        multiple == false || multiple == 0 || multiple == "0",
    ))
}

async fn execution_option(
    adapter: &Adapter<'_>,
    project_id: &str,
    id: &str,
) -> Result<Option<Value>, String> {
    let row = detail(adapter, &format!("executions/{id}")).await?;
    if remote_id(&row["id"])? != id || reference(&row["project"])? != project_id {
        return Err("禅道执行归属与所选项目不一致".into());
    }
    if unavailable(&row) {
        return Ok(None);
    }
    let products = row["products"]
        .as_array()
        .ok_or("禅道执行缺少关联产品数据")?;
    let mut product_ids = products
        .iter()
        .map(reference)
        .collect::<Result<Vec<_>, _>>()?;
    product_ids.sort_unstable();
    product_ids.dedup();
    let mut option = named_option(&row)?;
    option["productIds"] = json!(product_ids);
    Ok(Some(option))
}

async fn executions(
    adapter: &Adapter<'_>,
    project_id: &str,
    single: bool,
) -> Result<Vec<Value>, String> {
    let rows = adapter
        .read_pages(
            &format!("projects/{project_id}/executions"),
            "executions",
            single,
        )
        .await?;
    let ids = rows
        .iter()
        .filter(|row| !unavailable(row))
        .map(|row| {
            if reference(&row["project"])? != project_id {
                return Err("禅道项目执行列表的归属不一致".into());
            }
            remote_id(&row["id"])
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut options = futures_util::stream::iter(ids)
        .map(|id| async move { execution_option(adapter, project_id, &id).await })
        .buffered(5);
    let mut output = Vec::new();
    while let Some(option) = options.next().await {
        if let Some(option) = option? {
            output.push(option);
        }
    }
    Ok(output)
}

async fn verify_choices(adapter: &Adapter<'_>, project: &str, draft: &Draft) -> Result<(), String> {
    let (products, _) = project_products(adapter, project).await?;
    if !products.iter().any(|row| row["id"] == draft.product_id) {
        return Err("所选产品不属于该禅道项目，或已关闭".into());
    }
    if let Some(id) = &draft.execution_id {
        let execution = execution_option(adapter, project, id)
            .await?
            .ok_or("所选执行已关闭")?;
        if !execution["productIds"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == &draft.product_id))
        {
            return Err("所选执行未关联该产品".into());
        }
    }
    for id in draft.opened_build.iter().filter(|id| *id != "trunk") {
        let build = detail(adapter, &format!("builds/{id}")).await?;
        if remote_id(&build["id"])? != *id
            || reference(&build["product"])? != draft.product_id
            || reference(&build["project"])? != project
            || unavailable(&build)
            || draft.execution_id.as_ref().is_some_and(|expected| {
                reference(&build["execution"]).ok().as_ref() != Some(expected)
            })
        {
            return Err("影响版本不属于所选产品、项目或执行，或已删除".into());
        }
    }
    // 复用编辑 BUG 的账号过滤规则和选项序列化契约。
    let users = serde_json::to_value(users::read_assignees(adapter).await?)
        .map_err(|_| "禅道人员选项格式无效")?;
    let users = users.as_array().ok_or("禅道人员选项格式无效")?;
    for account in std::iter::once(&draft.assigned_to).chain(draft.mailto.iter()) {
        if !users.iter().any(|user| user["account"] == account.as_str()) {
            return Err("所选指派或抄送账号在当前禅道连接中不可用".into());
        }
    }
    Ok(())
}

fn create_url(v1: &str, v2: &str, version: &str, product: &str) -> Result<url::Url, String> {
    match version {
        "v1" => endpoint(v1, &format!("products/{product}/bugs")),
        "v2" => endpoint(v2, "bugs"),
        _ => Err("请选择此禅道连接实际支持的 API 版本".into()),
    }
}

async fn post(
    adapter: &Adapter<'_>,
    url: url::Url,
    request: &Value,
    version: &str,
) -> Result<(String, Option<Value>), String> {
    // 连协议级自动重试也禁用；只有这个入口能发出创建请求。
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| "无法建立禅道 BUG 创建请求")?;
    let response = client
        .post(url)
        .header("Token", adapter.token()?)
        .json(request)
        .send()
        .await
        .map_err(|_| "禅道 BUG 创建结果未知；请在禅道核对，勿重复提交")?;
    let body = validation::body(response, "创建").await?;
    if version == "v2" && body["status"] != "success" {
        return Err("禅道未确认 BUG 创建成功，请核对，勿重复提交".into());
    }
    let id = remote_id(&body["id"]).map_err(|_| "禅道未返回有效 BUG 编号，请核对，勿重复提交")?;
    Ok((id, (version == "v1").then_some(body)))
}

fn account(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| value["account"].as_str())
}

fn returned_list(value: &Value) -> Option<Vec<&str>> {
    let mut items: Vec<&str> = if let Some(csv) = value.as_str() {
        csv.split(',').filter(|s| !s.is_empty()).collect()
    } else if let Some(values) = value.as_array() {
        values.iter().map(account).collect::<Option<Vec<_>>>()?
    } else {
        value
            .as_object()?
            .values()
            .map(account)
            .collect::<Option<Vec<_>>>()?
    };
    items.sort_unstable();
    items.dedup();
    Some(items)
}

fn verify_created(
    remote: &Value,
    id: &str,
    project: &str,
    draft: &Draft,
    creator: &str,
) -> Result<(), String> {
    let correct_execution = match &draft.execution_id {
        Some(execution) => reference(&remote["execution"]).ok().as_ref() == Some(execution),
        None => remote
            .get("execution")
            .is_some_and(|v| v.is_null() || v == 0 || v == "0"),
    };
    if remote_id(&remote["id"]).ok().as_deref() != Some(id)
        || reference(&remote["project"]).ok().as_deref() != Some(project)
        || reference(&remote["product"]).ok().as_deref() != Some(draft.product_id.as_str())
        || !correct_execution
        || unavailable(remote)
        || remote["status"] != "active"
        || remote["title"] != draft.title
        || remote["type"] != draft.bug_type
        || reference(&remote["severity"]).ok().as_deref()
            != Some(draft.severity.to_string().as_str())
        || reference(&remote["pri"]).ok().as_deref() != Some(draft.priority.to_string().as_str())
        || account(&remote["openedBy"]) != Some(creator)
        || account(&remote["assignedTo"]) != Some(draft.assigned_to.as_str())
        || returned_list(&remote["openedBuild"]).as_deref()
            != Some(
                draft
                    .opened_build
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
        || draft
            .deadline
            .as_ref()
            .is_some_and(|date| remote["deadline"] != *date)
        || (!draft.mailto.is_empty()
            && returned_list(&remote["mailto"]).as_deref()
                != Some(
                    draft
                        .mailto
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .as_slice(),
                ))
    {
        return Err(format!(
            "禅道已返回 BUG #{id}，但详情与提交不一致或不完整。请到禅道核对，勿重复提交"
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum IntentStage {
    Uploading,
    #[default]
    Creating,
}

impl IntentStage {
    fn is_creating(&self) -> bool {
        matches!(self, Self::Creating)
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Intent {
    request_hash: String,
    remote_id: Option<String>,
    assigned_to_self: Option<bool>,
    // 省略默认字段使旧版 JSON 的意图仍能通过原值比较并推进回执。
    #[serde(default, skip_serializing_if = "IntentStage::is_creating")]
    stage: IntentStage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    image_ids: Vec<String>,
}

fn read_intent(store: &Store, key: &str) -> Result<Option<Intent>, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "无法读取禅道 BUG 提交意图")?;
    value
        .map(|raw| {
            serde_json::from_str(&raw).map_err(|_| "禅道 BUG 提交意图损坏，请在禅道核对".to_owned())
        })
        .transpose()
}

fn write_intent(
    store: &Store,
    key: &str,
    previous: Option<&Intent>,
    next: &Intent,
) -> Result<(), String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let next = serde_json::to_string(next).map_err(|e| e.to_string())?;
    // 唯一键解决跨进程并发；后续只允许基于已读取的状态推进。
    let rows = if let Some(previous) = previous {
        let previous = serde_json::to_string(previous).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE settings SET value=?1 WHERE key=?2 AND value=?3",
            params![next, key, previous],
        )
    } else {
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
            params![key, next],
        )
    }
    .map_err(|_| "无法保存禅道 BUG 提交意图，请核对原草稿")?;
    if rows != 1 {
        return Err("该草稿已在其他操作中处理，请核对原草稿，勿重复提交".into());
    }
    Ok(())
}

fn record_result(
    state: &AppState,
    ctx: &Context,
    key: &str,
    previous: &Intent,
    next: &Intent,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    // 项目配置可能在请求期间变化，同代工作空间仍应保留已取得的编号。
    if store.workspace_id != ctx.workspace || store.generation != ctx.generation {
        return Err("工作空间已切换，请回原工作空间和禅道核对，勿重复提交".into());
    }
    write_intent(&store, key, Some(previous), next)
}

fn upload_error(intent: &Intent, reason: &str) -> String {
    format!(
        "图片处理未完成，BUG 尚未提交。已确认上传 {} 张，已阻止同一草稿重复上传。{reason}",
        intent.image_ids.len()
    )
}

async fn upload_steps_images(
    state: &AppState,
    ctx: &Context,
    adapter: &Adapter<'_>,
    key: &str,
    uid: uuid::Uuid,
    steps: &mut HtmlSteps,
    intent: &mut Intent,
) -> Result<(), String> {
    for image in &mut steps.images {
        {
            let store = state.store.lock().map_err(|e| e.to_string())?;
            ctx.check(&store)?;
        }
        let id = media::upload_image(
            adapter,
            text(&ctx.connection, "baseUrl"),
            "bug-step",
            std::mem::take(&mut image.bytes),
            image.mime,
            image.extension,
            Some(uid),
        )
        .await?;
        let mut next = intent.clone();
        next.image_ids.push(id.clone());
        record_result(state, ctx, key, intent, &next).map_err(|error| {
            format!("禅道返回了附件 #{id}，回执保存失败，请按编号核对。{error}")
        })?;
        *intent = next;
    }
    Ok(())
}

fn cache_self(
    store: &Store,
    snapshot: &Value,
    ctx: &Context,
    remote: &Value,
    id: &str,
) -> Result<(), String> {
    let connection = text(&ctx.connection, "id");
    let old = snapshot["bugs"]
        .as_array()
        .ok_or("BUG 缓存无效")?
        .iter()
        .find(|row| row["connectionId"] == connection && row["remoteId"] == id);
    let bug = bugs::normalized(remote, connection, old)?;
    let mut expected = vec![
        (
            "connection",
            connection.to_owned(),
            ctx.connection["revision"].as_i64().ok_or("连接版本无效")?,
        ),
        (
            "project",
            text(&ctx.project, "id").to_owned(),
            ctx.project["revision"].as_i64().ok_or("项目版本无效")?,
        ),
    ];
    if let Some(old) = old {
        expected.push((
            "bug",
            text(old, "id").to_owned(),
            old["revision"].as_i64().ok_or("BUG 版本无效")?,
        ));
    }
    store
        .save_batch(vec![("bug", bug)], expected, true)
        .map_err(|_| {
            format!("禅道 BUG #{id} 已核验，本地缓存保存失败；请用原草稿重试或同步 BUG")
        })?;
    Ok(())
}

#[tauri::command]
pub async fn zentao_bug_create_options(
    state: State<'_, AppState>,
    workspace_id: String,
    project_id: String,
) -> Result<Value, String> {
    let ctx = Context::load(&state, &workspace_id, &project_id)?;
    let _guard = SyncGuard::acquire(&ctx.workspace, text(&ctx.connection, "id"))?;
    let adapter = ctx.adapter(&state)?;
    let assignees = users::read_assignees(&adapter).await?;
    let (products, single) = project_products(&adapter, &ctx.remote_project).await?;
    let executions = executions(&adapter, &ctx.remote_project, single).await?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    ctx.check(&store)?;
    Ok(json!({"products": products, "executions": executions, "assignees": assignees}))
}

#[tauri::command]
pub async fn zentao_bug_create(
    state: State<'_, AppState>,
    workspace_id: String,
    value: Value,
) -> Result<Value, String> {
    let draft = Draft::parse(value)?;
    let ctx = Context::load(&state, &workspace_id, &draft.project_id)?;
    let _guard = SyncGuard::acquire(&ctx.workspace, text(&ctx.connection, "id"))?;
    let adapter = ctx.adapter(&state)?;
    create_with_adapter(&state, &ctx, &adapter, &draft).await
}

async fn create_with_adapter(
    state: &AppState,
    ctx: &Context,
    adapter: &Adapter<'_>,
    draft: &Draft,
) -> Result<Value, String> {
    let connection_id = text(&ctx.connection, "id");
    let mut rich_steps = draft
        .steps_format
        .map(|_| HtmlSteps::parse(&draft.steps))
        .transpose()?;
    let cleaned_html = rich_steps
        .as_ref()
        .map(|steps| steps.render(|_, _| String::new()));
    if cleaned_html
        .as_ref()
        .is_some_and(|html| html.to_ascii_lowercase().contains("data:image/"))
    {
        return Err("图片数据请作为图片粘贴到复现步骤中".into());
    }
    let has_images = rich_steps
        .as_ref()
        .is_some_and(|steps| !steps.images.is_empty());
    // 22.0 的 v1 BUG 创建与文件上传明确共享 uid 的附件关联契约。
    let version = if has_images {
        "v1"
    } else {
        text(&ctx.connection, "apiVersion")
    };
    let mut request = draft.payload(&ctx.remote_project, version)?;
    if has_images {
        request["uid"] = json!(draft.id);
    }
    let v2 = endpoint(text(&ctx.connection, "baseUrl"), "api.php/v2")?.to_string();
    let url = create_url(&adapter.base, &v2, version, &draft.product_id)?;
    let creator = adapter.account().await?;
    let key = format!("zentao-bug-create:{}", draft.id);
    // 只持久化摘要和回执，不把凭据、标题、步骤或抄送账号写入 settings。
    let request_hash = format!(
        "{:x}",
        Sha256::digest(
            json!([
                connection_id,
                url.as_str(),
                draft.project_id,
                creator,
                request
            ])
            .to_string()
        )
    );
    let existing = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        ctx.check(&store)?;
        read_intent(&store, &key)?
    };
    let (intent, initial) = if let Some(intent) = existing {
        if !intent.stage.is_creating() {
            return Err(upload_error(&intent, "请在禅道核对原草稿的附件。"));
        }
        if intent.request_hash != request_hash {
            return Err("该草稿已提交过，请核对原内容，不能修改后重复创建".into());
        }
        let id = intent
            .remote_id
            .as_deref()
            .ok_or("该草稿的创建结果未知，请在禅道核对；已阻止重复提交")?;
        if let Some(self_assigned) = intent.assigned_to_self {
            return Ok(json!({"remoteId": id, "assignedToSelf": self_assigned}));
        }
        (intent, None)
    } else {
        verify_choices(adapter, &ctx.remote_project, draft).await?;
        if has_images && adapter.account().await? != creator {
            return Err("禅道令牌所属账号已变化，BUG 尚未提交，请重新打开原草稿".into());
        }
        let mut pending = Intent {
            request_hash,
            remote_id: None,
            assigned_to_self: None,
            stage: if has_images {
                IntentStage::Uploading
            } else {
                IntentStage::Creating
            },
            image_ids: Vec::new(),
        };
        {
            let store = state.store.lock().map_err(|e| e.to_string())?;
            ctx.check(&store)?;
            write_intent(&store, &key, None, &pending)?;
        }
        if let Some(steps) = rich_steps.as_mut().filter(|_| has_images) {
            upload_steps_images(state, ctx, adapter, &key, draft.id, steps, &mut pending)
                .await
                .map_err(|error| upload_error(&pending, &error))?;
            request["steps"] = json!(steps.render(|index, image| {
                format!("{{{}.{}}}", pending.image_ids[index], image.extension)
            }));
            let mut creating = pending.clone();
            creating.stage = IntentStage::Creating;
            {
                let store = state.store.lock().map_err(|e| e.to_string())?;
                ctx.check(&store)
                    .and_then(|_| write_intent(&store, &key, Some(&pending), &creating))
                    .map_err(|error| upload_error(&pending, &error))?;
            }
            pending = creating;
        } else if let Some(html) = cleaned_html {
            request["steps"] = json!(html);
        }
        let (id, initial) = post(adapter, url, &request, version).await?;
        let received = Intent {
            remote_id: Some(id.clone()),
            ..pending.clone()
        };
        record_result(state, ctx, &key, &pending, &received).map_err(|error| {
            format!("禅道返回了 BUG #{id}，回执保存失败，请按编号核对。{error}")
        })?;
        (received, initial)
    };
    let id = intent.remote_id.as_deref().ok_or("禅道 BUG 编号缺失")?;
    let remote = if let Some(initial) = initial {
        initial
    } else if version == "v1" {
        adapter
            .bug_request(id, None)
            .await
            .map_err(|e| format!("禅道 BUG #{id} 的详情核验未完成，可用原草稿重试：{e}"))?
    } else {
        let url = endpoint(&v2, &format!("bugs/{id}"))?;
        let response = adapter
            .read(url)
            .await
            .map_err(|e| format!("禅道 BUG #{id} 的详情核验未完成，可用原草稿重试：{e}"))?;
        let body = validation::body(response, "读取").await?;
        body.get("bug")
            .cloned()
            .ok_or("禅道 v2 响应缺少 BUG 详情，请在禅道核对")?
    };
    verify_created(&remote, id, &ctx.remote_project, draft, &creator)?;
    if intent.image_ids.iter().any(|file| {
        !media::referenced(
            text(&ctx.connection, "baseUrl"),
            text(&remote, "steps"),
            file,
        )
    }) {
        return Err(format!(
            "禅道 BUG #{id} 的复现步骤缺少本次上传的图片引用，请核对附件或用原草稿重试"
        ));
    }
    let assigned_to_self = draft.assigned_to == creator;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = ctx.check(&store)?;
    if assigned_to_self {
        cache_self(&store, &snapshot, ctx, &remote, id)?;
    }
    let completed = Intent {
        assigned_to_self: Some(assigned_to_self),
        ..intent.clone()
    };
    write_intent(&store, &key, Some(&intent), &completed)?;
    Ok(json!({"remoteId": id, "assignedToSelf": assigned_to_self}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zentao::tests::auth_server;
    use base64::{engine::general_purpose::STANDARD, Engine};

    fn fixture(
        base: &str,
        version: &str,
    ) -> (tempfile::TempDir, AppState, Context, Adapter<'static>) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let connection = store
            .save(
                "connection",
                json!({
                    "id":"connection","name":"模拟禅道","baseUrl":base,"apiVersion":version
                }),
            )
            .unwrap();
        store.save_batch(vec![("project", json!({
                    "id":"local-project","name":"本地项目","source":"zentao","connectionId":"connection","remoteId":"27"
        }))], vec![], true).unwrap();
        let workspace = store.workspace_id.clone();
        let state = AppState {
            store: Mutex::new(store),
        };
        let ctx = Context::load(&state, &workspace, "local-project").unwrap();
        let adapter = Adapter::personal(&connection, "mock-token".into()).unwrap();
        (root, state, ctx, adapter)
    }

    fn draft(assignee: &str) -> Value {
        json!({
            "id":"68d36ef8-9aa6-4e86-926b-46bbe68a4e22",
            "title":"导出报错", "steps":"<按钮> & 下一步\n错误", "bugType":"codeerror",
            "severity":2, "priority":3, "projectId":"local-project", "productId":"11",
            "executionId":"32", "assignedTo":assignee, "deadline":"2026-11-20", "mailto":["other"]
        })
    }

    fn creator() -> (u16, Value) {
        (200, json!({"profile":{"account":"me"}}))
    }

    fn project() -> (u16, Value) {
        (
            200,
            json!({"id":27,"multiple":"1","status":"doing","products":[{"id":11,"name":"产品 A"}]}),
        )
    }

    fn execution(product: u32) -> (u16, Value) {
        (
            200,
            json!({"id":"32","project":"27","name":"执行 A","status":"doing","products":[{"id":product}]}),
        )
    }

    fn presubmit() -> Vec<(u16, Value)> {
        vec![
            creator(),
            project(),
            execution(11),
            creator(),
            (
                200,
                json!({"page":1,"limit":100,"total":2,"users":[
                    {"account":"me","realname":"我"},{"account":"other","realname":"同事"}
                ]}),
            ),
        ]
    }

    fn remote(assignee: &str) -> Value {
        json!({
            "id":53,"project":"27","product":"11","execution":"32","title":"导出报错",
            "type":"codeerror","severity":"2","pri":"3","status":"active","openedBy":"me",
            "assignedTo":assignee,"openedBuild":"trunk","deadline":"2026-11-20","mailto":"other",
            "steps":"&lt;按钮&gt; &amp; 下一步<br />错误"
        })
    }

    fn request_body(request: &str) -> Value {
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    fn multipart_server(
        responses: Vec<(u16, Value)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        media::tests::request_server(
            responses
                .into_iter()
                .map(|(status, body)| {
                    (
                        status,
                        "application/json".into(),
                        body.to_string().into_bytes(),
                    )
                })
                .collect(),
        )
    }

    fn rich_draft(images: usize) -> Draft {
        let mut value = draft("me");
        let data = STANDARD.encode(crate::bug_steps::fixture_png());
        let images =
            format!(r#"<img alt="复现截图" src="data:image/png;base64,{data}" onerror="bad()">"#)
                .repeat(images);
        value["stepsFormat"] = json!("html");
        value["steps"] = json!(format!(
            r#"<p onclick="bad()"><strong>复现</strong>{images}<img src="https://external.invalid/image.png"></p>"#
        ));
        Draft::parse(value).unwrap()
    }

    fn uploaded(id: u32) -> (u16, Value) {
        (
            200,
            json!({"id":id,"url":format!("index.php?m=file&f=read&fileID={id}&t=png")}),
        )
    }

    #[tokio::test]
    async fn rich_create_binds_uid_and_recovers_unverified_attachment_by_get() {
        let mut responses = presubmit();
        let mut missing = remote("me");
        missing["steps"] = json!(
            r#"<p><strong>复现</strong><img src="https://external.invalid/file-read-708.png"></p>"#
        );
        let mut confirmed = remote("me");
        confirmed["steps"] = json!(
            r#"<p><strong>复现</strong><img src="/index.php?m=file&amp;f=read&amp;fileID=708&amp;t=png"></p>"#
        );
        responses.extend([
            creator(),
            uploaded(708),
            (201, missing),
            creator(),
            (200, confirmed.clone()),
        ]);
        let (base, server) = multipart_server(responses);
        let (_root, state, ctx, adapter) = fixture(&base, "v1");
        let draft = rich_draft(1);
        let error = create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err();
        assert!(error.contains("缺少本次上传的图片引用"));
        assert!(state.store.lock().unwrap().snapshot().unwrap()["bugs"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            create_with_adapter(&state, &ctx, &adapter, &draft)
                .await
                .unwrap(),
            json!({"remoteId":"53","assignedToSelf":true})
        );
        let requests = server.join().unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            2
        );
        assert!(requests[6].starts_with(&format!("POST /api.php/v1/files?uid={} ", draft.id)));
        assert!(requests[6].contains("name=\"imgFile\"; filename=\"bug-step.png\""));
        assert!(requests[6]
            .to_ascii_lowercase()
            .contains("content-type: image/png"));
        assert!(requests[7].starts_with("POST /api.php/v1/products/11/bugs "));
        assert!(requests[6..=7]
            .iter()
            .all(|r| r.to_ascii_lowercase().contains("token: mock-token")));
        let posted = request_body(&requests[7]);
        assert_eq!(posted["uid"], json!(draft.id));
        assert_eq!(
            posted["steps"],
            r#"<p><strong>复现</strong><img src="{708.png}" alt="复现截图"></p>"#
        );
        assert!(!requests[7].contains("data:image/"));
        assert!(requests[9].starts_with("GET /api.php/v1/bugs/53 "));
        let store = state.store.lock().unwrap();
        assert_eq!(
            store.snapshot().unwrap()["bugs"][0]["steps"],
            confirmed["steps"]
        );
        let intent = read_intent(&store, &format!("zentao-bug-create:{}", draft.id))
            .unwrap()
            .unwrap();
        assert_eq!(intent.image_ids, ["708"]);
        assert_eq!(intent.assigned_to_self, Some(true));
    }

    #[tokio::test]
    async fn lost_second_upload_response_keeps_confirmed_id_and_blocks_all_posts_on_retry() {
        let mut responses = presubmit();
        responses.extend([creator(), uploaded(708), (0, Value::Null), creator()]);
        let (base, server) = multipart_server(responses);
        let (root, state, ctx, adapter) = fixture(&base, "v1");
        let draft = rich_draft(2);
        let failed = create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err();
        assert!(failed.contains("BUG 尚未提交"));
        assert!(failed.contains("已确认上传 1 张"));
        assert!(failed.contains("上传结果未知"));
        drop(state);

        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let intent = read_intent(&store, &format!("zentao-bug-create:{}", draft.id))
            .unwrap()
            .unwrap();
        assert!(matches!(intent.stage, IntentStage::Uploading));
        assert_eq!(intent.image_ids, ["708"]);
        assert!(intent.remote_id.is_none());
        let raw = serde_json::to_string(&intent).unwrap();
        assert!(!raw.contains("data:image/") && !raw.contains("<strong>"));
        let state = AppState {
            store: Mutex::new(store),
        };
        let retry = create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err();
        assert!(retry.contains("BUG 尚未提交") && retry.contains("已阻止同一草稿重复上传"));
        let requests = server.join().unwrap();
        let posts: Vec<_> = requests.iter().filter(|r| r.starts_with("POST ")).collect();
        assert_eq!(posts.len(), 2);
        assert!(posts
            .iter()
            .all(|r| r.starts_with(&format!("POST /api.php/v1/files?uid={} ", draft.id))));
    }

    #[tokio::test]
    async fn v1_posts_to_product_and_caches_self_then_returns_saved_receipt() {
        let mut responses = presubmit();
        responses.insert(
            3,
            (
                200,
                json!({"id":"9","project":27,"product":11,"execution":32}),
            ),
        );
        let mut bug = remote("me");
        bug["openedBuild"] = json!("9,trunk");
        bug["assignedTo"] = json!({"account":"me","realname":"我"});
        bug["mailto"] = json!([{"account":"other","realname":"同事"}]);
        responses.extend([(201, bug), creator()]);
        let (base, server) = auth_server(responses);
        let (_root, state, ctx, adapter) = fixture(&base, "v1");
        let mut value = draft("me");
        value["openedBuild"] = json!(["trunk", "009"]);
        let draft = Draft::parse(value).unwrap();
        let expected = json!({"remoteId":"53","assignedToSelf":true});
        assert_eq!(
            create_with_adapter(&state, &ctx, &adapter, &draft)
                .await
                .unwrap(),
            expected
        );
        assert_eq!(
            create_with_adapter(&state, &ctx, &adapter, &draft)
                .await
                .unwrap(),
            expected
        );

        let requests = server.join().unwrap();
        assert!(requests[1].starts_with("GET /api.php/v1/projects/27?fields=products "));
        assert!(requests[3].starts_with("GET /api.php/v1/builds/9 "));
        assert!(requests[6].starts_with("POST /api.php/v1/products/11/bugs "));
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        assert_eq!(
            request_body(&requests[6]),
            json!({
                "title":"导出报错","steps":"&lt;按钮&gt; &amp; 下一步<br />错误",
                "type":"codeerror","severity":2,"pri":3,"project":27,"execution":32,
                "assignedTo":"me","openedBuild":["9","trunk"],"deadline":"2026-11-20","mailto":["other"]
            })
        );
        let store = state.store.lock().unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["bugs"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["bugs"][0]["remoteId"], "53");
        assert_eq!(snapshot["bugs"][0]["assignedTo"], "me");
        assert_eq!(snapshot["bugs"][0]["revision"], 1);
    }

    #[tokio::test]
    async fn v2_detail_failure_recovers_by_get_only_and_does_not_cache_other_assignee() {
        let mut responses = presubmit();
        responses.extend([
            (200, json!({"status":"success","id":53})),
            (503, json!({})),
            creator(),
            (200, json!({"bug":remote("other"),"actions":[]})),
        ]);
        let (base, server) = auth_server(responses);
        let (_root, state, ctx, adapter) = fixture(&base, "v2");
        let draft = Draft::parse(draft("other")).unwrap();
        assert!(create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err()
            .contains("#53"));
        assert_eq!(
            create_with_adapter(&state, &ctx, &adapter, &draft)
                .await
                .unwrap(),
            json!({"remoteId":"53","assignedToSelf":false})
        );
        let requests = server.join().unwrap();
        assert!(requests[5].starts_with("POST /api.php/v2/bugs "));
        let body = request_body(&requests[5]);
        assert_eq!(body["productID"], 11);
        assert_eq!(body["openedBuild"], json!(["trunk"]));
        assert_eq!(body["assignedTo"], "other");
        assert_eq!(body["mailto"], json!(["other"]));
        assert_eq!(body["deadline"], "2026-11-20");
        for index in [6, 8] {
            assert!(requests[index].starts_with("GET /api.php/v2/bugs/53 "));
        }
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        let snapshot = state.store.lock().unwrap().snapshot().unwrap();
        assert!(snapshot["bugs"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_post_persists_a_fingerprint_and_blocks_retry_after_reopen() {
        let mut responses = presubmit();
        responses.extend([(503, json!({})), creator(), creator()]);
        let (base, server) = auth_server(responses);
        let (root, state, ctx, adapter) = fixture(&base, "v1");
        let value = draft("me");
        let draft = Draft::parse(value.clone()).unwrap();
        assert!(create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err()
            .contains("503"));
        drop(state);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let workspace = store.workspace_id.clone();
        let key = format!("zentao-bug-create:{}", draft.id);
        let intent = read_intent(&store, &key).unwrap().unwrap();
        let raw = serde_json::to_string(&intent).unwrap();
        assert_eq!(intent.request_hash.len(), 64);
        assert!(intent.remote_id.is_none());
        assert!(!raw.contains("导出报错"));
        let state = AppState {
            store: Mutex::new(store),
        };
        let ctx = Context::load(&state, &workspace, "local-project").unwrap();
        assert!(create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err()
            .contains("已阻止"));
        let mut changed = value;
        changed["title"] = json!("不同标题");
        let changed = Draft::parse(changed).unwrap();
        assert!(create_with_adapter(&state, &ctx, &adapter, &changed)
            .await
            .unwrap_err()
            .contains("不能修改"));
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
    async fn execution_options_include_real_products_and_cross_product_selection_never_posts() {
        let (base, server) = auth_server(vec![
            (
                200,
                json!({"page":1,"total":1,"limit":100,"executions":[{"id":"32","project":27,"name":"执行 A"}]}),
            ),
            execution(99),
            creator(),
            project(),
            execution(99),
        ]);
        let (_root, state, ctx, adapter) = fixture(&base, "v1");
        assert_eq!(
            executions(&adapter, "27", false).await.unwrap(),
            vec![json!({"id":"32","name":"执行 A","productIds":["99"]})]
        );
        let draft = Draft::parse(draft("me")).unwrap();
        assert!(create_with_adapter(&state, &ctx, &adapter, &draft)
            .await
            .unwrap_err()
            .contains("未关联该产品"));
        assert!(server
            .join()
            .unwrap()
            .iter()
            .all(|r| !r.starts_with("POST ")));
        let store = state.store.lock().unwrap();
        assert!(
            read_intent(&store, &format!("zentao-bug-create:{}", draft.id))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn refuses_unverified_returned_assignment_or_missing_scope() {
        let draft = Draft::parse(draft("me")).unwrap();
        for (key, value) in [
            ("assignedTo", json!("other")),
            ("project", Value::Null),
            ("execution", json!(99)),
        ] {
            let mut response = remote("me");
            response[key] = value;
            assert!(verify_created(&response, "53", "27", &draft, "me").is_err());
        }
    }
}
