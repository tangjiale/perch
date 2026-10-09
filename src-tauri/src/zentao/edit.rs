//! 执行日期修改的依赖字段：只缩减超出新跨度的工作日，不重新估算工作日。
use super::*;

fn workdays(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
}

pub(super) fn workdays_match(actual: &Value, expected: &Value) -> bool {
    workdays(expected).is_some_and(|expected| workdays(actual) == Some(expected))
}

pub(super) fn complete_schedule_patch(patch: &mut Value, remote: &Value) -> Result<(), String> {
    if patch.get("begin").is_none() && patch.get("end").is_none() {
        return Ok(());
    }
    let begin = chrono::NaiveDate::parse_from_str(text(patch, "begin"), "%Y-%m-%d")
        .map_err(|_| "禅道执行需要有效的开始日期")?;
    let end = chrono::NaiveDate::parse_from_str(text(patch, "end"), "%Y-%m-%d")
        .map_err(|_| "禅道执行需要有效的结束日期")?;
    let span = (end - begin).num_days() + 1;
    if span <= 0 {
        return Err("结束日期不能早于开始日期".into());
    }
    if let Some(days) = remote.get("days").filter(|days| !days.is_null()) {
        let days = workdays(days).ok_or("禅道执行的可用工作日无效，请先在禅道核对并重新同步")?;
        // 22.0 PUT 会沿用旧 days；缩短日期时仅修正超过新包含末日跨度的旧值。
        if days > span as u64 {
            patch["days"] = json!(span);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_task() -> Value {
        json!({"id":"local-task","title":"执行","notes":"","status":"todo",
            "remoteStatus":"wait","remoteBegin":"2026-10-01","remoteEnd":"2026-10-16",
            "remoteDescription":"","schedule":{"kind":"all_day","start":"2026-10-01","end":"2026-10-17","timezone":"Asia/Shanghai"}})
    }

    fn edited_task() -> Value {
        let mut value = old_task();
        value["schedule"]["start"] = json!("2026-10-14");
        value
    }

    fn remote_execution() -> Value {
        json!({"id":224,"name":"执行","desc":"","status":"wait","project":12,
            "PM":{"account":"me"},"begin":"2026-10-01","end":"2026-10-16","days":"10"})
    }

    #[test]
    fn shortened_schedule_only_reduces_workdays_above_new_inclusive_span() {
        let expected = json!({"begin":"2026-10-14","end":"2026-10-16"});
        for days in [
            json!(10),
            json!("10"),
            json!(3),
            json!("2"),
            json!(0),
            Value::Null,
        ] {
            let mut patch = execution_patch(&old_task(), &edited_task()).unwrap();
            let mut remote = remote_execution();
            remote["days"] = days.clone();
            complete_schedule_patch(&mut patch, &remote).unwrap();
            if workdays(&days).is_some_and(|days| days > 3) {
                assert_eq!(
                    patch,
                    json!({"begin":"2026-10-14","end":"2026-10-16","days":3})
                );
            } else {
                assert_eq!(patch, expected);
            }
        }
        let mut patch = expected.clone();
        complete_schedule_patch(&mut patch, &json!({})).unwrap();
        assert_eq!(patch, expected);
        let mut patch = expected;
        assert!(complete_schedule_patch(&mut patch, &json!({"days":"unknown"})).is_err());
    }

    #[test]
    fn single_day_and_extended_schedule_preserve_valid_workdays() {
        let mut patch = json!({"begin":"2026-10-14","end":"2026-10-14"});
        complete_schedule_patch(&mut patch, &json!({"days":10})).unwrap();
        assert_eq!(patch["days"], 1);
        let mut patch = json!({"begin":"2026-10-01","end":"2026-10-30"});
        complete_schedule_patch(&mut patch, &json!({"days":10})).unwrap();
        assert!(patch.get("days").is_none());
        let mut patch = json!({"status":"doing"});
        complete_schedule_patch(&mut patch, &json!({"days":"unknown"})).unwrap();
        assert_eq!(patch, json!({"status":"doing"}));
    }

    #[test]
    fn date_edits_still_reject_remote_conflicts_and_changed_scope() {
        let old = old_task();
        let patch = execution_patch(&old, &edited_task()).unwrap();
        for (field, changed, reason) in [
            ("begin", json!("2026-10-02"), "开始日期已被修改"),
            ("end", json!("2026-10-17"), "结束日期已被修改"),
            ("PM", json!({"account":"other"}), "负责人"),
            ("project", json!(13), "所属项目"),
        ] {
            let mut remote = remote_execution();
            remote[field] = changed;
            assert!(verify_execution_edit(
                "https://example.test",
                &old,
                &remote,
                &patch,
                "12",
                "me"
            )
            .unwrap_err()
            .contains(reason));
        }
    }

    #[test]
    fn adjusted_workdays_are_checked_without_loosening_other_fields() {
        let old = old_task();
        let mut result = remote_execution();
        result["begin"] = json!("2026-10-14");
        result["days"] = json!("3");
        let patch = json!({"begin":"2026-10-14","end":"2026-10-16","days":3});
        let mut saved = edited_task();
        apply_execution_result("https://example.test", &mut saved, &old, &result, &patch).unwrap();
        assert_eq!(saved["schedule"]["start"], "2026-10-14");
        assert_eq!(saved["schedule"]["end"], "2026-10-17");
        for (field, wrong) in [
            ("days", json!(4)),
            ("days", Value::Null),
            ("begin", json!("2026-10-13")),
        ] {
            let mut returned = result.clone();
            returned[field] = wrong;
            let mut unchanged = edited_task();
            let before = unchanged.clone();
            assert!(apply_execution_result(
                "https://example.test",
                &mut unchanged,
                &old,
                &returned,
                &patch
            )
            .is_err());
            assert_eq!(unchanged, before);
        }
    }

    #[tokio::test]
    async fn date_edit_uses_current_remote_workdays_and_real_v1_put_body() {
        let mut updated = remote_execution();
        updated["begin"] = json!("2026-10-14");
        updated["days"] = json!("3");
        let (url, server) = super::super::tests::server(vec![remote_execution(), updated]);
        let adapter =
            Adapter::personal(&json!({"baseUrl":url,"apiVersion":"v2"}), "mock".into()).unwrap();
        let old = old_task();
        let mut value = edited_task();
        let mut patch = execution_patch(&old, &value).unwrap();
        let remote = adapter.execution_request("224", None).await.unwrap();
        verify_execution_edit(&url, &old, &remote, &patch, "12", "me").unwrap();
        complete_schedule_patch(&mut patch, &remote).unwrap();
        let result = adapter
            .execution_request("224", Some(&patch))
            .await
            .unwrap();
        verify_execution_scope(&result, "12", "me").unwrap();
        apply_execution_result(&url, &mut value, &old, &result, &patch).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api.php/v1/executions/224 "));
        assert!(requests[1].starts_with("PUT /api.php/v1/executions/224 "));
        let sent: Value =
            serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            sent,
            json!({"begin":"2026-10-14","end":"2026-10-16","days":3})
        );
        assert!(requests.iter().all(|request| !request.contains("/tasks")));
    }

    #[tokio::test]
    async fn rejected_dates_expose_safe_field_reason_without_replaying_put() {
        for status in [400, 422, 200] {
            let body = json!({"result":"fail","message":{"end":["结束日期不能晚于项目结束日期"],"days":"可用工作日不能超过3天","token":"private-value"}});
            let (url, server) = super::super::tests::auth_server(vec![(status, body)]);
            let adapter = Adapter::personal(&json!({"baseUrl":url}), "mock".into()).unwrap();
            let old = old_task();
            let patch = execution_patch(&old, &edited_task()).unwrap();
            let error = adapter
                .execution_request("224", Some(&patch))
                .await
                .unwrap_err();
            assert!(error.contains("计划结束日期：结束日期不能晚于项目结束日期"));
            assert!(error.contains("可用工作日"));
            assert!(error.contains("同步核对"));
            assert!(!error.contains("private-value"));
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].starts_with("PUT /api.php/v1/executions/224 "));
            assert!(!adapter.refreshed.load(std::sync::atomic::Ordering::SeqCst));
        }
    }
}
