//! 新建本地工作项与禅道执行的单次提交边界。
use super::*;
use crate::storage::Store;
use rusqlite::OptionalExtension;

pub(super) fn new_remote_project(snapshot: &Value, value: &Value) -> Result<Option<Value>, String> {
    if snapshot["tasks"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["id"] == value["id"]))
    {
        return Ok(None);
    }
    let project_id = text(value, "projectId");
    if project_id.is_empty() {
        return Ok(None);
    }
    let project = get(snapshot, "projects", project_id)?;
    Ok((project["source"] == "zentao").then_some(project))
}

fn payload(value: &Value, project: &str, account: &str) -> Result<Value, String> {
    if value["status"] != "todo" {
        return Err("新建禅道执行的初始状态必须是待做".into());
    }
    if text(value, "title").trim().is_empty() {
        return Err("请填写执行名称".into());
    }
    let id = uuid::Uuid::parse_str(text(value, "id"))
        .map_err(|_| "新建任务缺少有效标识，请重新打开新建窗口")?;
    let patch = execution_patch(&json!({}), value)?;
    if patch.get("begin").is_none() || patch.get("end").is_none() {
        return Err("禅道执行需要开始和结束日期".into());
    }
    Ok(
        json!({"project":project.parse::<u64>().map_err(|_| "禅道项目编号无效")?,
        "name":text(value,"title"),"desc":text(value,"notes"),"begin":patch["begin"],"end":patch["end"],
        "lifetime":"short","PM":account,"code":format!("perch-{}",id.simple()),
        "products":[],"plans":[],"teamMembers":[account]}),
    )
}

