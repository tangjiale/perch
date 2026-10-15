//! BUG 操作历史按需只读加载；创建人与真实指派来源独立展示。
use super::*;

fn person(value: &Value) -> Option<String> {
    [
        value.as_str(),
        value["realname"].as_str(),
        value["account"].as_str(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|s| !s.is_empty())
    .map(str::to_owned)
}
fn scalar(value: &Value) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok(String::new()),
        Value::Object(_) => person(value).ok_or("禅道历史字段格式无效".into()),
        _ => Err("禅道历史字段格式无效".into()),
    }
}
fn date(value: &Value, offset: Option<chrono::FixedOffset>) -> Option<String> {
    let raw = value.as_str()?;
    let when = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| {
            let when = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
            Some(offset.map_or_else(
                || when.naive_local(),
                |offset| when.with_timezone(&offset).naive_local(),
            ))
        })?;
    Some(when.format("%Y-%m-%d %H:%M:%S").to_string())
}
fn history(remote: &Value, id: &str) -> Result<Value, String> {
    if remote_id(&remote["id"]).ok().as_deref() != Some(id)
        || remote["deleted"] == true
        || remote["deleted"] == "1"
    {
        return Err("禅道 BUG 编号不一致或已删除，请同步核对".into());
    }
    let actions = remote["actions"]
        .as_array()
        .ok_or("禅道响应缺少有效 BUG 操作历史")?;
    // BUG assignedDate 经官方 time 格式器带时区；动作 date 保留服务器 SQL 时间。
    // 用远端显式时区统一 RFC3339 动作，SQL 时间始终按服务器业务时钟比较。
    let offset = remote["assignedDate"]
        .as_str()
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|when| *when.offset());
    let mut seen = HashSet::new();
    let mut entries = vec![];
    let mut latest = None;
    for action in actions {
        let aid = remote_id(&action["id"])?
            .parse::<u64>()
            .map_err(|_| "禅道历史编号无效")?;
        if aid == 0
            || !seen.insert(aid)
            || action["objectType"] != "bug"
            || remote_id(&action["objectID"]).ok().as_deref() != Some(id)
        {
            return Err("禅道历史重复或不属于当前 BUG".into());
        }
        let code = action["action"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("禅道历史动作无效")?;
        let histories = match action.get("history") {
            Some(Value::Array(rows)) => rows.as_slice(),
            Some(Value::Null) => &[],
            _ => return Err("禅道历史缺少有效字段变更记录".into()),
        };
        let mut changes = vec![];
        let mut target = None;
        for row in histories {
            let field = row["field"].as_str().ok_or("禅道历史变更字段无效")?;
            if row.get("old").is_none() || row.get("new").is_none() {
                return Err("禅道历史字段变更不完整".into());
            }
            if field.eq_ignore_ascii_case("assignedTo") && target.replace(&row["new"]).is_some() {
                return Err("禅道历史重复指派变更".into());
            }
            changes
                .push(json!({"field":field,"old":scalar(&row["old"])?,"new":scalar(&row["new"])?}));
        }
        // 官方 assigned 动作 extra 是目标人员显示名，history 也可能已转为姓名。
        if target.is_none() && code.eq_ignore_ascii_case("assigned") {
            target = Some(&action["extra"]);
        }
        let when = date(&action["date"], offset);
        let order = (when.clone().unwrap_or_default(), aid);
        if let Some(target) = target {
            if latest
                .as_ref()
                .is_none_or(|(previous, _, _)| order > *previous)
            {
                latest = Some((order.clone(), action, target));
            }
        }
        entries.push((order, json!({"id":aid.to_string(),"actor":person(&action["actor"]).unwrap_or_default(),"action":code,"date":when,"extra":scalar(&action["extra"])?,"comment":scalar(&action["comment"])? ,"changes":changes})));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut output = json!({"assignedBy":null,"assignedAt":null,"createdBy":person(&remote["openedBy"]),"entries":entries.into_iter().map(|(_,entry)|entry).collect::<Vec<_>>()});
    if let Some((_, action, target)) = latest {
        let target = person(target);
        let owner = &remote["assignedTo"];
        let matches = target.as_deref().is_some_and(|s| {
            owner.as_str() == Some(s)
                || owner["account"].as_str() == Some(s)
                || owner["realname"].as_str() == Some(s)
        });
        let when = date(&action["date"], offset);
        let consistent_date = date(&remote["assignedDate"], offset)
            .is_none_or(|assigned| when.as_deref() == Some(assigned.as_str()));
        if matches && consistent_date {
            output["assignedBy"] = json!(person(&action["actor"]));
            output["assignedAt"] = json!(when);
        }
    }
    Ok(output)
}
impl Adapter<'_> {
    async fn bug_history(&self, id: &str) -> Result<Value, String> {
        let mut url = endpoint(&self.base, &format!("bugs/{id}"))?;
        url.query_pairs_mut().append_pair("fields", "actions");
        let mut stream = checked_zentao(self.read(url).await?).await?.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "BUG 历史读取中断，请重试")?;
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err("BUG 历史响应过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "BUG 历史响应无效".into())
    }
}
#[tauri::command]
pub async fn zentao_bug_history(
    state: State<'_, AppState>,
    bug_id: String,
) -> Result<Value, String> {
    let (old, connection, workspace, generation) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let snapshot = store.snapshot()?;
        let old = get(&snapshot, "bugs", &bug_id)?;
        let connection = get(&snapshot, "connections", text(&old, "connectionId"))?;
        (
            old,
            connection,
            store.workspace_id.clone(),
            store.generation.clone(),
        )
    };
    if connection["enabled"] == false {
        return Err("该禅道连接已停用".into());
    }
    let unchanged = || -> Result<(), String> {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace || store.generation != generation {
            return Err("工作空间已切换，请重新打开 BUG".into());
        }
        let snapshot = store.snapshot()?;
        if get(&snapshot, "connections", text(&connection, "id"))? != connection
            || get(&snapshot, "bugs", &bug_id)? != old
        {
            return Err("BUG 或连接已变化，请重新打开详情".into());
        }
        Ok(())
    };
    // 只读历史不占同步写锁；用户可在加载期间操作 BUG，变化后的读取结果会被丢弃。
    let token = credential(&workspace, text(&connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter = Adapter::personal(&connection, token)?.with_auth(
        crate::zentao_auth::AutoAuth::new(&state, &workspace, &generation, &connection)?,
    );
    unchanged()?;
    let account = adapter.account().await?;
    unchanged()?;
    let id = remote_id(&old["remoteId"])?;
    let remote = adapter.bug_history(&id).await?;
    bugs::verify_baseline(&old, &remote, &account)?;
    let output = history(&remote, &id)?;
    unchanged()?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn action(id: u64, code: &str, target: &str) -> Value {
        json!({"id":id,"objectType":"bug","objectID":42,"actor":"指派者","action":code,"date":"2026-10-08 12:00:00","comment":"<p>备注</p>","extra":target,"history":[]})
    }
    fn remote(actions: Value) -> Value {
        json!({"id":42,"assignedTo":{"account":"me","realname":"本人"},"openedBy":{"account":"creator","realname":"创建者"},"actions":actions})
    }
    #[test]
    fn sorted_history_and_assignment_are_independent_of_creator() {
        let value = history(
            &remote(json!([
                action(20, "assigned", "本人"),
                action(3, "opened", "")
            ])),
            "42",
        )
        .unwrap();
        assert_eq!(value["entries"][0]["id"], "3");
        assert_eq!(value["assignedBy"], "指派者");
        assert_eq!(value["createdBy"], "创建者");
        assert!(
            history(&remote(json!([action(1, "opened", "")])), "42").unwrap()["assignedBy"]
                .is_null()
        );
        assert_eq!(value["entries"][1]["comment"], "<p>备注</p>");
    }
    #[test]
    fn field_changes_person_objects_and_latest_target_are_checked() {
        let mut a = action(1, "edited", "");
        a["actor"] = json!({"account":"boss","realname":"负责人"});
        a["history"] = json!([{"field":"assignedTo","old":"other","new":{"account":"me"}}]);
        let value = history(&remote(json!([a.clone()])), "42").unwrap();
        assert_eq!(value["assignedBy"], "负责人");
        assert_eq!(value["entries"][0]["changes"][0]["new"], "me");
        assert!(
            history(&remote(json!([a, action(2, "assigned", "别人")])), "42").unwrap()
                ["assignedBy"]
                .is_null()
        );
    }
    #[test]
    fn missing_malformed_and_foreign_history_are_rejected() {
        for actions in [
            Value::Null,
            json!({}),
            json!([{}]),
            json!([action(1, "assigned", "me"), action(1, "assigned", "me")]),
        ] {
            assert!(history(&remote(actions), "42").is_err());
        }
        let mut a = action(1, "assigned", "me");
        a["objectID"] = json!(99);
        assert!(history(&remote(json!([a])), "42").is_err());
        assert!(history(&remote(json!([])), "43").is_err());
        let mut a = action(1, "assigned", "me");
        a["history"] = Value::Null;
        assert!(history(&remote(json!([a])), "42").is_ok());
    }
    #[test]
    fn standard_dates_and_empty_realname_keep_assignment_evidence() {
        assert_eq!(
            person(&json!({"realname":"  ","account":"boss"})).as_deref(),
            Some("boss")
        );
        let mut data = remote(json!([action(1, "assigned", "me")]));
        data["assignedDate"] = json!("2026-10-08T12:00:00+08:00");
        data["actions"][0]["actor"] = json!({"realname":"","account":"boss"});
        let value = history(&data, "42").unwrap();
        assert_eq!(value["assignedBy"], "boss");
        assert_eq!(value["assignedAt"], "2026-10-08 12:00:00");
        data["actions"][0]["date"] = json!("2026-10-08T04:00:00Z");
        assert_eq!(history(&data, "42").unwrap()["assignedBy"], "boss");
        data["actions"][0]["date"] = json!("2026-10-08T04:00:01Z");
        assert!(history(&data, "42").unwrap()["assignedBy"].is_null());
    }
    #[test]
    fn cached_baseline_rejects_transferred_and_changed_remote_bug() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(temp.path().join("source")).unwrap();
        let connection = store
            .save("connection", json!({"id":"c","name":"测试"}))
            .unwrap();
        let cached = json!({"id":"zentao-bug-c-42","connectionId":"c","remoteId":"42","title":"测试 BUG","steps":"","status":"active","assignedTo":"me","severity":3,"priority":3,"remoteEditedDate":null,"confirmed":null});
        let cached = store
            .save_bug_snapshot(vec![cached], &connection)
            .unwrap()
            .remove(0);
        let mut data = remote(json!([]));
        data["title"] = json!("测试 BUG");
        data["status"] = json!("active");
        bugs::verify_baseline(&cached, &data, "me").unwrap();
        data["assignedTo"]["account"] = json!("other");
        assert!(bugs::verify_baseline(&cached, &data, "me").is_err());
        data["assignedTo"]["account"] = json!("me");
        data["lastEditedDate"] = json!("2026-10-08 12:00:01");
        assert!(bugs::verify_baseline(&cached, &data, "me").is_err());
    }
    #[tokio::test]
    async fn actual_request_uses_bug_actions_query() {
        let (url, server) =
            super::super::tests::server(vec![remote(json!([action(1, "assigned", "me")]))]);
        let adapter = Adapter::personal(
            &json!({"baseUrl":url,"apiVersion":"v1"}),
            "fixture-token".into(),
        )
        .unwrap();
        let value = adapter.bug_history("42").await.unwrap();
        assert_eq!(history(&value, "42").unwrap()["assignedBy"], "指派者");
        let requests = server.join().unwrap();
        assert!(
            requests[0].starts_with("GET /api.php/v1/bugs/42?fields=actions HTTP/1.1"),
            "{}",
            requests[0]
        );
    }
}
