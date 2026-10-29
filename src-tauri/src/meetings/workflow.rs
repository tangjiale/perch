use super::*;
use serde_json::Map;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const MAX_ACTION_STATES: usize = 2000;

fn optional_text(value: &Value, key: &str, limit: usize, label: &str) -> Result<String, String> {
    let content = match value.get(key) {
        None => "",
        Some(value) => value.as_str().ok_or_else(|| format!("{label}必须是文本"))?,
    };
    bounded(content, limit, label, false)?;
    Ok(content.to_owned())
}

fn string_list(value: &Value, key: &str, limit: usize, label: &str) -> Result<Vec<String>, String> {
    let Some(items) = value.get(key) else {
        return Ok(Vec::new());
    };
    let items = items
        .as_array()
        .ok_or_else(|| format!("{label}必须是列表"))?;
    if items.len() > limit {
        return Err(format!("{label}最多 {limit} 项"));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for item in items {
        let item = item
            .as_str()
            .ok_or_else(|| format!("{label}必须是文本"))?
            .trim();
        bounded(item, 100, label, true)?;
        if seen.insert(item.to_owned()) {
            result.push(item.to_owned());
        }
    }
    Ok(result)
}

pub(super) fn normalize_context(value: Option<&Value>) -> Result<Value, String> {
    let empty = json!({});
    let value = value.unwrap_or(&empty);
    let object = value.as_object().ok_or("会议资料格式无效")?;
    if object.keys().any(|key| {
        ![
            "background",
            "participants",
            "notes",
            "glossary",
            "summaryStyle",
        ]
        .contains(&key.as_str())
    }) {
        return Err("会议资料包含不支持的字段".into());
    }
    let style = match value.get("summaryStyle") {
        None => "standard",
        Some(style) => style.as_str().ok_or("纪要详略设置无效")?,
    };
    if !["standard", "brief", "detailed"].contains(&style) {
        return Err("纪要详略只能选择标准、简洁或详细".into());
    }
    Ok(json!({
        "background": optional_text(value, "background", 5000, "会议背景")?,
        "participants": string_list(value, "participants", 100, "参会人")?,
        "notes": optional_text(value, "notes", 20_000, "会议笔记")?,
        "glossary": string_list(value, "glossary", 200, "会议热词")?,
        "summaryStyle": style,
    }))
}

pub(super) fn validate_analysis_context(value: &Value) -> Result<(), String> {
    if let Some(background) = value.get("background").filter(|v| !v.is_null()) {
        bounded(
            background.as_str().ok_or("纪要背景必须是文本")?,
            5000,
            "纪要背景",
            false,
        )?;
    }
    if value.get("participants").is_some_and(|v| !v.is_null()) {
        string_list(value, "participants", 100, "纪要参会人")?;
    }
    Ok(())
}

pub(super) fn analysis_content(value: Value) -> Result<Value, String> {
    let mut content = Map::new();
    for key in [
        "summary",
        "decisions",
        "mindMap",
        "background",
        "participants",
    ] {
        if let Some(value) = value.get(key).filter(|value| !value.is_null()) {
            content.insert(key.to_owned(), value.clone());
        }
    }
    let actions = value["actionItems"].as_array().ok_or("会议行动项无效")?;
    let mut sanitized = Vec::new();
    for action in actions {
        let mut clean = Map::new();
        for key in ["task", "owner", "deadline"] {
            if let Some(value) = action.get(key).filter(|v| !v.is_null()) {
                clean.insert(key.to_owned(), value.clone());
            }
        }
        sanitized.push(Value::Object(clean));
    }
    content.insert("actionItems".into(), json!(sanitized));
    Ok(Value::Object(content))
}

fn canonical(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 模型输出不拥有完成状态或任务关联。状态独立放在会议元数据，内容相同的重生成可复用。
fn decorate_actions(value: &mut Value) -> Result<(), String> {
    let meeting_id = text(value, "id").to_owned();
    let states = match value.get("actionStates") {
        None => Map::new(),
        Some(states) => states.as_object().ok_or("会议行动项状态损坏")?.clone(),
    };
    if states.len() > MAX_ACTION_STATES {
        return Err("会议行动项状态超过容量限制".into());
    }
    let Some(analysis) = value.get_mut("analysis").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let actions = analysis["actionItems"]
        .as_array_mut()
        .ok_or("会议行动项损坏")?;
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    for action in actions {
        let body = json!([
            canonical(text(action, "task")),
            canonical(text(action, "owner")),
            canonical(text(action, "deadline")),
        ])
        .to_string();
        let occurrence = occurrences.entry(body.clone()).or_default();
        let id = format!(
            "action-{:x}",
            Sha256::digest(
                json!([meeting_id, body, *occurrence])
                    .to_string()
                    .as_bytes()
            )
        );
        *occurrence += 1;
        let state = states.get(&id);
        let completed = match state.and_then(|v| v.get("completed")) {
            Some(value) => value.as_bool().ok_or("行动项完成状态损坏")?,
            None => false,
        };
        let task_id = state.and_then(|v| v.get("taskId")).filter(|v| !v.is_null());
        if let Some(task_id) = task_id {
            identifier(task_id.as_str().ok_or("行动项任务关联损坏")?)?;
        }
        let action = action.as_object_mut().ok_or("会议行动项损坏")?;
        action.insert("id".into(), json!(id));
        action.insert("completed".into(), json!(completed));
        action.remove("taskId");
        if let Some(task_id) = task_id {
            action.insert("taskId".into(), task_id.clone());
        }
    }
    Ok(())
}

fn validate_markers(value: &Value) -> Result<(), String> {
    let Some(markers) = value.get("markers") else {
        return Ok(());
    };
    let markers = markers.as_array().ok_or("会议重点标记格式无效")?;
    if markers.len() > 500 {
        return Err("每场会议最多 500 个重点标记".into());
    }
    let duration = value["durationMs"].as_i64().unwrap_or(0);
    let mut ids = HashSet::new();
    for marker in markers {
        let id = text(marker, "id");
        identifier(id)?;
        if !ids.insert(id) {
            return Err("会议重点标记标识重复".into());
        }
        let at = marker["atMs"].as_i64().ok_or("重点标记时间无效")?;
        if at < 0 || at > duration {
            return Err("重点标记时间超出已保存录音范围".into());
        }
        bounded(text(marker, "text"), 2000, "重点标记", true)?;
    }
    Ok(())
}

pub(super) fn normalize_meeting(value: &mut Value) -> Result<(), String> {
    value["context"] = normalize_context(value.get("context"))?;
    validate_markers(value)?;
    if value.get("markers").is_none() {
        value["markers"] = json!([]);
    }
    decorate_actions(value)
}

pub(super) fn glossary_prompt(value: &Value) -> String {
    value["context"]["glossary"]
        .as_array()
        .map(|terms| {
            terms
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default()
}

pub(super) fn generation_context(value: &Value) -> Result<String, String> {
    let context = normalize_context(value.get("context"))?;
    let markers = value.get("markers").cloned().unwrap_or_else(|| json!([]));
    Ok(format!(
        "会议标题：{}\n会议资料（背景、名单和热词仅供理解；notes 是用户人工补充）：\n{}\n用户重点标记（atMs 是已保存录音的毫秒位置，文字为用户补充，不等同于已确认决策）：\n{}\n以下为实际会议转录：\n",
        text(value, "title"), context, markers
    ))
}

pub(super) fn generation_input(value: &Value) -> Result<String, String> {
    let segments = value["segments"].as_array().ok_or("会议转录无效")?;
    if segments.is_empty() {
        return Err("请先录音并转录会议内容".into());
    }
    let mut input = generation_context(value)?;
    for segment in segments {
        let speaker = text(segment, "speaker");
        let name = value["speakerNames"][speaker].as_str().unwrap_or(speaker);
        input.push_str(&format!(
            "[{}-{}] {}：{}\n",
            segment["startMs"],
            segment["endMs"],
            if name.is_empty() { "未标注" } else { name },
            text(segment, "text")
        ));
    }
    if input.chars().count() > 100_000 {
        return Err("会议转录、资料和重点标记合计超过 10 万字，请先分场整理".into());
    }
    Ok(input)
}

fn action(value: &Value, action_id: &str) -> Result<Value, String> {
    identifier(action_id)?;
    if value["analysis"]["stale"] == true {
        return Err("会议纪要已过期，请重新生成并核对行动项后操作".into());
    }
    value["analysis"]["actionItems"]
        .as_array()
        .ok_or("请先生成会议纪要")?
        .iter()
        .find(|action| text(action, "id") == action_id)
        .cloned()
        .ok_or_else(|| "行动项已变化，请刷新会议后重试".into())
}

fn set_action_state(value: &mut Value, id: &str, state: Value) -> Result<(), String> {
    if value.get("actionStates").is_none() {
        value["actionStates"] = json!({});
    }
    let states = value["actionStates"]
        .as_object_mut()
        .ok_or("会议行动项状态损坏")?;
    if !states.contains_key(id) && states.len() >= MAX_ACTION_STATES {
        return Err("会议行动项状态已达容量限制，请新建会议整理后续事项".into());
    }
    states.insert(id.to_owned(), state);
    decorate_actions(value)
}

pub(super) fn complete_action(
    store: &Store,
    workspace_id: &str,
    id: &str,
    action_id: &str,
    completed: bool,
    revision: i64,
) -> Result<Value, String> {
    workspace(store, workspace_id)?;
    crate::meeting_realtime::ensure_idle(workspace_id, id)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(db)?;
    let mut value = read(&tx, id)?;
    if value["revision"].as_i64() != Some(revision) {
        return Err(CONFLICT.into());
    }
    let item = action(&value, action_id)?;
    if item.get("taskId").is_some() {
        return Err("该行动项已关联任务，请在我的任务中管理完成状态".into());
    }
    if item["completed"] == completed {
        return Ok(value);
    }
    set_action_state(&mut value, action_id, json!({"completed":completed}))?;
    write_metadata(&tx, &mut value, revision)?;
    let result = read(&tx, id)?;
    tx.commit().map_err(db)?;
    Ok(result)
}

fn html(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('\n', "<br>")
}

fn task_sort_order(conn: &Connection) -> Result<i64, String> {
    let highest: f64 = conn.query_row(
        "SELECT COALESCE(MAX(CAST(json_extract(data,'$.sortOrder') AS REAL)),0) FROM tasks WHERE status='todo'",
        [], |row| row.get(0),
    ).map_err(db)?;
    // 前端排序使用 JS number，超过安全整数范围会让相邻排序值再次重合。
    if !highest.is_finite() || highest > 9_007_199_254_739_967.0 {
        return Err("任务排序已达上限，请先整理任务看板后重试".into());
    }
    (highest.max(0.0).ceil() as i64)
        .checked_add(1024)
        .ok_or_else(|| "任务排序超过限制".into())
}

pub(super) fn action_task(
    store: &Store,
    workspace_id: &str,
    id: &str,
    action_id: &str,
    revision: i64,
) -> Result<Value, String> {
    workspace(store, workspace_id)?;
    crate::meeting_realtime::ensure_idle(workspace_id, id)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(db)?;
    let mut value = read(&tx, id)?;
    let item = action(&value, action_id)?;
    if let Some(task_id) = item["taskId"].as_str() {
        // 已落库的关联是重试凭据；旧 revision 的重复请求只读返回，不会再次创建。
        let raw: String = tx
            .query_row("SELECT data FROM tasks WHERE id=?1", [task_id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db)?
            .ok_or("关联任务已被删除，不能自动重新创建")?;
        let task: Value = serde_json::from_str(&raw).map_err(|_| "关联任务数据损坏")?;
        if text(&task, "source") != "local"
            || text(&task, "meetingId") != id
            || text(&task, "meetingActionId") != action_id
        {
            return Err("关联任务来源已变化，请到我的任务核对".into());
        }
        return Ok(json!({"meeting":value,"task":task}));
    }
    if value["revision"].as_i64() != Some(revision) {
        return Err(CONFLICT.into());
    }
    if item["completed"] == true {
        return Err("行动项已完成，请先取消完成标记再转为任务".into());
    }
    let task_id = uuid::Uuid::now_v7().to_string();
    let title: String = text(&item, "task").trim().chars().take(200).collect();
    let notes = format!(
        "<p>来自会议：{}</p><p>会议编号：{}</p><p>行动项：{}</p><p>负责人（会议原文）：{}</p><p>期限（会议原文，未自动排期）：{}</p>",
        html(text(&value,"title")), html(id), html(text(&item,"task")),
        html(if text(&item,"owner").is_empty() { "待确认" } else { text(&item,"owner") }),
        html(if text(&item,"deadline").is_empty() { "待确认" } else { text(&item,"deadline") }),
    );
    let task = Store::create_local_task_in_transaction(
        &tx,
        workspace_id,
        json!({
            "id":task_id,"title":title,"source":"local","status":"todo","priority":"normal",
            "notes":notes,"sortOrder":task_sort_order(&tx)?,"meetingId":id,"meetingActionId":action_id,
        }),
    )?;
    set_action_state(
        &mut value,
        action_id,
        json!({"completed":false,"taskId":task_id}),
    )?;
    write_metadata(&tx, &mut value, revision)?;
    let saved = read(&tx, id)?;
    tx.commit().map_err(db)?;
    Ok(json!({"meeting":saved,"task":task}))
}

pub(crate) fn add_marker(
    store: &Store,
    workspace_id: &str,
    id: &str,
    marker_id: &str,
    at_ms: i64,
    content: &str,
) -> Result<Value, String> {
    workspace(store, workspace_id)?;
    identifier(marker_id)?;
    bounded(content, 2000, "重点标记", true)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(db)?;
    let mut value = read(&tx, id)?;
    if at_ms < 0 || at_ms > value["durationMs"].as_i64().unwrap_or(0) {
        return Err("重点标记时间超出已保存录音范围".into());
    }
    let markers = value["markers"]
        .as_array_mut()
        .ok_or("会议重点标记格式无效")?;
    if let Some(previous) = markers
        .iter()
        .find(|marker| text(marker, "id") == marker_id)
    {
        if previous["atMs"] != at_ms || text(previous, "text") != content {
            return Err("同一个重点标记标识不能用于不同内容".into());
        }
        return Ok(value);
    }
    if markers.len() >= 500 {
        return Err("每场会议最多 500 个重点标记".into());
    }
    markers.push(json!({"id":marker_id,"atMs":at_ms,"text":content}));
    stale_analysis(&mut value);
    let revision = value["revision"].as_i64().ok_or("会议版本无效")?;
    write_metadata(&tx, &mut value, revision)?;
    let saved = read(&tx, id)?;
    tx.commit().map_err(db)?;
    Ok(saved)
}

#[cfg(test)]
mod tests;