// 只展示业务字段校验，禁止直接回显 HTML、堆栈、认证字段或完整服务端响应。
fn validation_details(value: &Value) -> Option<String> {
    let fields = [
        ("name", "执行名称"),
        ("code", "执行代号"),
        ("project", "所属项目"),
        ("begin", "计划开始日期"),
        ("end", "计划结束日期"),
        ("days", "可用工作日"),
        ("products", "关联产品"),
        ("plans", "关联计划"),
        ("PM", "执行负责人"),
        ("lifetime", "执行周期"),
        ("acl", "访问控制"),
    ];
    let mut details = vec![];
    for (field, label) in fields {
        if let Some(object) = value.as_object() {
            for (key, message) in object {
                if key != field && !key.starts_with(&format!("{field}[")) {
                    continue;
                }
                let messages = message
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![message.clone()]);
                for message in messages.iter().take(3) {
                    if let Some(message) = message.as_str() {
                        let lower = message.to_lowercase();
                        if message.chars().count() > 300
                            || message.contains(['<', '>'])
                            || [
                                "token",
                                "password",
                                "secret",
                                "authorization",
                                "cookie",
                                "sql",
                                "stack trace",
                                "http://",
                                "https://",
                                "密码",
                                "令牌",
                                "密钥",
                            ]
                            .iter()
                            .any(|word| lower.contains(word))
                        {
                            continue;
                        }
                        let clean: String = message.chars().filter(|c| !c.is_control()).collect();
                        if !clean.trim().is_empty() {
                            details.push(format!("{label}：{}", clean.trim()));
                        }
                    }
                }
            }
        }
    }
    if !details.is_empty() {
        return Some(details.into_iter().take(8).collect::<Vec<_>>().join("；"));
    }
    None
}
fn response_validation(value: &Value) -> Option<String> {
    for candidate in [value, &value["message"], &value["errors"], &value["error"]] {
        if let Some(details) = validation_details(candidate) {
            return Some(details);
        }
        if let Some(raw) = candidate.as_str().filter(|raw| raw.len() <= 8192) {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                if let Some(details) = validation_details(&parsed) {
                    return Some(details);
                }
            }
        }
    }
    None
}
async fn body(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status().as_u16();
    // 400/422 保留字段错误；其他状态沿用认证/权限处理，绝不输出原始错误正文。
    let response = if matches!(status, 400 | 422) {
        response
    } else {
        checked_zentao(response).await?
    };
    let mut stream = response.bytes_stream();
    let mut bytes = vec![];
    let limit = if matches!(status, 400 | 422) {
        64 * 1024
    } else {
        4 * 1024 * 1024
    };
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "禅道响应读取失败，请同步核对创建结果")?;
        if bytes.len() + chunk.len() > limit {
            return Err("禅道响应过大，请同步核对创建结果".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let parsed = serde_json::from_slice::<Value>(&bytes);
    if matches!(status, 400 | 422) {
        let details = parsed
            .as_ref()
            .ok()
            .and_then(response_validation)
            .unwrap_or_else(|| {
                "服务器未返回可安全展示的字段校验信息，请在禅道查看创建规则或联系管理员核对该请求"
                    .into()
            });
        return Err(format!("禅道未接受创建请求（HTTP {status}）。{details}。这不是连接失效提示；请先在禅道核对是否已创建，确认未创建后重新打开新建窗口并调整表单。"));
    }
    let result = parsed.map_err(|_| "禅道响应无效，请同步核对创建结果")?;
    if result["status"] == "fail" || result["result"] == "fail" {
        let details = response_validation(&result)
            .unwrap_or_else(|| "服务器未提供可安全展示的字段信息".into());
        return Err(format!(
            "禅道创建被拒绝：{details}。请先在禅道核对结果，勿重复提交。"
        ));
    }
    Ok(result)
}

fn validate_project_dates(request: &Value, project: &Value) -> Result<(), String> {
    let date = |value: &Value, field: &str| {
        chrono::NaiveDate::parse_from_str(text(value, field), "%Y-%m-%d").ok()
    };
    if let (Some(begin), Some(project_begin)) = (date(request, "begin"), date(project, "begin")) {
        if begin < project_begin {
            return Err(format!(
                "计划开始日期不能早于禅道项目计划开始日期 {project_begin}，请调整任务日期。"
            ));
        }
    }
    if let (Some(end), Some(project_end)) = (date(request, "end"), date(project, "end")) {
        if end > project_end {
            return Err(format!(
                "计划结束日期不能晚于禅道项目计划结束日期 {project_end}，请调整任务日期。"
            ));
        }
    }
    Ok(())
}

impl Adapter<'_> {
    async fn create_execution(&self, payload: &Value) -> Result<Value, String> {
        // 22.0 先读取 JSON，再用 param('project', 0) 覆盖所属项目。
        // 项目编号必须同时传入查询参数，否则仅传 JSON 会被覆盖为 0。
        let project = remote_id(&payload["project"])
            .map_err(|_| "缺少有效的禅道项目编号，请重新同步项目后选择")?;
        let mut url = endpoint(&self.base, "executions")?;
        url.query_pairs_mut().append_pair("project", &project);
        // POST 不参与自动续登/重放；调用前的账号与项目 GET 已完成身份验证。
        body(
            self.client
                .post(url)
                .header("Token", self.token()?)
                .json(payload)
                .send()
                .await
                .map_err(|_| "禅道创建结果未知，请先同步核对，勿重新新建相同执行")?,
        )
        .await
    }
    async fn create_project(&self, id: &str) -> Result<Value, String> {
        let project = body(
            self.read(endpoint(&self.base, &format!("projects/{id}"))?)
                .await?,
        )
        .await?;
        if remote_id(&project["id"]).ok().as_deref() != Some(id) {
            return Err("无法核验禅道项目，请重新同步".into());
        }
        // v1 入口不接受 type，必须由项目模型派生；不能把看板/阶段冒充截图中的迭代。
        if !matches!(text(&project, "model"), "scrum" | "agileplus") {
            return Err("该禅道项目不支持默认迭代创建，请在禅道中创建对应执行后同步".into());
        }
        if project["status"] == "closed"
            || project["deleted"] == true
            || project["multiple"] == false
            || project["multiple"] == "0"
            || project["multiple"] == 0
        {
            return Err("该禅道项目已关闭或未启用多执行，不能创建执行".into());
        }
        Ok(project)
    }
}

