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
            return Err("凭据必须通过专用接口加密保存".into());
        }
    }
    if kind == "apps"
        && ["account", "credentials"]
            .iter()
            .any(|key| value.get(*key).is_some())
    {
        return Err("应用账号和密码必须通过专用接口加密保存".into());
    }
    let name_key = if ["tasks", "bugs", "prompts"].contains(&kind) {
        "title"
    } else {
        "name"
    };
    if !["messages", "conversations"].contains(&kind) && text(value, name_key).trim().is_empty() {
        return Err("名称不能为空".into());
    }
    if kind == "prompts" {
        crate::prompt_optimizer::validate_entry(value)?;
    }
    if kind == "models" {
        if !matches!(
            text(value, "capability"),
            "chat" | "vision" | "embedding" | "speech"
        ) {
            return Err("模型类型必须为文本、视觉、向量或语音".into());
        }
        if let Some(mode) = value.get("speechMode").filter(|mode| !mode.is_null()) {
            if !matches!(mode.as_str(), Some("transcription" | "diarization")) {
                return Err("语音转录能力无效".into());
            }
        }
    }
    if kind == "tasks" {
        if let Some(status) = value.get("status") {
            if !matches!(
                status.as_str(),
                Some("todo" | "doing" | "suspended" | "done")
            ) {
                return Err("任务状态必须是待办、进行中、已挂起或已完成".into());
            }
        }
        validate_task_reminder(value)?;
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
        if let Some(date) = optional(value, "scheduleDate") {
            if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
                return Err("日期格式必须为 YYYY-MM-DD".into());
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

/// 提醒是本地偏好；开启前必须有合法结束时间，不能仅依赖前端校验。
pub fn validate_task_reminder(value: &Value) -> Result<(), String> {
    let Some(reminder) = value.get("reminder").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let enabled = reminder["enabled"]
        .as_bool()
        .ok_or("提醒启用状态必须为布尔值")?;
    let advance = reminder["advanceMinutes"]
        .as_u64()
        .ok_or("提醒提前时间无效")?;
    if ![0, 15, 60, 1440].contains(&advance) {
        return Err("提醒提前时间只能为到期时、15分钟、1小时或1天".into());
    }
    if !enabled {
        return Ok(());
    }
    let schedule = &value["schedule"];
    let timezone = text(schedule, "timezone")
        .parse::<chrono_tz::Tz>()
        .map_err(|_| "开启到期提醒需要有效的排期时区")?;
    let valid = match text(schedule, "kind") {
        "timed" => chrono::DateTime::parse_from_rfc3339(text(schedule, "start"))
            .ok()
            .zip(chrono::DateTime::parse_from_rfc3339(text(schedule, "end")).ok())
            .is_some_and(|(start, end)| end > start),
        "all_day" => chrono::NaiveDate::parse_from_str(text(schedule, "start"), "%Y-%m-%d")
            .ok()
            .zip(chrono::NaiveDate::parse_from_str(text(schedule, "end"), "%Y-%m-%d").ok())
            .is_some_and(|(start, end)| {
                use chrono::TimeZone;
                end > start
                    && end.and_hms_opt(0, 0, 0).is_some_and(|midnight| {
                        timezone.from_local_datetime(&midnight).earliest().is_some()
                    })
            }),
        _ => false,
    };
    if !valid {
        return Err("开启到期提醒需要有效的排期结束时间".into());
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
    fn speech_model_requires_a_known_transcription_mode() {
        let mut model = json!({"name":"会议语音","capability":"speech","speechMode":"diarization"});
        assert!(validate(&model, "models").is_ok());
        model["speechMode"] = json!("invented");
        assert!(validate(&model, "models").is_err());
        model["speechMode"] = Value::Null;
        assert!(validate(&model, "models").is_ok());
        model["capability"] = json!("unknown");
        assert!(validate(&model, "models").is_err());
    }

    #[test]
    fn task_reminder_validates_options_and_requires_valid_end() {
        let mut task = json!({"title":"任务","reminder":{"enabled":true,"advanceMinutes":15},"schedule":{"kind":"all_day","start":"2026-10-12","end":"2026-10-13","timezone":"Asia/Shanghai"}});
        for advance in [0, 15, 60, 1440] {
            task["reminder"]["advanceMinutes"] = json!(advance);
            assert!(validate(&task, "tasks").is_ok());
        }
        for invalid in [json!(1), json!(-15), json!("15"), json!(15.5)] {
            task["reminder"]["advanceMinutes"] = invalid;
            assert!(validate(&task, "tasks").is_err());
        }
        task["reminder"]["advanceMinutes"] = json!(15);
        task["schedule"]["end"] = Value::Null;
        assert!(validate(&task, "tasks").is_err());
        task["reminder"]["enabled"] = json!(false);
        assert!(validate(&task, "tasks").is_ok());
        task["reminder"]["enabled"] = json!("true");
        assert!(validate(&task, "tasks").is_err());
        task["reminder"] = Value::Null;
        assert!(validate(&task, "tasks").is_ok());
    }

    #[test]
    fn timed_reminder_rejects_missing_offset_and_reversed_end() {
        let mut task = json!({"reminder":{"enabled":true,"advanceMinutes":0},"schedule":{"kind":"timed","start":"2026-10-12T10:00:00+08:00","end":"2026-10-12T11:00:00+08:00","timezone":"Asia/Shanghai"}});
        assert!(validate_task_reminder(&task).is_ok());
        for timezone in [Value::Null, json!("invalid")] {
            task["schedule"]["timezone"] = timezone;
            assert!(validate_task_reminder(&task).is_err());
        }
        task["schedule"]["timezone"] = json!("Asia/Shanghai");
        for end in [
            "2026-10-12T09:00:00+08:00",
            "2026-10-12T11:00:00",
            "invalid",
        ] {
            task["schedule"]["end"] = json!(end);
            assert!(validate_task_reminder(&task).is_err());
        }
    }

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
