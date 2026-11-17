//! 禅道 22.0 API v1 BUG：按令牌账号同步本人指派及本人已解决记录；写入不自动重放。
use super::*;

fn account_of(value: &Value) -> Result<String, String> {
    // 22.0 的 user 格式转换对未指派、closed 等非用户账号返回 null。
    if value.is_null() {
        return Ok(String::new());
    }
    value
        .as_str()
        .or_else(|| value["account"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| "禅道 BUG 缺少有效指派账号，本次同步未发布".into())
}
fn assigned_account(remote: &Value) -> Result<String, String> {
    account_of(
        remote
            .get("assignedTo")
            .ok_or("禅道 BUG 缺少指派字段，请重新同步")?,
    )
}

fn resolved_by_me(remote: &Value, account: &str) -> bool {
    // v1 列表/详情使用 user 对象；v1 action 和 v2 列表可直接返回账号字符串。
    let resolver = &remote["resolvedBy"];
    !account.is_empty()
        && matches!(text(remote, "status"), "resolved" | "closed")
        && resolver.as_str().or_else(|| resolver["account"].as_str()) == Some(account)
}

fn reference(value: &Value) -> Option<String> {
    remote_id(value).or_else(|_| remote_id(&value["id"])).ok()
}
fn confirmation(remote: &Value) -> Result<Value, String> {
    match remote.get("confirmed") {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(value) if value == true || value == 1 || value == "1" => Ok(json!(true)),
        Some(value) if value == false || value == 0 || value == "0" => Ok(json!(false)),
        _ => Err("禅道 BUG 确认字段无效，请重新同步".into()),
    }
}
pub(super) fn normalized(
    remote: &Value,
    connection: &str,
    old: Option<&Value>,
) -> Result<Value, String> {
    if remote["deleted"] == true || remote["deleted"] == "1" {
        return Err("禅道 BUG 已删除，请重新同步".into());
    }
    let id = remote_id(&remote["id"])?;
    let status = text(remote, "status");
    if !["active", "resolved", "closed"].contains(&status)
        || text(remote, "title").trim().is_empty()
    {
        return Err("禅道 BUG 返回了无效状态或名称".into());
    }
    let mut value = json!({"id":format!("zentao-bug-{connection}-{id}"),"connectionId":connection,"remoteId":id,"title":remote["title"],"steps":text(remote,"steps"),"status":status,"assignedTo":assigned_account(remote)?,"severity":remote["severity"].as_u64().or_else(||remote["severity"].as_str()?.parse().ok()).unwrap_or(3),"priority":remote["pri"].as_u64().or_else(||remote["pri"].as_str()?.parse().ok()).unwrap_or(3)});
    if let Some(old) = old {
        value["id"] = old["id"].clone();
        value["revision"] = old["revision"].clone();
    }
    value["confirmed"] = confirmation(remote)?;
    value["lastAssignedTo"] = if text(&value, "assignedTo").is_empty() {
        old.map(|row| row["lastAssignedTo"].clone())
            .unwrap_or(Value::Null)
    } else {
        value["assignedTo"].clone()
    };
    for (local, key) in [("productId", "product"), ("projectId", "project")] {
        if let Some(id) = reference(&remote[key]) {
            value[local] = json!(id);
        }
    }
    for key in [
        "productName",
        "projectName",
        "resolution",
        "openedDate",
        "resolvedDate",
        "deadline",
    ] {
        if !text(remote, key).is_empty() && !text(remote, key).starts_with("0000-") {
            value[key] = remote[key].clone();
        }
    }
    if let Some(name) = remote["assignedTo"]["realname"].as_str() {
        value["assignedToName"] = json!(name);
    }
    // 只保留冲突校验所需版本，避免把远端 action 历史和文件正文复制进工作空间。
    value["remoteEditedDate"] = remote["lastEditedDate"].clone();
    Ok(value)
}

fn mark_resolved_scope(remote: &Value, value: &mut Value, account: &str, personally_closed: bool) {
    if resolved_by_me(remote, account) {
        value["resolvedByMe"] = json!(true);
        if text(value, "assignedTo") != account && !personally_closed {
            value["readOnly"] = json!(true);
        }
    }
}

impl Adapter<'_> {
    pub(super) async fn bug_request(
        &self,
        id: &str,
        operation: Option<(&str, &Value)>,
    ) -> Result<Value, String> {
        let path = match operation {
            Some(("edit", _)) | None => format!("bugs/{id}"),
            Some((action, _)) => format!("bugs/{id}/{action}"),
        };
        let url = endpoint(&self.base, &path)?;
        let response = if let Some((action, payload)) = operation {
            let request = if action == "edit" {
                self.client.put(url)
            } else {
                self.client.post(url)
            };
            request
                .header("Token", self.token()?)
                .json(payload)
                .send()
                .await
                .map_err(|_| "BUG 写入结果未知，请重新同步核对，勿直接重复提交")?
        } else {
            self.read(url).await?
        };
        let mut stream = checked_zentao(response).await?.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "BUG 响应读取中断，请重新同步核对")?;
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err("BUG 响应过大，请重新同步核对".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| "BUG 响应无效，请重新同步核对")?;
        if remote_id(&body["id"]).ok().as_deref() != Some(id)
            || body["deleted"] == true
            || body["deleted"] == "1"
        {
            return Err("禅道未返回有效 BUG 结果，请同步核对；未保存本地修改".into());
        }
        Ok(body)
    }
}

async fn fetch_bugs(adapter: &Adapter<'_>, account: &str) -> Result<Vec<Value>, String> {
    // v1 /bugs 必须指定 product；产品范围不能沿用 involved 项目范围，否则漏掉指派 BUG。
    // Adapter::all 给每页传 status=all，覆盖 22.0 BUG 列表默认的 unclosed。
    let products = adapter.all("products", "products").await?;
    let mut output = vec![];
    let mut ids = HashSet::new();
    for product in products {
        let id = remote_id(&product["id"])?;
        for bug in adapter.all(&format!("products/{id}/bugs"), "bugs").await? {
            let assignee = assigned_account(&bug)?;
            let bug_id = remote_id(&bug["id"])?;
            if !ids.insert(bug_id.clone()) {
                return Err("BUG 在多个产品重复出现，请重新同步".into());
            }
            if reference(&bug["product"]).as_deref() != Some(&id) {
                return Err("BUG 产品关联不一致，本次同步未发布".into());
            }
            let resolved = resolved_by_me(&bug, account);
            if (assignee != account && !resolved) || bug["deleted"] == true || bug["deleted"] == "1"
            {
                continue;
            }
            // 列表可能省略 steps；详情提供编辑基线和产品名。
            let mut detail = adapter.bug_request(&bug_id, None).await?;
            if assigned_account(&detail)? != assignee
                || (resolved && !resolved_by_me(&detail, account))
                || reference(&detail["product"]).as_deref() != Some(&id)
            {
                return Err("BUG 指派、解决归属或产品在同步期间变化，请重新同步".into());
            }
            detail["productName"] = product["name"].clone();
            output.push(detail);
        }
    }
    Ok(output)
}