// 使用现有 settings 表保存不含凭据的提交凭据，崩溃、超时或重启后仍阻止同一草稿重复 POST。
fn intent(store: &Store, key: &str) -> Result<Option<Value>, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(|e| e.to_string()))
        .transpose()
}
fn set_intent(store: &Store, key: &str, value: &Value) -> Result<(), String> {
    store.conn.lock().map_err(|e| e.to_string())?.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key,value.to_string()]).map_err(|e| e.to_string())?;
    Ok(())
}
fn apply_created(
    base: &str,
    value: &mut Value,
    project: &Value,
    result: &Value,
    expected: &Value,
    account: &str,
) -> Result<(), String> {
    let id = remote_id(&result["id"])?;
    verify_execution_scope(result, &remote_id(&project["remoteId"])?, account)?;
    if result["type"] != "sprint" || result["lifetime"] != "short" || result["status"] != "wait" {
        return Err("禅道创建结果与默认迭代、短期、未开始设置不一致，请同步核对".into());
    }
    let fields = json!({"name":expected["name"],"desc":expected["desc"],"begin":expected["begin"],"end":expected["end"]});
    let old = value.clone();
    apply_execution_result(base, value, &old, result, &fields)?;
    value["source"] = json!("zentao");
    value["remoteType"] = json!("execution");
    value["remoteId"] = json!(id);
    value["connectionId"] = project["connectionId"].clone();
    value["projectId"] = project["id"].clone();
    Ok(())
}

