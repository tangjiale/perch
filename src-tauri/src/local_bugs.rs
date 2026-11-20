use crate::{
    bug_steps::{HtmlSteps, StepsFormat},
    storage::Store,
    AppState,
};
use chrono::{NaiveDate, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalBugInput {
    id: uuid::Uuid,
    revision: Option<i64>,
    title: String,
    steps: String,
    steps_format: Option<StepsFormat>,
    bug_type: String,
    severity: u8,
    priority: u8,
    deadline: Option<String>,
    status: Option<String>,
}

pub(crate) fn list(conn: &Connection) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare("SELECT data FROM local_bugs ORDER BY rowid")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    })
    .collect()
}

#[tauri::command]
pub async fn local_bug_save(
    state: State<'_, AppState>,
    workspace_id: String,
    value: LocalBugInput,
) -> Result<Value, String> {
    let generation = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != workspace_id {
            return Err("工作空间已切换，请重新打开 BUG。".into());
        }
        store.generation.clone()
    };
    let value = if value.steps_format.is_some() {
        tauri::async_runtime::spawn_blocking(move || prepare_html(value))
            .await
            .map_err(|_| "无法完成复现步骤的图片检查。")??
    } else {
        value
    };
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace_id || store.generation != generation {
        return Err("工作空间已切换，请重新打开 BUG。".into());
    }
    save(&store, value)
}

fn prepare_html(mut value: LocalBugInput) -> Result<LocalBugInput, String> {
    value.steps = HtmlSteps::parse(&value.steps)?.local_html();
    Ok(value)
}

