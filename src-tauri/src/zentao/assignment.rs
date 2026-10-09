//! 禅道执行负责人变更来源，只读操作历史，不把创建人推断成指派人。
use super::*;

fn label(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn assignment(remote: &Value, id: &str, project: &str, account: &str) -> Result<Value, String> {
    verify_execution_scope(remote, project, account)?;
    if remote_id(&remote["id"]).ok().as_deref() != Some(id) {
        return Err("禅道执行编号不一致，请重新同步核对".into());
    }
    let actions = remote["actions"]
        .as_array()
        .ok_or("禅道响应缺少有效操作历史，无法读取指派信息")?;
    let mut output = json!({"assignedBy":null,"assignedAt":null,"createdBy":label(&remote["openedBy"]).or_else(|| label(&remote["openedBy"]["realname"])).or_else(|| label(&remote["openedBy"]["account"]))});
    let mut latest: Option<(u64, &Value, &Value)> = None;
    let mut seen = HashSet::new();
    for action in actions {
        let action_id = remote_id(&action["id"])
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or("禅道操作历史编号无效")?;
        if !action.is_object() || !seen.insert(action_id) {
            return Err("禅道操作历史重复或格式无效".into());
        }
        if action["objectType"] != "execution"
            || remote_id(&action["objectID"]).ok().as_deref() != Some(id)
        {
            return Err("禅道操作历史不属于当前执行".into());
        }
        let histories = action["history"]
            .as_array()
            .ok_or("禅道操作历史缺少有效变更记录")?;
        let mut pm = None;
        for history in histories {
            let field = history["field"].as_str().ok_or("禅道变更记录字段无效")?;
            if field.eq_ignore_ascii_case("PM") && pm.replace(history).is_some() {
                return Err("禅道操作历史包含重复负责人变更".into());
            }
        }
        if let Some(history) = pm {
            if latest
                .as_ref()
                .is_none_or(|(previous, _, _)| action_id > *previous)
            {
                latest = Some((action_id, action, history));
            }
        }
    }
    if let Some((_, action, history)) = latest {
        // 22.0 的 old/new 可能已经被转换为姓名；身份授权只使用详情 PM.account。
        // 同名账号的 old/new 文本可能相同，不能据此跳过真实变更。
        let (owner_account, owner_name) = owner_identity(&remote["PM"]);
        let target = label(&history["new"]);
        if target
            .as_deref()
            .is_some_and(|target| target == owner_account || target == owner_name)
            && history["old"].is_string()
        {
            if let Some(actor) = label(&action["actor"]) {
                output["assignedBy"] = json!(actor);
                output["assignedAt"] = label(&action["date"])
                    .filter(|date| {
                        chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S").is_ok()
                    })
                    .map(Value::String)
                    .unwrap_or(Value::Null);
            }
        }
    }
    Ok(output)
}

#[tauri::command]
pub async fn zentao_task_assignment(
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<Value, String> {
    let ctx = super::execution_context::context(&state, &task_id)?;
    let adapter = super::execution_context::adapter(&state, &ctx)?;
    let account = adapter.account().await?;
    let id = remote_id(&ctx.task["remoteId"])?;
    let remote = adapter.execution_assignment(&id).await?;
    let output = assignment(&remote, &id, &ctx.project, &account)?;
    super::execution_context::unchanged(&state, &ctx)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote(actions: Value) -> Value {
        json!({"id":42,"project":7,"PM":{"account":"me","realname":"本人"},"openedBy":{"account":"creator","realname":"创建者"},"actions":actions})
    }
    fn action(id: u64, old: &str, new: &str, actor: &str) -> Value {
        json!({"id":id,"objectType":"execution","objectID":42,"actor":actor,"date":"2026-10-08 12:00:00","history":[{"field":"PM","old":old,"new":new}]})
    }
    #[test]
    fn latest_assignment_is_sorted_and_never_falls_back() {
        let mut data = remote(json!([
            action(20, "别人", "本人", "指派人乙"),
            action(10, "", "me", "指派人甲")
        ]));
        assert_eq!(
            assignment(&data, "42", "7", "me").unwrap()["assignedBy"],
            "指派人乙"
        );
        data["actions"][0]["history"][0]["new"] = json!("别的人");
        assert!(assignment(&data, "42", "7", "me").unwrap()["assignedBy"].is_null());
    }
    #[test]
    fn creator_is_not_assignment_and_same_names_are_not_skipped() {
        let data = remote(json!([]));
        let info = assignment(&data, "42", "7", "me").unwrap();
        assert_eq!(info["createdBy"], "创建者");
        assert!(info["assignedBy"].is_null());
        let data = remote(json!([action(3, "本人", "本人", "管理员")]));
        assert_eq!(
            assignment(&data, "42", "7", "me").unwrap()["assignedBy"],
            "管理员"
        );
    }
    #[test]
    fn malformed_and_foreign_history_is_rejected() {
        for actions in [
            Value::Null,
            json!({}),
            json!([{}]),
            json!([action(1, "", "me", "甲"), action(1, "", "me", "乙")]),
        ] {
            assert!(assignment(&remote(actions), "42", "7", "me").is_err());
        }
        let mut data = remote(json!([action(1, "", "me", "甲")]));
        data["actions"][0]["objectID"] = json!(99);
        assert!(assignment(&data, "42", "7", "me").is_err());
        assert!(assignment(&remote(json!([])), "42", "8", "me").is_err());
        assert!(assignment(&remote(json!([])), "42", "7", "other").is_err());
    }
    #[tokio::test]
    async fn reads_assignment_with_actions_query_and_verifies_scope() {
        let (url, server) =
            super::super::tests::server(vec![remote(json!([action(1, "", "me", "甲")]))]);
        let adapter = Adapter::personal(
            &json!({"baseUrl":url,"apiVersion":"v1"}),
            "fixture-token".into(),
        )
        .unwrap();
        let data = adapter.execution_assignment("42").await.unwrap();
        assert_eq!(
            assignment(&data, "42", "7", "me").unwrap()["assignedBy"],
            "甲"
        );
        assert!(assignment(&data, "42", "7", "other").is_err());
        assert!(assignment(&data, "42", "8", "me").is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /api.php/v1/executions/42?fields=actions "));
        assert!(!requests[0].contains("/tasks"));
    }
}
