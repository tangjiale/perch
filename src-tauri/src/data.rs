use serde_json::Value;

pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn validate(value: &Value, kind: &str) -> Result<(), String> {
    if !value.is_object() {
        return Err("数据必须是对象".into());
    }
    for key in ["apiKey", "token", "password", "secret", "accessToken"] {
        if value.get(key).is_some() {
            return Err("凭据必须通过系统钥匙串保存".into());
        }
    }
    let name_key = if ["tasks", "bugs"].contains(&kind) {
        "title"
    } else {
        "name"
    };
    if !["messages", "conversations"].contains(&kind) && text(value, name_key).trim().is_empty() {
        return Err("名称不能为空".into());
    }
    if kind == "tasks" {
        if let Some(timezone) = optional(value, "timezone") {
            timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| "无效的 IANA 时区")?;
        }
        let start = value.get("startAt").and_then(Value::as_i64);
        let end = value.get("endAt").and_then(Value::as_i64);
        if end.is_some() && (start.is_none() || end <= start) {
            return Err("结束时间必须晚于开始时间".into());
        }
        if start.is_some() && optional(value, "scheduleDate").is_some() {
            return Err("带时间排期与全天日期不能同时设置".into());
        }
        for key in ["scheduleDate"] {
            if let Some(date) = optional(value, key) {
                if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
                    return Err("日期格式必须为 YYYY-MM-DD".into());
                }
            }
        }
    }
    if kind == "apps" {
        let url = url::Url::parse(text(value, "url")).map_err(|_| "应用网址无效")?;
        if !["http", "https"].contains(&url.scheme()) || url.host_str().is_none() {
            return Err("应用网址必须是 HTTP 或 HTTPS 地址".into());
        }
    }
    Ok(())
}

/// 旧任务截止日合并至唯一排期；已有结束时间优先，绝不覆盖它。
pub fn migrate_task_end(value: &mut Value) {
    use chrono::TimeZone;
    let due = optional(value, "dueDate")
        .and_then(|v| chrono::NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok());
    if let Some(object) = value.as_object_mut() {
        object.remove("dueDate");
    }
    let Some(next) = due.and_then(|d| d.succ_opt()) else {
        return;
    };
    if optional(&value["schedule"], "end").is_some() {
        return;
    }
    let timezone = optional(&value["schedule"], "timezone")
        .or_else(|| optional(value, "timezone"))
        .unwrap_or_else(|| "Asia/Shanghai".into());
    if text(&value["schedule"], "kind") == "timed" {
        let start = chrono::DateTime::parse_from_rfc3339(text(&value["schedule"], "start")).ok();
        let end = timezone.parse::<chrono_tz::Tz>().ok().and_then(|tz| {
            tz.from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
                .earliest()
        });
        if let (Some(start), Some(end)) = (start, end) {
            if end > start {
                value["schedule"]["end"] = serde_json::json!(end.to_rfc3339());
            }
        }
    } else {
        let start = optional(&value["schedule"], "start")
            .and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
            .unwrap_or_else(|| due.unwrap());
        if next > start {
            value["schedule"] = serde_json::json!({"kind":"all_day","start":start.to_string(),"end":next.to_string(),"timezone":timezone});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_deadline_uses_timezone_and_never_reverses_dates() {
        let mut task = json!({"dueDate":"2026-09-08","schedule":{"kind":"timed","start":"2026-09-08T12:00:00+08:00","timezone":"Asia/Shanghai"}});
        migrate_task_end(&mut task);
        assert_eq!(task["schedule"]["end"], "2026-09-09T00:00:00+08:00");
        task["dueDate"] = json!("2026-09-01");
        task["schedule"].as_object_mut().unwrap().remove("end");
        migrate_task_end(&mut task);
        assert!(task["schedule"].get("end").is_none());
        assert!(task.get("dueDate").is_none());
    }
}