fn save(store: &Store, value: LocalBugInput) -> Result<Value, String> {
    let title = value.title.trim();
    if value.id.is_nil()
        || title.is_empty()
        || title.chars().count() > 255
        || title.chars().any(char::is_control)
    {
        return Err("标题长度须为 1 至 255 个字符。".into());
    }
    if !matches!(
        value.bug_type.as_str(),
        "codeerror"
            | "config"
            | "install"
            | "security"
            | "performance"
            | "standard"
            | "automation"
            | "designdefect"
            | "others"
    ) || !(1..=4).contains(&value.severity)
        || !(1..=4).contains(&value.priority)
    {
        return Err("BUG 类型、严重程度或优先级无效。".into());
    }
    let deadline = value.deadline.as_deref().filter(|s| !s.is_empty());
    if let Some(raw) = deadline {
        if raw.len() != 10
            || NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_or(true, |date| {
                date.format("%Y-%m-%d").to_string() != raw || raw.starts_with("0000")
            })
        {
            return Err("截止日期无效。".into());
        }
    }

    let id = format!("local-bug-{}", value.id);
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let old: Option<(i64, String)> = tx
        .query_row(
            "SELECT revision,data FROM local_bugs WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if old.as_ref().map(|row| row.0) != value.revision {
        return Err("此 BUG 已存在、发生变化或不再可用，请刷新列表。".into());
    }
    let old: Value = old
        .map(|(_, raw)| serde_json::from_str(&raw))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(Value::Null);
    let (steps, html) = match value.steps_format {
        Some(StepsFormat::Html) => (value.steps, true),
        None if old["steps"].as_str() == Some(value.steps.as_str()) => {
            (value.steps, old["stepsFormat"] == "html")
        }
        None => {
            if value.steps.chars().count() > 30_000 || value.steps.contains('\0') {
                return Err("复现步骤不能超过 30,000 个字符。".into());
            }
            (value.steps, false)
        }
    };
    let status = value.status.as_deref().unwrap_or("active");
    if !matches!(status, "active" | "resolved" | "closed")
        || (value.revision.is_none() && status != "active")
    {
        return Err("BUG 状态无效。".into());
    }
    let now = Utc::now();
    let date = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let resolved = match status {
        "resolved" => old["resolvedDate"].as_str().or(Some(date.as_str())),
        "closed" => old["resolvedDate"].as_str(),
        _ => None,
    };
    let revision = value.revision.unwrap_or(0) + 1;
    let mut result = json!({
        "id":id, "revision":revision, "source":"local",
        "title":title, "steps":steps, "bugType":value.bug_type,
        "severity":value.severity, "priority":value.priority, "status":status,
        "createdAt":old["createdAt"].as_i64().unwrap_or(now.timestamp_millis()),
        "updatedAt":now.timestamp_millis(),
        "openedDate":old["openedDate"].as_str().unwrap_or(&date),
        "resolvedDate":resolved, "deadline":deadline,
        "connectionId":"", "remoteId":"", "assignedTo":"",
    });
    if html {
        result["stepsFormat"] = json!("html");
    }
    // 独立于远端缓存，禅道同步和移除连接都不会改动本地 BUG。
    tx.execute(
        "INSERT INTO local_bugs(id,revision,data) VALUES (?1,?2,?3)
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,data=excluded.data",
        params![id, revision, result.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn local_bug_persists_and_checks_revision_without_a_connection() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let id = uuid::Uuid::now_v7();
        let mut input = json!({
            "id":id,"title":" 编辑器闪退 ","steps":"<b>按回车</b>",
            "bugType":"codeerror","severity":2,"priority":3
        });
        let submit = |store: &Store, value: &Value| {
            save(store, serde_json::from_value(value.clone()).unwrap())
        };
        let created = submit(&store, &input).unwrap();
        assert_eq!(created["title"], "编辑器闪退");
        assert_eq!(created["steps"], "<b>按回车</b>");
        assert_eq!(created["status"], "active");
        assert_eq!(store.snapshot().unwrap()["bugs"], json!([]));
        assert!(submit(&store, &input).is_err());
        input["revision"] = json!(1);
        input["status"] = json!("resolved");
        let resolved = submit(&store, &input).unwrap();
        assert_eq!(resolved["revision"], 2);
        assert_eq!(resolved["openedDate"], created["openedDate"]);
        assert!(resolved["resolvedDate"].is_string());
        assert!(submit(&store, &input).is_err());
        let archive = root.path().join("local-bugs.zip");
        store.backup(&archive).unwrap();
        drop(store);
        let reopened = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened.workspace_snapshot().unwrap()["bugs"],
            json!([resolved])
        );
        let restored = Store::restore_to(&archive, &root.path().join("restored")).unwrap();
        assert_eq!(
            restored.workspace_snapshot().unwrap()["bugs"],
            json!([resolved])
        );
    }

    #[test]
    fn html_steps_and_pasted_image_survive_reopen_and_unchanged_edit() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let image = STANDARD.encode(crate::bug_steps::fixture_png());
        let steps = format!(
            r#"<p onclick="bad()"><strong>截图</strong>{}<img src="data:image/png;base64,{image}" onerror="bad()"><img src="https://external.invalid/image.png"></p>"#,
            "x".repeat(31_000)
        );
        let mut input = json!({
            "id":uuid::Uuid::now_v7(),"title":"本地富文本","steps":steps,"stepsFormat":"html",
            "bugType":"codeerror","severity":2,"priority":3
        });
        let prepared = prepare_html(serde_json::from_value(input.clone()).unwrap()).unwrap();
        let created = save(&store, prepared).unwrap();
        let clean = created["steps"].as_str().unwrap();
        assert_eq!(created["stepsFormat"], "html");
        assert!(clean.starts_with("<p><strong>截图</strong>"));
        assert!(clean.ends_with(&format!(r#"<img src="data:image/png;base64,{image}"></p>"#)));
        assert!(!clean.contains("bad()"));
        drop(store);

        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(
            store.workspace_snapshot().unwrap()["bugs"],
            json!([created])
        );
        input.as_object_mut().unwrap().remove("stepsFormat");
        input["revision"] = json!(1);
        input["steps"] = created["steps"].clone();
        input["title"] = json!("仅修改标题");
        let edited = save(&store, serde_json::from_value(input).unwrap()).unwrap();
        assert_eq!(edited["steps"], created["steps"]);
        assert_eq!(edited["stepsFormat"], "html");
    }
}
