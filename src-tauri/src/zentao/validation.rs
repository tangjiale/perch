//! 禅道创建和修改共用的安全字段校验响应解析。
use super::*;

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
                                "stacktrace",
                                "traceback",
                                "fatal error",
                                "exception",
                                ".php:",
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
pub(super) fn response_validation(value: &Value) -> Option<String> {
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
pub(super) async fn body(response: reqwest::Response, operation: &str) -> Result<Value, String> {
    let status = response.status().as_u16();
    let writing = matches!(operation, "创建" | "修改");
    let guidance = if writing {
        "请先同步核对结果，勿直接重复提交。"
    } else {
        "请检查禅道中的字段规则后重新读取。"
    };
    // 字段校验正文单独限流，其他状态仍沿用认证和权限处理。
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
        let chunk = chunk.map_err(|_| format!("禅道{operation}响应读取失败。{guidance}"))?;
        if bytes.len() + chunk.len() > limit {
            return Err(format!("禅道{operation}响应过大。{guidance}"));
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
                "服务器未返回可安全展示的字段校验信息，请在禅道查看字段规则或联系管理员核对该请求"
                    .into()
            });
        // 禅道可能先更新部分字段再返回校验错误，不能断言远端未改变。
        return Err(format!(
            "禅道{operation}请求返回字段校验错误（HTTP {status}）。{details}。{guidance}"
        ));
    }
    let result = parsed.map_err(|_| format!("禅道{operation}响应无效。{guidance}"))?;
    if result["status"] == "fail" || result["result"] == "fail" {
        let details = response_validation(&result)
            .unwrap_or_else(|| "服务器未提供可安全展示的字段信息".into());
        return Err(format!("禅道{operation}被拒绝：{details}。{guidance}"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_whitelisted_and_private_response_text_is_rejected() {
        let result = response_validation(&json!({"message": {
            "days": ["可用工作日不能超过三天"],
            "begin": "开始日期不能早于项目", "end": "结束日期不能晚于项目",
            "password": "private-value", "name": "<b>private-value</b>",
            "code": "Exception stack trace private-value"
        }}))
        .unwrap();
        assert!(result.contains("可用工作日：可用工作日不能超过三天"));
        assert!(result.contains("计划开始日期"));
        assert!(result.contains("计划结束日期"));
        assert!(!result.contains("private-value"));
        assert!(response_validation(&json!({"message":"raw private-value"})).is_none());
        assert!(
            response_validation(&json!({"message":"{\"end\":[\"结束日期不合法\"]}"}))
                .unwrap()
                .contains("结束日期不合法")
        );
    }

    #[tokio::test]
    async fn validation_statuses_keep_details_and_do_not_replay_writes() {
        for status in [400, 422] {
            let (url, server) = super::super::tests::auth_server(vec![(
                status,
                json!({"message": {"days": "不能超过排期天数", "token": "private-value"}}),
            )]);
            let response = reqwest::Client::new().put(url).send().await.unwrap();
            let error = body(response, "修改").await.unwrap_err();
            assert!(error.contains(&format!("HTTP {status}")));
            assert!(error.contains("可用工作日：不能超过排期天数"));
            assert!(error.contains("请先同步核对结果，勿直接重复提交"));
            assert!(!error.contains("private-value"));
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn response_limits_apply_to_errors_and_success() {
        for (status, size) in [(400, 64 * 1024), (200, 4 * 1024 * 1024)] {
            // 超限后客户端主动断开是预期行为，服务端不因 BrokenPipe 误判测试失败。
            use std::io::{Read, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                let content = json!({"message":"x".repeat(size)}).to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}", content.len());
                let _ = stream.write_all(response.as_bytes());
            });
            let response = reqwest::Client::new().put(url).send().await.unwrap();
            let error = body(response, "修改").await.unwrap_err();
            assert!(error.contains("响应过大"));
            assert!(error.contains("勿直接重复提交"));
            server.join().unwrap();
        }
    }
}
