//! 禅道 22.0 API v1 BUG：产品全集分页后按令牌账号筛选；写入不自动重放。
use super::*;

fn account_of(value: &Value) -> Result<String, String> {
    // 22.0 的 user 格式转换对未指派、closed 等非用户账号返回 null。
    if value.is_null() {
        return Ok(String::new());
    }
    // 官方 user 格式化对未指派及 closed 哨兵返回 null；它们不属于当前账号。
    if value.is_null() {
        return Ok(String::new());
    }
    value
        .as_str()
        .or_else(|| value["account"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| "禅道 BUG 缺少有效指派账号，本次同步未发布".into())
}
fn reference(value: &Value) -> Option<String> {
    remote_id(value).or_else(|_| remote_id(&value["id"])).ok()
}
fn normalized(remote: &Value, connection: &str, old: Option<&Value>) -> Result<Value, String> {
    if remote.get("assignedTo").is_none() {
        return Err("禅道 BUG 缺少指派字段，请重新同步".into());
    }
    let id = remote_id(&remote["id"])?;
    let status = text(remote, "status");
    if !["active", "resolved", "closed"].contains(&status)
        || text(remote, "title").trim().is_empty()
    {
        return Err("禅道 BUG 返回了无效状态或名称".into());
    }
    let mut value = json!({"id":format!("zentao-bug-{connection}-{id}"),"connectionId":connection,"remoteId":id,"title":remote["title"],"steps":text(remote,"steps"),"status":status,"assignedTo":account_of(&remote["assignedTo"])?,"severity":remote["severity"].as_u64().or_else(||remote["severity"].as_str()?.parse().ok()).unwrap_or(3),"priority":remote["pri"].as_u64().or_else(||remote["pri"].as_str()?.parse().ok()).unwrap_or(3)});
    if let Some(old) = old {
        value["id"] = old["id"].clone();
        value["revision"] = old["revision"].clone();
    }
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

impl Adapter<'_> {
    async fn bug_request(
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
    let products = adapter.all("products", "products").await?;
    let mut output = vec![];
    let mut ids = HashSet::new();
    for product in products {
        let id = remote_id(&product["id"])?;
        for mut bug in adapter.all(&format!("products/{id}/bugs"), "bugs").await? {
            if bug.get("assignedTo").is_none() {
                return Err("禅道 BUG 列表缺少指派字段，本次同步未发布".into());
            }
            let bug_id = remote_id(&bug["id"])?;
            if !ids.insert(bug_id) {
                return Err("BUG 在多个产品重复出现，请重新同步".into());
            }
            if reference(&bug["product"]).as_deref() != Some(&id) {
                return Err("BUG 产品关联不一致，本次同步未发布".into());
            }
            if account_of(
                bug.get("assignedTo")
                    .ok_or("BUG 列表缺少指派字段，本次同步未发布")?,
            )? != account
                || bug["deleted"] == true
                || bug["deleted"] == "1"
            {
                continue;
            }
            // 列表可能省略 steps；详情提供编辑基线和产品名。
            let detail = adapter.bug_request(&remote_id(&bug["id"])?, None).await?;
            if account_of(&detail["assignedTo"])? != account
                || reference(&detail["product"]).as_deref() != Some(&id)
            {
                return Err("BUG 指派或产品在同步期间变化，请重新同步".into());
            }
            bug = detail;
            bug["productName"] = product["name"].clone();
            output.push(bug);
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
            normalized(
                bug,
                &connection_id,
                old.iter().find(|v| {
                    text(v, "connectionId") == connection_id && text(v, "remoteId") == id
                }),
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    let count = values.len();
    store.save_bug_snapshot(values, &connection)?;
    Ok(format!("同步完成，共 {count} 个指派给我的 BUG"))
}

fn verify_baseline(old: &Value, remote: &Value, account: &str) -> Result<(), String> {
    let current = normalized(remote, text(old, "connectionId"), Some(old))?;
    // 本人刚关闭的缓存允许在下次同步清理前重新激活；不能放行其他人关闭或转派的记录。
    let known_closed = text(old, "status") == "closed"
        && text(&current, "status") == "closed"
        && text(old, "assignedTo").is_empty()
        && text(&current, "assignedTo").is_empty()
        && text(old, "lastAssignedTo") == account;
    if text(&current, "assignedTo") != account && !known_closed {
        return Err("该 BUG 已不再指派给当前账号，请重新同步".into());
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
            return Err(format!("BUG 远端数据已变化（{key}），请同步后再修改"));
        }
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
    let (action, payload, status) = if let Some(edit) = &edit {
        for key in [
            "connectionId",
            "remoteId",
            "status",
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
    let mut value = normalized(&result, connection_id, Some(&old))?;
    if text(&value, "status") != status
        || value["productId"] != old["productId"]
        || value["projectId"] != old["projectId"]
    {
        return Err("禅道处理结果与预期不一致，请同步核对；未保存本地修改".into());
    }
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
    store
        .save_batch(
            vec![("bug", value)],
            vec![
                (
                    "connection",
                    connection_id.into(),
                    connection["revision"].as_i64().ok_or("连接版本无效")?,
                ),
                ("bug", id, revision),
            ],
            true,
        )
        .map_err(|e| format!("本地保存失败，禅道可能已更新，请重新同步：{e}"))?
        .pop()
        .ok_or("BUG 保存结果缺失".into())
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
        let remote = bug(1, "me");
        let old = normalized(&remote, "c", None).unwrap();
        let mut closed = remote.clone();
        closed["status"] = json!("closed");
        closed["assignedTo"] = Value::Null;
        assert!(verify_baseline(&old, &closed, "me").is_err());
        let cached_closed = normalized(&closed, "c", Some(&old)).unwrap();
        verify_baseline(&cached_closed, &closed, "me").unwrap();
        assert!(verify_baseline(&cached_closed, &closed, "other").is_err());
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
        assert!(requests[2].contains("page=2"));
        assert!(requests
            .iter()
            .all(|r| !r.contains("/tasks") && !r.contains("involved=")));
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
    fn schema_three_upgrade_preserves_existing_data_and_accepts_old_backup() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let store = crate::storage::Store::open_at(source.clone()).unwrap();
        let project = store.save("project", json!({"name":"原有项目"})).unwrap();
        let db = store.generation.join("workbench.sqlite3");
        let workspace = store.workspace_id.clone();
        store.conn.lock().unwrap().execute_batch("DROP TABLE bugs; DELETE FROM schema_migrations WHERE version=4; PRAGMA user_version=3; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
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