pub(super) async fn save_new(
    state: &State<'_, AppState>,
    mut value: Value,
    project: Value,
    expected_workspace: &str,
    expected_generation: &std::path::Path,
) -> Result<Value, String> {
    let (connection, workspace, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != expected_workspace || store.generation != expected_generation {
            return Err("数据目录已切换，未提交禅道创建请求，请刷新后重试".into());
        }
        store.validate_save("task", value.clone())?;
        (
            get(
                &store.snapshot()?,
                "connections",
                text(&project, "connectionId"),
            )?,
            store.workspace_id.clone(),
            store.generation.clone(),
        )
    };
    if connection["enabled"] == false {
        return Err("该禅道连接已停用，无法创建执行".into());
    }
    let connection_id = text(&connection, "id");
    let _guard = SyncGuard::acquire(&workspace, connection_id)?;
    let project_id = remote_id(&project["remoteId"])?;
    // 日期等表单校验在访问钥匙串和网络之前进行。
    payload(&value, &project_id, "validation")?;
    let key = credential(&workspace, connection_id)?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter = Adapter::personal(&connection, key)?.with_auth(
        crate::zentao_auth::AutoAuth::new(state, &workspace, &generation, &connection)?,
    );
    let account = adapter.account().await?;
    let remote_project = adapter.create_project(&project_id).await?;
    let request = payload(&value, &project_id, &account)?;
    validate_project_dates(&request, &remote_project)?;
    let intent_key = format!("zentao-create:{}:{}", connection_id, text(&value, "id"));
    let cached = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        validate_context(
            &store,
            &workspace,
            &generation,
            &connection,
            &project,
            &value,
        )?;
        if let Some(saved) = intent(&store, &intent_key)? {
            if saved["request"] != request {
                return Err("该草稿已提交过禅道创建请求，请先同步核对，不能修改后重复创建".into());
            }
            if saved["result"].is_null() {
                return Err("该草稿的禅道创建结果待核对，已阻止重复提交。请先同步并在禅道按名称查找；确认未创建后重新打开新建窗口".into());
            }
            Some(saved["result"].clone())
        } else {
            set_intent(
                &store,
                &intent_key,
                &json!({"request":request,"result":null}),
            )?;
            None
        }
    };
    let result = if let Some(result) = cached {
        result
    } else {
        let result = adapter.create_execution(&request).await?;
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace || store.generation != generation {
            return Err("数据目录已切换，禅道可能已创建执行，请切回原目录同步核对".into());
        }
        set_intent(
            &store,
            &intent_key,
            &json!({"request":request,"result":result}),
        )?;
        result
    };
    apply_created(
        text(&connection, "baseUrl"),
        &mut value,
        &project,
        &result,
        &request,
        &account,
    )?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    validate_context(
        &store,
        &workspace,
        &generation,
        &connection,
        &project,
        &value,
    )
    .map_err(|error| format!("禅道可能已创建执行，请同步核对：{error}"))?;
    // 远端成功、本地失败后可能已先同步导入；重试只返回现有工作项。
    if let Some(imported) = existing(
        &store.snapshot()?,
        "tasks",
        connection_id,
        "execution",
        text(&value, "remoteId"),
    ) {
        return Ok(imported.clone());
    }
    store
        .save("task", value)
        .map_err(|error| format!("禅道已创建执行，本地保存失败，请先同步核对：{error}"))
}
fn validate_context(
    store: &Store,
    workspace: &str,
    generation: &std::path::Path,
    connection: &Value,
    project: &Value,
    value: &Value,
) -> Result<(), String> {
    if store.workspace_id != workspace || store.generation != generation {
        return Err("数据目录已切换".into());
    }
    let snapshot = store.snapshot()?;
    if get(&snapshot, "connections", text(connection, "id"))?["revision"] != connection["revision"]
        || get(&snapshot, "projects", text(project, "id"))?["revision"] != project["revision"]
    {
        return Err("连接或项目已变化，请刷新后重试".into());
    }
    store.validate_save("task", value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> Value {
        json!({"id":uuid::Uuid::now_v7().to_string(),"title":"执行名称","notes":"执行说明","source":"local","status":"todo","projectId":"project","schedule":{"kind":"all_day","timezone":"Asia/Shanghai","start":"2026-09-08","end":"2026-09-10"}})
    }
    fn project() -> Value {
        json!({"id":"project","name":"项目","source":"zentao","connectionId":"c","remoteId":"12"})
    }
    fn created() -> Value {
        json!({"id":99,"project":12,"name":"执行名称","desc":"执行说明","type":"sprint","lifetime":"short","status":"wait","PM":{"account":"me","realname":"本人"},"begin":"2026-09-08","end":"2026-09-09"})
    }
    #[test]
    fn creation_errors_expose_fields_without_response_secrets() {
        let error = json!({"error":400,"message":{"name":["名称已经存在"],"end":"不能晚于项目结束日期","token":"private-value"}});
        let details = response_validation(&error).unwrap();
        assert!(details.contains("执行名称：名称已经存在"));
        assert!(details.contains("计划结束日期"));
        assert!(!details.contains("private-value"));
        assert!(response_validation(&json!({"message":"<html>private-value</html>"})).is_none());
        assert!(response_validation(
            &json!({"message":{"name":"SQL private-value","code":"token=private-value"}})
        )
        .is_none());
        assert!(
            response_validation(&json!({"message":"{\"begin\":[\"开始日期不合法\"]}"}))
                .unwrap()
                .contains("开始日期不合法")
        );
    }
    #[test]
    fn creation_dates_use_remote_project_inclusive_bounds() {
        let project = json!({"begin":"2026-07-01","end":"2026-09-30"});
        assert!(validate_project_dates(
            &json!({"begin":"2026-09-21","end":"2026-09-22"}),
            &project
        )
        .is_ok());
        assert!(validate_project_dates(
            &json!({"begin":"2026-06-30","end":"2026-09-22"}),
            &project
        )
        .unwrap_err()
        .contains("2026-07-01"));
        assert!(validate_project_dates(
            &json!({"begin":"2026-09-21","end":"2026-10-01"}),
            &project
        )
        .unwrap_err()
        .contains("2026-09-30"));
        assert!(
            validate_project_dates(&json!({"end":"2027-01-01"}), &json!({"end":"0000-00-00"}))
                .is_ok()
        );
    }
    #[tokio::test]
    async fn creation_http_400_surfaces_validation_and_does_not_retry() {
        let (url, server) = super::super::tests::auth_server(vec![(
            400,
            json!({"error":400,"message":{"name":["名称已经存在"]},"token":"private-value"}),
        )]);
        let adapter = Adapter::personal(&json!({"baseUrl":url}), "mock".into()).unwrap();
        let error = adapter
            .create_execution(&payload(&draft(), "12", "me").unwrap())
            .await
            .unwrap_err();
        assert!(error.contains("执行名称：名称已经存在"));
        assert!(!error.contains("private-value"));
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[test]
    fn creation_routes_only_new_remote_project_tasks() {
        let mut snapshot =
            json!({"tasks":[],"projects":[project(),{"id":"local","source":"local"}]});
        let mut value = draft();
        assert!(new_remote_project(&snapshot, &value).unwrap().is_some());
        snapshot["tasks"] = json!([value]);
        assert!(new_remote_project(&snapshot, &value).unwrap().is_none());
        snapshot["tasks"] = json!([]);
        value["projectId"] = json!("local");
        assert!(new_remote_project(&snapshot, &value).unwrap().is_none());
        value["projectId"] = Value::Null;
        assert!(new_remote_project(&snapshot, &value).unwrap().is_none());
    }
    #[test]
    fn defaults_use_token_owner_and_inclusive_end_with_validation() {
        let mut value = draft();
        let request = payload(&value, "12", "me").unwrap();
        assert_eq!(request["end"], "2026-09-09");
        assert_eq!(request["PM"], "me");
        assert_eq!(request["lifetime"], "short");
        assert_eq!(request["products"], json!([]));
        assert_eq!(request["plans"], json!([]));
        assert!(request.get("type").is_none());
        value["status"] = json!("doing");
        assert!(payload(&value, "12", "me").is_err());
        value["status"] = json!("todo");
        value["schedule"]["end"] = json!("2026-09-08");
        assert!(payload(&value, "12", "me").is_err());
        value["schedule"]["kind"] = json!("timed");
        assert!(payload(&value, "12", "me").is_err());
    }
    #[test]
    fn created_mapping_persists_and_next_sync_reuses_same_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        store.save("project", project()).unwrap();
        let mut value = draft();
        let expected = payload(&value, "12", "me").unwrap();
        apply_created(
            "https://example.test/zentao",
            &mut value,
            &project(),
            &created(),
            &expected,
            "me",
        )
        .unwrap();
        let saved = store.save("task", value).unwrap();
        assert_eq!(saved["remoteId"], "99");
        assert_eq!(saved["remoteExecutionOwnerAccount"], "me");
        assert_eq!(saved["schedule"]["end"], "2026-09-10");
        let snapshot = store.snapshot().unwrap();
        assert_eq!(
            existing(&snapshot, "tasks", "c", "execution", "99").unwrap()["id"],
            saved["id"]
        );
        let changes = merge(
            &snapshot,
            "c",
            RemoteSnapshot {
                projects: vec![json!({"id":12,"name":"项目"})],
                items: vec![("12".into(), "execution".into(), created())],
            },
        )
        .unwrap();
        store.save_batch(changes, vec![], true).unwrap();
        assert_eq!(
            store.snapshot().unwrap()["tasks"].as_array().unwrap().len(),
            1
        );
    }
    #[test]
    fn rejects_wrong_owner_project_or_returned_defaults() {
        let value = draft();
        let expected = payload(&value, "12", "me").unwrap();
        for (key, changed) in [
            ("PM", json!("other")),
            ("project", json!(13)),
            ("type", json!("kanban")),
            ("status", json!("doing")),
            ("end", json!("2026-09-10")),
        ] {
            let mut result = created();
            result[key] = changed;
            assert!(
                apply_created(
                    "https://example.test/zentao",
                    &mut value.clone(),
                    &project(),
                    &result,
                    &expected,
                    "me"
                )
                .is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn pending_and_completed_intent_survive_restart_without_task_publication() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        set_intent(
            &store,
            "zentao-create:c:id",
            &json!({"request":{},"result":null}),
        )
        .unwrap();
        drop(store);
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        assert!(intent(&store, "zentao-create:c:id").unwrap().unwrap()["result"].is_null());
        assert!(store.snapshot().unwrap()["tasks"]
            .as_array()
            .unwrap()
            .is_empty());
        set_intent(
            &store,
            "zentao-create:c:id",
            &json!({"request":{},"result":created()}),
        )
        .unwrap();
        drop(store);
        let store = Store::open_at(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            intent(&store, "zentao-create:c:id").unwrap().unwrap()["result"]["id"],
            99
        );
    }
    #[tokio::test]
    async fn failed_response_never_creates_a_local_task() {
        let (url, server) =
            super::super::tests::server(vec![json!({"status":"fail","message":"denied"})]);
        let adapter = Adapter::personal(&json!({"baseUrl":url}), "mock".into()).unwrap();
        let value = draft();
        let expected = payload(&value, "12", "me").unwrap();
        assert!(adapter
            .create_execution(&expected)
            .await
            .unwrap_err()
            .contains("创建被拒绝"));
        assert_eq!(value["source"], "local");
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn unauthorized_post_is_not_retried() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 8192];
            let count = stream.read(&mut buffer).unwrap();
            assert!(String::from_utf8_lossy(&buffer[..count])
                .starts_with("POST /api.php/v1/executions?project=12 "));
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let adapter = Adapter::personal(&json!({"baseUrl":url}), "mock".into()).unwrap();
        assert!(adapter
            .create_execution(&payload(&draft(), "12", "me").unwrap())
            .await
            .unwrap_err()
            .contains("401"));
        server.join().unwrap();
        assert!(!adapter.refreshed.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[tokio::test]
    async fn create_rejects_missing_project_before_network() {
        let adapter =
            Adapter::personal(&json!({"baseUrl":"http://127.0.0.1:1"}), "mock".into()).unwrap();
        for project in [Value::Null, json!(0), json!("local-uuid")] {
            let mut request = payload(&draft(), "12", "me").unwrap();
            request["project"] = project;
            assert!(adapter
                .create_execution(&request)
                .await
                .unwrap_err()
                .contains("编号"));
        }
    }
    #[tokio::test]
    async fn creates_using_v1_post_and_checks_project_model() {
        let (url, server) = super::super::tests::server(vec![
            json!({"id":12,"model":"scrum","status":"doing","multiple":"1"}),
            created(),
            json!({"id":12,"model":"kanban"}),
        ]);
        let adapter =
            Adapter::personal(&json!({"baseUrl":url,"apiVersion":"v2"}), "mock".into()).unwrap();
        adapter.create_project("12").await.unwrap();
        let result = adapter
            .create_execution(&payload(&draft(), "12", "me").unwrap())
            .await
            .unwrap();
        assert_eq!(result["id"], 99);
        assert!(adapter.create_project("12").await.is_err());
        let requests = server.join().unwrap();
        assert!(requests[1].starts_with("POST /api.php/v1/executions?project=12 "));
        let request_body: Value =
            serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(request_body["project"], 12);
        assert_eq!(request_body["PM"], "me");
        assert!(requests.iter().all(|request| !request.contains("/tasks")));
    }
}