#[tauri::command]
pub async fn zentao_bugs_sync(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<String, String> {
    sync_inner(&state, connection_id, None, None).await
}

pub(super) async fn sync_inner(
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
        && super::background::interval_minutes(connection.get("syncIntervalMinutes"))? == 0
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
    let remote = fetch_bugs(&adapter, &account).await?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace || store.generation != generation {
        return Err("工作空间已切换，请重新同步 BUG".into());
    }
    let snapshot = store.snapshot()?;
    let old = snapshot["bugs"].as_array().ok_or("BUG 缓存无效")?;
    let values = remote
        .iter()
        .map(|bug| {
            let id = remote_id(&bug["id"])?;
            let mut value = normalized(
                bug,
                &connection_id,
                old.iter().find(|v| {
                    text(v, "connectionId") == connection_id && text(v, "remoteId") == id
                }),
            )?;
            mark_resolved_scope(bug, &mut value, &account, false);
            Ok(value)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let count = values.len();
    let assigned = values
        .iter()
        .filter(|bug| text(bug, "assignedTo") == account)
        .count();
    store.save_bug_snapshot_for_account(values, &connection, &account)?;
    Ok(format!(
        "同步完成：{assigned} 个指派给我的 BUG，{} 个我已解决的只读 BUG",
        count - assigned
    ))
}

fn verify_fields(old: &Value, current: &Value) -> Result<(), String> {
    if old.get("confirmed").unwrap_or(&Value::Null) != &current["confirmed"] {
        return Err("BUG 远端确认状态已变化，请重新同步".into());
    }
    for key in [
        "remoteId",
        "title",
        "steps",
        "status",
        "assignedTo",
        "severity",
        "priority",
        "productId",
        "projectId",
        "remoteEditedDate",
    ] {
        if current.get(key) != old.get(key) {
            return Err(format!("BUG 远端数据已变化（{key}），请重新同步"));
        }
    }
    Ok(())
}

pub(super) fn verify_baseline(old: &Value, remote: &Value, account: &str) -> Result<(), String> {
    if old["readOnly"] == true {
        return Err("该 BUG 仅供查看，不能修改".into());
    }
    let current = normalized(remote, text(old, "connectionId"), Some(old))?;
    // 本人刚关闭的缓存允许在下次同步前重新激活；只读记录不能使用此例外。
    let known_closed = text(old, "status") == "closed"
        && text(&current, "status") == "closed"
        && text(old, "assignedTo").is_empty()
        && text(&current, "assignedTo").is_empty()
        && text(old, "lastAssignedTo") == account
        && remote
            .get("closedBy")
            .is_none_or(|by| account_of(by).is_ok_and(|closed_by| closed_by == account));
    if text(&current, "assignedTo") != account && !known_closed {
        return Err("该 BUG 已不再指派给当前账号，请重新同步".into());
    }
    verify_fields(old, &current)
}

pub(super) fn verify_read_baseline(
    old: &Value,
    remote: &Value,
    account: &str,
) -> Result<(), String> {
    if old["readOnly"] != true {
        return verify_baseline(old, remote, account);
    }
    if old["resolvedByMe"] != true
        || !resolved_by_me(remote, account)
        || text(old, "assignedTo") == account
    {
        return Err("该 BUG 不再符合本人已解决的查看范围，请重新同步".into());
    }
    let current = normalized(remote, text(old, "connectionId"), Some(old))?;
    if text(&current, "remoteEditedDate").is_empty() {
        return Err("禅道未返回可核对的 BUG 版本，请重新同步".into());
    }
    verify_fields(old, &current)?;
    if current.get("resolution") != old.get("resolution") {
        return Err("BUG 远端解决方案已变化，请重新同步".into());
    }
    Ok(())
}
fn edit_payload(value: &Value) -> Result<Value, String> {
    if text(value, "title").trim().is_empty() {
        return Err("BUG 标题不能为空".into());
    }
    for key in ["severity", "priority"] {
        if !value[key].as_u64().is_some_and(|v| (1..=4).contains(&v)) {
            return Err("严重程度和优先级必须为 1–4".into());
        }
    }
    if text(value, "assignedTo").trim().is_empty() {
        return Err("请填写指派账号".into());
    }
    Ok(
        json!({"title":value["title"],"steps":text(value,"steps"),"severity":value["severity"],"pri":value["priority"],"assignedTo":value["assignedTo"]}),
    )
}
fn activation_input(input: &Value, remote: &Value) -> Result<Value, String> {
    let mut input = input.clone();
    if input.get("openedBuild").is_none() {
        // GET 详情返回 [{id,title}]；未改版本时使用刚读取的远端值。
        let builds = remote["openedBuild"]
            .as_array()
            .ok_or("无法读取原影响版本，请填写影响版本后重试")?;
        let ids: Result<Vec<Value>, String> = builds
            .iter()
            .map(|build| {
                let id = build.get("id").unwrap_or(build);
                if id == "trunk" {
                    Ok(json!("trunk"))
                } else {
                    remote_id(id).map(Value::String)
                }
            })
            .collect();
        input["openedBuild"] = json!(ids?);
    }
    Ok(input)
}

fn transition_payload(
    action: &str,
    input: &Value,
    old: &Value,
) -> Result<(&'static str, Value, &'static str), String> {
    let mut payload = json!({"comment":text(input,"comment")});
    match action {
        "confirm" => {
            if text(old, "status") != "active" {
                return Err("只有激活的 BUG 可以确认".into());
            }
            match old["confirmed"].as_bool() {
                Some(false) => (),
                Some(true) => return Err("该 BUG 已确认，请刷新列表".into()),
                None => return Err("BUG 确认状态未知，请先同步".into()),
            }
            Ok(("confirm", payload, "active"))
        }
        "assign" => {
            if text(old, "status") == "closed" {
                return Err("已关闭 BUG 不能指派，请先激活".into());
            }
            let account = text(input, "assignedTo").trim();
            if account.is_empty() || account == "closed" {
                return Err("请选择有效指派账号".into());
            }
            payload["assignedTo"] = json!(account);
            Ok((
                "assign",
                payload,
                if text(old, "status") == "resolved" {
                    "resolved"
                } else {
                    "active"
                },
            ))
        }
        "resolve" => {
            if text(old, "status") != "active" {
                return Err("只有激活的 BUG 可以解决".into());
            }
            let resolution = text(input, "resolution");
            if ![
                "fixed",
                "notrepro",
                "bydesign",
                "duplicate",
                "external",
                "postponed",
                "willnotfix",
                "tostory",
            ]
            .contains(&resolution)
            {
                return Err("请选择有效解决方案".into());
            }
            payload["resolution"] = json!(resolution);
            payload["resolvedDate"] = json!(chrono::Local::now().format("%Y-%m-%d").to_string());
            if resolution == "fixed" {
                let build = text(input, "resolvedBuild");
                if build != "trunk" {
                    remote_id(&input["resolvedBuild"])?;
                }
                payload["resolvedBuild"] = input["resolvedBuild"].clone();
            }
            if resolution == "duplicate" {
                payload["duplicateBug"] = json!(remote_id(&input["duplicateBug"])?);
            }
            payload["assignedTo"] = json!(if text(input, "assignedTo").is_empty() {
                text(old, "assignedTo")
            } else {
                text(input, "assignedTo")
            });
            Ok(("resolve", payload, "resolved"))
        }
        "close" => {
            if text(old, "status") != "resolved" {
                return Err("请先解决 BUG 再关闭".into());
            }
            Ok(("close", payload, "closed"))
        }
        "activate" => {
            if text(old, "status") == "active" {
                return Err("该 BUG 已处于激活状态".into());
            }
            let builds = input["openedBuild"]
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("请填写激活版本")?;
            for build in builds {
                if build != "trunk" {
                    remote_id(build)?;
                }
            }
            payload["openedBuild"] = json!(builds);
            let account = text(input, "assignedTo");
            if account.trim().is_empty() {
                return Err("请填写激活后的指派账号".into());
            }
            payload["assignedTo"] = json!(account);
            Ok(("active", payload, "active"))
        }
        _ => Err("未知 BUG 操作".into()),
    }
}

fn verify_confirmation_result(action: &str, result: &Value, remote: &Value) -> Result<(), String> {
    if action == "confirm" {
        if text(result, "assignedTo") != account_of(&remote["assignedTo"])? {
            return Err("禅道确认后指派人与预期不一致，请同步核对；未保存本地修改".into());
        }
        if result["confirmed"] != true {
            return Err("禅道未确认 BUG，请同步核对；未保存本地修改".into());
        }
    } else if action == "assign" && result["confirmed"] != confirmation(remote)? {
        return Err("禅道确认状态与预期不一致，请同步核对；未保存本地修改".into());
    }
    Ok(())
}

fn normalize_write_result(
    result: &Value,
    old: &Value,
    account: &str,
    action: &str,
    status: &str,
) -> Result<(Value, bool), String> {
    let mut value = normalized(result, text(old, "connectionId"), Some(old))?;
    if text(&value, "status") != status
        || value["remoteId"] != old["remoteId"]
        || value["productId"] != old["productId"]
        || value["projectId"] != old["projectId"]
    {
        return Err("禅道处理结果与预期不一致，请同步核对；未保存本地修改".into());
    }
    if action == "resolve" && !resolved_by_me(result, account) {
        return Err("禅道未确认当前账号是解决人，请同步核对；未保存本地修改".into());
    }
    // 只有本次经验证由本人关闭的记录保留旧的重激活入口。同步补入的记录不享有该例外。
    let personally_closed = action == "close"
        && text(old, "assignedTo") == account
        && text(&value, "assignedTo").is_empty();
    if personally_closed {
        if result
            .get("closedBy")
            .is_some_and(|by| account_of(by).map_or(true, |closed_by| closed_by != account))
        {
            return Err("禅道未确认当前账号是关闭人，请同步核对；未保存本地修改".into());
        }
        value["lastAssignedTo"] = json!(account);
    }
    mark_resolved_scope(result, &mut value, account, personally_closed);
    let keep =
        text(&value, "assignedTo") == account || value["readOnly"] == true || personally_closed;
    Ok((value, keep))
}

fn persist_write_result(
    store: &crate::storage::Store,
    old: &Value,
    connection: &Value,
    value: Value,
    keep: bool,
) -> Result<Value, String> {
    if !keep {
        store
            .remove_transferred_bug(old, connection)
            .map_err(|error| format!("本地缓存更新失败，禅道可能已更新，请重新同步：{error}"))?;
        return Ok(value);
    }
    store
        .save_batch(
            vec![("bug", value)],
            vec![
                (
                    "connection",
                    text(connection, "id").into(),
                    connection["revision"].as_i64().ok_or("连接版本无效")?,
                ),
                (
                    "bug",
                    text(old, "id").into(),
                    old["revision"].as_i64().ok_or("BUG 缺少版本")?,
                ),
            ],
            true,
        )
        .map_err(|e| format!("本地保存失败，禅道可能已更新，请重新同步：{e}"))?
        .pop()
        .ok_or("BUG 保存结果缺失".into())
}

async fn mutate(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    edit: Option<Value>,
    transition: Option<(String, Value)>,
) -> Result<Value, String> {
    let (old, connection, workspace, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let snapshot = store.snapshot()?;
        let old = get(&snapshot, "bugs", &id)?;
        let connection = get(&snapshot, "connections", text(&old, "connectionId"))?;
        (
            old,
            connection,
            store.workspace_id.clone(),
            store.generation.clone(),
        )
    };
    if old["revision"] != revision {
        return Err("BUG 已修改，请刷新后重试".into());
    }
    if old["readOnly"] == true {
        return Err("该 BUG 仅供查看，不能修改".into());
    }
    if connection["enabled"] == false {
        return Err("该禅道连接已停用".into());
    }
    let connection_id = text(&connection, "id");
    let _guard = SyncGuard::acquire(&workspace, connection_id)?;
    let key = credential(&workspace, connection_id)?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter = Adapter::personal(&connection, key)?.with_auth(
        crate::zentao_auth::AutoAuth::new(&state, &workspace, &generation, &connection)?,
    );
    let account = adapter.account().await?;
    let remote_id = remote_id(&old["remoteId"])?;
    let remote = adapter.bug_request(&remote_id, None).await?;
    verify_baseline(&old, &remote, &account)?;
    if text(&old, "assignedTo") != account
        && transition
            .as_ref()
            .is_none_or(|(action, _)| action != "activate")
    {
        return Err("本人已关闭且未指派的 BUG 只能重新激活".into());
    }
    let (action, payload, status) = if let Some(edit) = &edit {
        for key in [
            "connectionId",
            "remoteId",
            "status",
            "confirmed",
            "productId",
            "projectId",
        ] {
            if edit.get(key) != old.get(key) {
                return Err("BUG 来源、状态及关联不能通过编辑修改".into());
            }
        }
        ("edit", edit_payload(edit)?, text(&old, "status"))
    } else {
        let (action, input) = transition.as_ref().ok_or("缺少 BUG 操作")?;
        let input = if action == "activate" {
            activation_input(input, &remote)?
        } else {
            input.clone()
        };
        transition_payload(action, &input, &old)?
    };
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace || store.generation != generation {
            return Err("工作空间已切换，未提交修改".into());
        }
        let snapshot = store.snapshot()?;
        if get(&snapshot, "connections", connection_id)?["revision"] != connection["revision"]
            || get(&snapshot, "bugs", &id)?["revision"] != revision
        {
            return Err("BUG 或连接已变化，未提交修改".into());
        }
    }
    let result = adapter
        .bug_request(&remote_id, Some((action, &payload)))
        .await?;
    let (mut value, keep) = normalize_write_result(&result, &old, &account, action, status)?;
    verify_confirmation_result(action, &value, &remote)?;
    if let Some(edit) = edit {
        for key in ["title", "steps", "severity", "priority", "assignedTo"] {
            if value[key] != edit[key] {
                return Err(format!("禅道未确认 {key} 修改，请同步核对"));
            }
        }
    } else {
        for key in ["resolution", "assignedTo"] {
            if payload.get(key).is_some() && value[key] != payload[key] {
                return Err(format!("禅道未确认 {key} 操作结果，请同步核对"));
            }
        }
    }
    for key in ["productName", "projectName"] {
        if value.get(key).is_none() {
            if let Some(name) = old.get(key) {
                value[key] = name.clone();
            }
        }
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace || store.generation != generation {
        return Err("工作空间已切换，禅道可能已更新，请重新同步".into());
    }
    persist_write_result(&store, &old, &connection, value, keep)
}
#[tauri::command]
pub async fn zentao_bug_save(state: State<'_, AppState>, value: Value) -> Result<Value, String> {
    mutate(
        state,
        text(&value, "id").into(),
        value["revision"].as_i64().ok_or("BUG 缺少版本")?,
        Some(value),
        None,
    )
    .await
}
#[tauri::command]
pub async fn zentao_bug_transition(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    action: String,
    payload: Value,
) -> Result<Value, String> {
    mutate(state, id, revision, None, Some((action, payload))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zentao::tests::auth_server;
    fn bug(id: u64, account: &str) -> Value {
        json!({"id":id,"title":"测试 BUG","steps":"<p>复现</p>","product":1,"project":0,"status":"active","assignedTo":{"account":account,"realname":"测试用户"},"severity":2,"pri":3,"lastEditedDate":"2026-09-08T12:00:00+08:00"})
    }
    fn adapter(base: &str) -> Adapter<'static> {
        Adapter::personal(
            &json!({"baseUrl":base,"apiVersion":"v2"}),
            "test-token".into(),
        )
        .unwrap()
    }
    #[tokio::test]
    async fn unassigned_and_closed_null_users_do_not_abort_personal_sync() {
        let mut unassigned = bug(2, "unused");
        unassigned["assignedTo"] = Value::Null;
        let mut closed = bug(3, "unused");
        closed["assignedTo"] = Value::Null;
        closed["status"] = json!("closed");
        assert_eq!(normalized(&closed, "c", None).unwrap()["assignedTo"], "");
        let (base, server) = auth_server(vec![
            (
                200,
                json!({"page":1,"total":1,"limit":100,"products":[{"id":1,"name":"产品"}]}),
            ),
            (
                200,
                json!({"page":1,"total":3,"limit":100,"bugs":[bug(1,"me"),unassigned,closed]}),
            ),
            (200, bug(1, "me")),
        ]);
        let result = fetch_bugs(&adapter(&base), "me").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["id"], 1);
        assert_eq!(server.join().unwrap().len(), 3);
    }
    #[test]
    fn activating_without_new_builds_keeps_remote_build_ids() {
        let input = json!({"assignedTo":"me"});
        let remote =
            json!({"openedBuild":[{"id":42,"title":"版本 42"},{"id":"trunk","title":"主干"}]});
        assert_eq!(
            activation_input(&input, &remote).unwrap()["openedBuild"],
            json!(["42", "trunk"])
        );
        let changed = json!({"assignedTo":"me","openedBuild":["52"]});
        assert_eq!(activation_input(&changed, &remote).unwrap(), changed);
        assert!(activation_input(&input, &json!({})).is_err());
    }
    #[test]
    fn only_known_personally_closed_cache_can_be_reopened() {
        let mut remote = bug(1, "me");
        remote["status"] = json!("resolved");
        remote["resolvedBy"] = json!({"account":"me"});
        let old = normalized(&remote, "c", None).unwrap();
        let mut closed = remote.clone();
        closed["status"] = json!("closed");
        closed["assignedTo"] = Value::Null;
        closed["closedBy"] = json!({"account":"me"});
        assert!(verify_baseline(&old, &closed, "me").is_err());
        let (cached_closed, keep) =
            normalize_write_result(&closed, &old, "me", "close", "closed").unwrap();
        assert!(keep);
        assert_eq!(cached_closed["resolvedByMe"], true);
        assert_ne!(cached_closed["readOnly"], true);
        verify_baseline(&cached_closed, &closed, "me").unwrap();
        assert!(verify_baseline(&cached_closed, &closed, "other").is_err());
        let mut synced = normalized(&closed, "c", Some(&cached_closed)).unwrap();
        mark_resolved_scope(&closed, &mut synced, "me", false);
        assert!(verify_baseline(&synced, &closed, "me").is_err());
        verify_read_baseline(&synced, &closed, "me").unwrap();
        let mut closed_by_other = closed.clone();
        closed_by_other["closedBy"] = json!("other");
        assert!(verify_baseline(&cached_closed, &closed_by_other, "me").is_err());
        assert!(normalize_write_result(&closed_by_other, &old, "me", "close", "closed").is_err());
        closed["title"] = json!("别人关闭后又修改了标题");
        assert!(verify_baseline(&cached_closed, &closed, "me").is_err());
    }
    #[tokio::test]
    async fn all_products_and_pages_filter_assigned_account_without_project_task_queries() {
        let (base, server) = auth_server(vec![
            (
                200,
                json!({"page":1,"total":1,"limit":100,"products":[{"id":1,"name":"所有可见产品"}]}),
            ),
            (
                200,
                json!({"page":1,"total":2,"limit":1,"bugs":[bug(1,"me")]}),
            ),
            (
                200,
                json!({"page":2,"total":2,"limit":1,"bugs":[bug(2,"other")]}),
            ),
            (200, bug(1, "me")),
        ]);
        let bugs = fetch_bugs(&adapter(&base), "me").await.unwrap();
        assert_eq!(bugs.len(), 1);
        assert_eq!(bugs[0]["productName"], "所有可见产品");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api.php/v1/products?"));
        assert!(requests[1].contains("/products/1/bugs?"));
        assert!(requests[1].contains("status=all"));
        assert!(requests[2].contains("page=2"));
        assert!(requests[2].contains("status=all"));
        assert!(requests
            .iter()
            .all(|r| !r.contains("/tasks") && !r.contains("involved=")));
    }
    #[tokio::test]
    async fn sync_includes_only_same_account_terminal_resolutions_and_checks_detail() {
        let terminal = |id, assignee, status, resolver: Value| {
            let mut remote = bug(id, assignee);
            remote["status"] = json!(status);
            remote["resolvedBy"] = resolver;
            remote
        };
        let owned = terminal(1, "me", "resolved", json!({"account":"me"}));
        let transferred = terminal(2, "tester", "resolved", json!("me"));
        let mut closed = terminal(3, "closed", "closed", json!({"account":"me"}));
        closed["assignedTo"] = Value::Null;
        let mut missing = bug(4, "tester");
        missing["status"] = json!("resolved");
        let (base, server) = auth_server(vec![
            (
                200,
                json!({"profile":{"account":"me","realname":"测试用户"}}),
            ),
            (
                200,
                json!({"page":1,"total":1,"limit":100,"products":[{"id":1,"name":"产品"}]}),
            ),
            (
                200,
                json!({"page":1,"total":8,"limit":4,"bugs":[owned,transferred,missing,
                    terminal(5,"tester","resolved",Value::Null)]}),
            ),
            (
                200,
                json!({"page":2,"total":8,"limit":4,"bugs":[
                    terminal(6,"tester","resolved",json!("ME")),
                    terminal(7,"tester","closed",json!({"account":"other","realname":"me"})),
                    terminal(8,"tester","active",json!("me")),closed]}),
            ),
            (200, owned),
            (200, transferred),
            (200, closed),
        ]);
        let client = adapter(&base);
        let account = client.account().await.unwrap();
        let values = fetch_bugs(&client, &account).await.unwrap();
        assert_eq!(values.len(), 3);
        for (remote, (id, read_only)) in values.iter().zip([(1, false), (2, true), (3, true)]) {
            assert_eq!(remote["id"], id);
            let mut value = normalized(remote, "c", None).unwrap();
            mark_resolved_scope(remote, &mut value, &account, false);
            assert_eq!(value["resolvedByMe"], true);
            assert_eq!(value["readOnly"] == true, read_only);
            verify_read_baseline(&value, remote, &account).unwrap();
            assert_eq!(
                verify_baseline(&value, remote, &account).is_err(),
                read_only
            );
        }
        let requests = server.join().unwrap();
        for (index, page) in [(2, 1), (3, 2)] {
            assert!(requests[index].starts_with("GET /api.php/v1/products/1/bugs?"));
            assert!(requests[index].contains(&format!("page={page}")));
            assert!(requests[index].contains("status=all"));
        }

        // 列表声称是本人解决，但详情已改变；不能发布只读缓存或清理旧快照。
        let listed = terminal(2, "tester", "resolved", json!("me"));
        let no_longer_mine = terminal(2, "tester", "resolved", json!("other"));
        let (base, server) = auth_server(vec![
            (
                200,
                json!({"page":1,"total":1,"limit":100,"products":[{"id":1}]}),
            ),
            (200, json!({"page":1,"total":1,"limit":100,"bugs":[listed]})),
            (200, no_longer_mine),
        ]);
        assert!(fetch_bugs(&adapter(&base), "me").await.is_err());
        assert_eq!(server.join().unwrap().len(), 3);
    }

    #[test]
    fn read_only_baseline_requires_resolver_and_unchanged_remote_snapshot() {
        let mut remote = bug(1, "tester");
        remote["status"] = json!("resolved");
        remote["resolvedBy"] = json!({"account":"me"});
        remote["resolution"] = json!("fixed");
        let mut cached = normalized(&remote, "c", None).unwrap();
        mark_resolved_scope(&remote, &mut cached, "me", false);
        for (key, value) in [
            ("resolvedBy", json!("other")),
            ("resolvedBy", Value::Null),
            ("status", json!("active")),
            ("id", json!(2)),
            ("assignedTo", json!("someone_else")),
            ("product", json!(8)),
            ("project", json!(9)),
            ("lastEditedDate", json!("2026-11-17 09:30:00")),
            ("resolution", json!("duplicate")),
            ("deleted", json!(true)),
        ] {
            let mut changed = remote.clone();
            changed[key] = value;
            assert!(
                verify_read_baseline(&cached, &changed, "me").is_err(),
                "{key}"
            );
        }
        assert!(verify_read_baseline(&cached, &remote, "tester").is_err());
    }

    #[tokio::test]
    async fn resolving_to_another_account_keeps_read_only_cache_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("source")).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"公司"}))
            .unwrap();
        let old = store
            .save_bug_snapshot_for_account(
                vec![normalized(&bug(1, "me"), "c", None).unwrap()],
                &connection,
                "me",
            )
            .unwrap()
            .remove(0);
        let connection = get(&store.snapshot().unwrap(), "connections", "c").unwrap();
        let mut response = bug(1, "tester");
        response["status"] = json!("resolved");
        response["resolution"] = json!("fixed");
        response["resolvedBy"] = json!("me");
        let (base, server) = auth_server(vec![(200, response)]);
        let (action, payload, status) = transition_payload(
            "resolve",
            &json!({"assignedTo":"tester","resolution":"fixed","resolvedBuild":"trunk"}),
            &old,
        )
        .unwrap();
        let response = adapter(&base)
            .bug_request("1", Some((action, &payload)))
            .await
            .unwrap();
        let (value, keep) = normalize_write_result(&response, &old, "me", action, status).unwrap();
        let saved = persist_write_result(&store, &old, &connection, value, keep).unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["bugs"], json!([saved]));
        assert_eq!(saved["assignedTo"], "tester");
        assert_eq!(saved["resolvedByMe"], true);
        assert_eq!(saved["readOnly"], true);
        verify_read_baseline(&saved, &response, "me").unwrap();
        let mut wrong_resolver = response;
        wrong_resolver["resolvedBy"] = json!("tester");
        assert!(normalize_write_result(&wrong_resolver, &old, "me", action, status).is_err());
        let mut still_assigned = wrong_resolver.clone();
        still_assigned["assignedTo"] = json!("me");
        let prior_resolution = normalized(&still_assigned, "c", None).unwrap();
        let (_, keep) = normalize_write_result(
            &wrong_resolver,
            &prior_resolution,
            "me",
            "assign",
            "resolved",
        )
        .unwrap();
        assert!(!keep);
        let mut my_resolution = wrong_resolver;
        my_resolution["resolvedBy"] = json!("me");
        still_assigned["resolvedBy"] = json!("me");
        let mut prior_resolution = normalized(&still_assigned, "c", None).unwrap();
        mark_resolved_scope(&still_assigned, &mut prior_resolution, "me", false);
        let (retained, keep) = normalize_write_result(
            &my_resolution,
            &prior_resolution,
            "me",
            "assign",
            "resolved",
        )
        .unwrap();
        assert!(keep);
        assert_eq!(retained["readOnly"], true);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("POST /api.php/v1/bugs/1/resolve "));
        assert!(requests[0].contains(r#""assignedTo":"tester""#));
    }
    #[tokio::test]
    async fn duplicate_pages_and_changed_assignment_abort_snapshot() {
        for changed in [false, true] {
            let mut responses = vec![(
                200,
                json!({"page":1,"total":1,"limit":100,"products":[{"id":1,"name":"产品"}]}),
            )];
            if changed {
                responses.extend([
                    (
                        200,
                        json!({"page":1,"total":1,"limit":100,"bugs":[bug(1,"me")]}),
                    ),
                    (200, bug(1, "other")),
                ]);
            } else {
                responses.extend([
                    (200, json!({"bugs":[bug(1,"me")]})),
                    (200, json!({"bugs":[bug(1,"me")]})),
                ]);
            }
            let (base, server) = auth_server(responses);
            assert!(fetch_bugs(&adapter(&base), "me").await.is_err());
            server.join().unwrap();
        }
    }
    #[tokio::test]
    async fn edit_put_and_transitions_post_use_official_paths_and_do_not_replay_errors() {
        for (action, method, path) in [
            ("edit", "PUT", "bugs/1"),
            ("resolve", "POST", "bugs/1/resolve"),
            ("close", "POST", "bugs/1/close"),
            ("confirm", "POST", "bugs/1/confirm"),
            ("assign", "POST", "bugs/1/assign"),
            ("active", "POST", "bugs/1/active"),
        ] {
            let (base, server) = auth_server(vec![(200, bug(1, "me"))]);
            adapter(&base)
                .bug_request("1", Some((action, &json!({"comment":"测试"}))))
                .await
                .unwrap();
            let requests = server.join().unwrap();
            assert!(requests[0].starts_with(&format!("{method} /api.php/v1/{path} ")));
        }
        for status in [401, 403, 500] {
            let (base, server) = auth_server(vec![(status, json!({"error":"private"}))]);
            let error = adapter(&base)
                .bug_request("1", Some(("resolve", &json!({}))))
                .await
                .unwrap_err();
            assert!(!error.contains("private"));
            assert_eq!(server.join().unwrap().len(), 1);
        }
        let (base, server) = auth_server(vec![(200, json!({"status":"fail"}))]);
        assert!(adapter(&base)
            .bug_request("1", Some(("close", &json!({}))))
            .await
            .is_err());
        server.join().unwrap();
    }
    #[tokio::test]
    async fn action_response_must_target_requested_bug() {
        for action in ["confirm", "assign"] {
            let (base, server) = auth_server(vec![(200, bug(2, "me"))]);
            assert!(adapter(&base)
                .bug_request("1", Some((action, &json!({}))))
                .await
                .is_err());
            assert_eq!(server.join().unwrap().len(), 1);
        }
        let mut remote = bug(1, "me");
        remote["confirmed"] = json!(1);
        let mut result = normalized(&remote, "c", None).unwrap();
        result["assignedTo"] = json!("other");
        assert!(verify_confirmation_result("confirm", &result, &remote).is_err());
    }
    #[test]
    fn confirmation_values_conflicts_and_action_guards() {
        let mut remote = bug(1, "me");
        let unknown = normalized(&remote, "c", None).unwrap();
        assert!(unknown["confirmed"].is_null());
        assert!(transition_payload("confirm", &json!({}), &unknown).is_err());
        for raw in [json!(false), json!(0), json!("0")] {
            remote["confirmed"] = raw;
            let old = normalized(&remote, "c", None).unwrap();
            assert_eq!(old["confirmed"], false);
            let (action, payload, status) =
                transition_payload("confirm", &json!({"comment":"已复现"}), &old).unwrap();
            assert_eq!(action, "confirm");
            assert_eq!(status, "active");
            assert_eq!(payload, json!({"comment":"已复现"}));
            assert!(verify_confirmation_result(action, &old, &remote).is_err());
            remote["confirmed"] = json!("1");
            let confirmed = normalized(&remote, "c", None).unwrap();
            assert_eq!(confirmed["confirmed"], true);
            assert!(transition_payload("confirm", &json!({}), &confirmed).is_err());
            assert!(verify_baseline(&old, &remote, "me").is_err());
            verify_confirmation_result(action, &confirmed, &remote).unwrap();
        }
        remote["confirmed"] = json!("unexpected");
        assert!(normalized(&remote, "c", None).is_err());
    }
    #[tokio::test]
    async fn confirm_assign_payloads_and_failed_writes_are_not_replayed() {
        let mut remote = bug(1, "me");
        remote["confirmed"] = json!(0);
        let old = normalized(&remote, "c", None).unwrap();
        assert!(transition_payload("assign", &json!({}), &old).is_err());
        for (action, input, expected) in [
            (
                "confirm",
                json!({"comment":"确认备注"}),
                json!({"comment":"确认备注"}),
            ),
            (
                "assign",
                json!({"assignedTo":"other","comment":"指派备注"}),
                json!({"assignedTo":"other","comment":"指派备注"}),
            ),
        ] {
            let (path, payload, _) = transition_payload(action, &input, &old).unwrap();
            let (base, server) = auth_server(vec![(200, remote.clone())]);
            adapter(&base)
                .bug_request("1", Some((path, &payload)))
                .await
                .unwrap();
            let requests = server.join().unwrap();
            assert!(requests[0].starts_with(&format!("POST /api.php/v1/bugs/1/{action} ")));
            let body = requests[0].split("\r\n\r\n").nth(1).unwrap();
            assert_eq!(serde_json::from_str::<Value>(body).unwrap(), expected);
            for status in [400, 403, 500] {
                let (base, server) = auth_server(vec![(status, json!({"error":"失败"}))]);
                assert!(adapter(&base)
                    .bug_request("1", Some((path, &payload)))
                    .await
                    .is_err());
                assert_eq!(server.join().unwrap().len(), 1);
            }
        }
    }
    #[test]
    fn baseline_checks_owner_version_and_fields_and_payloads_require_business_fields() {
        assert_eq!(account_of(&Value::Null).unwrap(), "");
        let remote = bug(1, "me");
        let old = normalized(&remote, "c", None).unwrap();
        verify_baseline(&old, &remote, "me").unwrap();
        assert!(verify_baseline(&old, &remote, "other").is_err());
        let mut changed = remote.clone();
        changed["steps"] = json!("远端修改");
        assert!(verify_baseline(&old, &changed, "me").is_err());
        assert!(transition_payload("resolve", &json!({"resolution":"fixed"}), &old).is_err());
        assert!(transition_payload("resolve", &json!({"resolution":"duplicate"}), &old).is_err());
        let (_, payload, status) = transition_payload(
            "resolve",
            &json!({"resolution":"fixed","resolvedBuild":"trunk"}),
            &old,
        )
        .unwrap();
        assert_eq!(status, "resolved");
        assert_eq!(payload["assignedTo"], "me");
        assert!(transition_payload("close", &json!({}), &old).is_err());
        let mut resolved = old.clone();
        resolved["status"] = json!("resolved");
        assert!(transition_payload("activate", &json!({"assignedTo":"me"}), &resolved).is_err());
        assert_eq!(
            transition_payload(
                "activate",
                &json!({"assignedTo":"me","openedBuild":["trunk"]}),
                &resolved
            )
            .unwrap()
            .0,
            "active"
        );
        assert!(
            edit_payload(&json!({"title":"BUG","severity":9,"priority":1,"assignedTo":"me"}))
                .is_err()
        );
    }
    #[test]
    fn unchanged_bug_sync_preserves_open_dialog_revision_and_timestamps() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("source")).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"公司"}))
            .unwrap();
        let remote = bug(1, "me");
        let opened = store
            .save_bug_snapshot(vec![normalized(&remote, "c", None).unwrap()], &connection)
            .unwrap()
            .remove(0);
        let history_count =
            || {
                store.conn.lock().unwrap().query_row(
                "SELECT COUNT(*) FROM entity_history WHERE entity_type='bugs' AND entity_id=?1",
                [text(&opened, "id")],
                |row| row.get::<_, i64>(0),
            ).unwrap()
            };
        let original_history_count = history_count();
        for _ in 0..3 {
            let snapshot = store.snapshot().unwrap();
            let connection = get(&snapshot, "connections", "c").unwrap();
            let cached = get(&snapshot, "bugs", text(&opened, "id")).unwrap();
            let synced = store
                .save_bug_snapshot(
                    vec![normalized(&remote, "c", Some(&cached)).unwrap()],
                    &connection,
                )
                .unwrap()
                .remove(0);
            for field in ["revision", "createdAt", "updatedAt"] {
                assert_eq!(synced[field], opened[field], "无变化同步不应修改 {field}");
            }
            let latest = get(&store.snapshot().unwrap(), "connections", "c").unwrap();
            assert_eq!(
                latest["revision"].as_i64(),
                Some(connection["revision"].as_i64().unwrap() + 1),
                "无变化同步仍须发布连接同步状态"
            );
            assert!(latest["lastBugSync"].as_i64().unwrap() > 0);
            assert!(
                latest["lastBugSync"].as_i64().unwrap()
                    >= connection["lastBugSync"].as_i64().unwrap()
            );
            assert_eq!(
                history_count(),
                original_history_count,
                "无变化同步不应新增 BUG 历史"
            );
        }
        // 弹窗仍持有打开时的版本；无变化同步后应能正常提交修改。
        let mut assigned = opened.clone();
        assigned["assignedTo"] = json!("other");
        let saved = store
            .save_batch(vec![("bug", assigned)], vec![], true)
            .unwrap()
            .remove(0);
        assert_eq!(
            saved["revision"].as_i64(),
            Some(opened["revision"].as_i64().unwrap() + 1)
        );
        assert_eq!(saved["assignedTo"], "other");
    }

    #[test]
    fn changed_bug_sync_keeps_revision_conflicts_and_atomic_scope_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("source")).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"公司"}))
            .unwrap();
        let mut remote = bug(1, "me");
        let mut cached = store
            .save_bug_snapshot(
                vec![
                    normalized(&remote, "c", None).unwrap(),
                    normalized(&bug(2, "me"), "c", None).unwrap(),
                ],
                &connection,
            )
            .unwrap()
            .remove(0);
        for (field, value) in [
            ("title", json!("修改标题")),
            ("steps", json!("<p>修改复现步骤</p>")),
            ("status", json!("resolved")),
            (
                "assignedTo",
                json!({"account":"other","realname":"其他用户"}),
            ),
            ("lastEditedDate", json!("2026-09-09T12:00:00+08:00")),
        ] {
            let previous = cached.clone();
            remote[field] = value;
            let snapshot = store.snapshot().unwrap();
            let connection = get(&snapshot, "connections", "c").unwrap();
            let second = get(&snapshot, "bugs", "zentao-bug-c-2").unwrap();
            cached = store
                .save_bug_snapshot(
                    vec![normalized(&remote, "c", Some(&cached)).unwrap(), second],
                    &connection,
                )
                .unwrap()
                .remove(0);
            assert_eq!(
                cached["revision"].as_i64(),
                Some(previous["revision"].as_i64().unwrap() + 1),
                "业务变化 {field} 必须使旧版本过期"
            );
            assert!(store
                .save_batch(vec![("bug", previous)], vec![], true)
                .is_err());
        }
        let snapshot = store.snapshot().unwrap();
        let connection = get(&snapshot, "connections", "c").unwrap();
        let mut stale_identical = normalized(&remote, "c", Some(&cached)).unwrap();
        stale_identical["revision"] = json!(cached["revision"].as_i64().unwrap() - 1);
        assert!(store
            .save_bug_snapshot(vec![stale_identical], &connection)
            .is_err());
        assert_eq!(
            store.snapshot().unwrap(),
            snapshot,
            "旧版本失败不能清理其他 BUG 或更新连接"
        );
        let mut wrong_scope = cached;
        wrong_scope["connectionId"] = json!("other");
        assert!(store
            .save_bug_snapshot(vec![wrong_scope], &connection)
            .is_err());
        assert_eq!(
            store.snapshot().unwrap(),
            snapshot,
            "跨连接输入不能部分发布"
        );
    }

    #[test]
    fn transferred_cache_removal_checks_revisions_and_preserves_other_bugs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("source")).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"公司"}))
            .unwrap();
        let values = store
            .save_batch(
                vec![
                    ("bug", normalized(&bug(1, "me"), "c", None).unwrap()),
                    ("bug", normalized(&bug(2, "me"), "c", None).unwrap()),
                ],
                vec![],
                true,
            )
            .unwrap();
        let mut stale = values[0].clone();
        stale["revision"] = json!(0);
        assert!(store.remove_transferred_bug(&stale, &connection).is_err());
        assert_eq!(
            store.snapshot().unwrap()["bugs"].as_array().unwrap().len(),
            2
        );
        store
            .remove_transferred_bug(&values[0], &connection)
            .unwrap();
        let remaining = store.snapshot().unwrap();
        assert_eq!(remaining["bugs"].as_array().unwrap().len(), 1);
        assert_eq!(remaining["bugs"][0]["remoteId"], "2");
        assert!(store
            .remove_transferred_bug(&values[0], &connection)
            .is_err());
    }
    #[test]
    fn schema_three_upgrade_preserves_existing_data_and_accepts_old_backup() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let store = crate::storage::Store::open_at(source.clone()).unwrap();
        let project = store.save("project", json!({"name":"原有项目"})).unwrap();
        let db = store.generation.join("workbench.sqlite3");
        let workspace = store.workspace_id.clone();
        store.conn.lock().unwrap().execute_batch("DROP TABLE meeting_segments; DROP TABLE meeting_audio_chunks; DROP TABLE meetings; DROP TABLE prompts; DROP TABLE provider_credentials; DROP TABLE bugs; DROP TABLE local_bugs; DROP TABLE task_reminder_deliveries; DELETE FROM settings WHERE key IN ('credential-encryption-check','credential-encryption-cleanup-pending'); DELETE FROM schema_migrations WHERE version>=4; PRAGMA user_version=3; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(store);
        let bytes = std::fs::read(db).unwrap();
        let archive = tmp.path().join("v3.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("workbench.sqlite3", options).unwrap();
        zip.write_all(&bytes).unwrap();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(json!({"formatVersion":1,"schemaVersion":3,"workspaceId":workspace,"files":[{"path":"workbench.sqlite3","size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}]}).to_string().as_bytes()).unwrap();
        zip.finish().unwrap();
        let restored =
            crate::storage::Store::restore_to(&archive, &tmp.path().join("restore")).unwrap();
        assert_eq!(restored.snapshot().unwrap()["projects"][0], project);
        assert_eq!(restored.snapshot().unwrap()["bugs"], json!([]));
        let reopened = crate::storage::Store::open_at(source).unwrap();
        assert_eq!(reopened.snapshot().unwrap()["projects"][0], project);
        assert_eq!(reopened.snapshot().unwrap()["bugs"], json!([]));
    }
    #[test]
    fn snapshot_pruning_is_atomic_isolated_revisioned_and_survives_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("source")).unwrap();
        let c = store
            .save("connection", json!({"id":"c","name":"公司"}))
            .unwrap();
        store
            .save("connection", json!({"id":"other","name":"其他"}))
            .unwrap();
        let a = normalized(&bug(1, "me"), "c", None).unwrap();
        let b = normalized(&bug(2, "me"), "other", None).unwrap();
        store.save_batch(vec![("bug", b)], vec![], true).unwrap();
        let saved = store.save_bug_snapshot(vec![a], &c).unwrap();
        let snapshot = store.snapshot().unwrap();
        let c = get(&snapshot, "connections", "c").unwrap();
        let mut bad = saved[0].clone();
        bad["status"] = json!("invalid");
        assert!(store.save_bug_snapshot(vec![bad], &c).is_err());
        assert_eq!(
            store.snapshot().unwrap()["bugs"].as_array().unwrap().len(),
            2
        );
        let backup = tmp.path().join("bugs.zip");
        store.backup(&backup).unwrap();
        let restored =
            crate::storage::Store::restore_to(&backup, &tmp.path().join("restore")).unwrap();
        assert_eq!(
            restored.snapshot().unwrap()["bugs"],
            store.snapshot().unwrap()["bugs"]
        );
        store.save_bug_snapshot(vec![], &c).unwrap();
        let remaining = store.snapshot().unwrap();
        assert_eq!(remaining["bugs"].as_array().unwrap().len(), 1);
        assert_eq!(remaining["bugs"][0]["connectionId"], "other");
        assert!(store.save_bug_snapshot(vec![], &c).is_err());
    }
}
