use crate::{data::text, storage::Store, website_icon::website_icon_fetch, AppState};
use futures_util::{stream, StreamExt};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::Semaphore, time::Instant};
use url::Url;

const ICON_CONCURRENCY: usize = 4;
static ICON_SLOTS: Semaphore = Semaphore::const_new(ICON_CONCURRENCY);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum CategoryChoice {
    Existing { id: String },
    Name { name: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookmarkAppInput {
    name: String,
    url: String,
    category: Option<CategoryChoice>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    created: usize,
    skipped: usize,
    categories_created: usize,
}

type AppsByOrigin = HashMap<String, Vec<Value>>;

fn bookmark_url(raw: &str) -> Option<Url> {
    let raw = raw.trim();
    if raw.chars().any(char::is_control) {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

fn category_id(
    tx: &Transaction<'_>,
    workspace_id: &str,
    choice: CategoryChoice,
    result: &mut ImportResult,
) -> Result<String, String> {
    match choice {
        CategoryChoice::Existing { id } => tx
            .query_row("SELECT id FROM categories WHERE id=?1", [&id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "所选应用分类已被删除，请刷新应用分类后重新选择。".into()),
        CategoryChoice::Name { name } => {
            let name = name.trim();
            let existing = tx
                .query_row(
                    "SELECT id FROM categories WHERE normalized_name=?1",
                    [name.to_lowercase()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if let Some(id) = existing {
                return Ok(id);
            }
            let saved = Store::save_in_transaction(
                tx,
                workspace_id,
                "categories",
                json!({"name": name}),
                false,
            )?;
            result.categories_created += 1;
            Ok(text(&saved, "id").to_owned())
        }
    }
}

fn import_apps(
    store: &Store,
    items: Vec<BookmarkAppInput>,
) -> Result<(ImportResult, AppsByOrigin), String> {
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut known_urls = {
        let mut stmt = tx
            .prepare("SELECT url FROM apps")
            .map_err(|e| e.to_string())?;
        let mut urls = HashSet::new();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for raw in rows {
            if let Some(url) = bookmark_url(&raw.map_err(|e| e.to_string())?) {
                urls.insert(url.to_string());
            }
        }
        urls
    };
    let mut result = ImportResult::default();
    let mut apps_by_origin = AppsByOrigin::new();
    let sort_order = chrono::Utc::now().timestamp_millis();
    for (index, input) in items.into_iter().enumerate() {
        let url = bookmark_url(&input.url).ok_or_else(|| {
            format!(
                "第 {} 条书签须使用不含账号密码的 HTTP 或 HTTPS 网址。",
                index + 1
            )
        })?;
        if !known_urls.insert(url.to_string()) {
            result.skipped += 1;
            continue;
        }
        let category = input
            .category
            .map(|choice| category_id(&tx, &store.workspace_id, choice, &mut result))
            .transpose()?;
        let saved = Store::save_in_transaction(
            &tx,
            &store.workspace_id,
            "apps",
            json!({
                "name": input.name.trim(),
                "url": url.as_str(),
                "description": "",
                "categoryId": category,
                "favorite": false,
                "sortOrder": sort_order + index as i64,
            }),
            false,
        )?;
        result.created += 1;
        // 仅用站点源发现图标，不向站点发送书签的路径、查询参数或片段。
        apps_by_origin
            .entry(url.origin().ascii_serialization())
            .or_default()
            .push(saved);
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok((result, apps_by_origin))
}

fn apply_imported_icon(store: &Store, apps: Vec<Value>, icon: &str) -> bool {
    let mut updated = false;
    for mut saved in apps {
        // 使用导入时的 revision，避免覆盖后来修改或已删除的应用。
        saved["logo"] = json!(icon);
        updated |= store.save("app", saved).is_ok();
    }
    updated
}

async fn fetch_icons(
    app: AppHandle,
    workspace_id: String,
    generation: PathBuf,
    apps_by_origin: AppsByOrigin,
) {
    let state = app.state::<AppState>();
    let current_generation = || {
        state
            .store
            .lock()
            .is_ok_and(|store| store.generation == generation)
    };
    let mut requests = stream::iter(apps_by_origin)
        .map(|(origin, apps)| async {
            let _slot = ICON_SLOTS.acquire().await.ok()?;
            if !current_generation() {
                return None;
            }
            let icon = website_icon_fetch(origin).await.ok()?;
            Some((icon, apps))
        })
        .buffer_unordered(ICON_CONCURRENCY);
    let mut updated = false;
    let mut last_refresh = Instant::now();
    while let Some(result) = requests.next().await {
        if !current_generation() {
            return;
        }
        let Some((icon, apps)) = result else {
            continue;
        };
        if let Ok(store) = state.store.lock() {
            if store.generation != generation {
                return;
            }
            updated |= apply_imported_icon(&store, apps, &icon);
        }
        if updated && last_refresh.elapsed() >= Duration::from_secs(1) {
            let _ = app.emit("apps-updated", json!({"workspaceId": workspace_id}));
            updated = false;
            last_refresh = Instant::now();
        }
    }
    if updated && current_generation() {
        let _ = app.emit("apps-updated", json!({"workspaceId": workspace_id}));
    }
}

#[tauri::command]
pub async fn import_browser_bookmark_apps(
    app: AppHandle,
    workspace_id: String,
    items: Vec<BookmarkAppInput>,
) -> Result<ImportResult, String> {
    let handle = app.clone();
    let active_workspace = workspace_id.clone();
    let (result, apps_by_origin, generation) = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.workspace_id != active_workspace {
            return Err("工作空间已切换，请重新打开书签同步。".into());
        }
        let (result, apps_by_origin) = import_apps(&store, items)?;
        Ok::<_, String>((result, apps_by_origin, store.generation.clone()))
    })
    .await
    .map_err(|_| "无法完成书签导入，请重试。")??;
    if !apps_by_origin.is_empty() {
        tauri::async_runtime::spawn(fetch_icons(app, workspace_id, generation, apps_by_origin));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, url: &str, category: Value) -> BookmarkAppInput {
        serde_json::from_value(json!({"name": name, "url": url, "category": category})).unwrap()
    }

    #[test]
    fn import_reuses_categories_and_skips_existing_urls() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_at(directory.path().to_path_buf()).unwrap();
        let existing = store
            .save("category", json!({"name": "研发 Docs"}))
            .unwrap();
        let existing_app = store
            .save(
                "app",
                json!({"name": "原名称", "url": "https://example.test"}),
            )
            .unwrap();
        let selection = || {
            vec![
                input(
                    "重复",
                    "https://EXAMPLE.test/",
                    json!({"kind": "name", "name": "无需创建"}),
                ),
                input(
                    "第一篇",
                    "https://example.test/one?view=full#start",
                    json!({"kind": "name", "name": " 研发 DOCS "}),
                ),
                input(
                    "第二篇",
                    "https://example.test/one?view=short#start",
                    json!({"kind": "name", "name": "新分类"}),
                ),
                input(
                    "第三篇",
                    "https://example.test/one?view=short#end",
                    json!({"kind": "name", "name": "新分类"}),
                ),
                input(
                    "第四篇",
                    "https://example.test/four",
                    json!({"kind": "existing", "id": existing["id"]}),
                ),
                input("其他站点", "http://other.test:8080/view", Value::Null),
            ]
        };
        let (result, sites) = import_apps(&store, selection()).unwrap();
        assert_eq!(
            (result.created, result.skipped, result.categories_created),
            (5, 1, 1)
        );
        let documents = &sites["https://example.test"];
        assert_eq!(documents.len(), 4);
        assert_eq!(documents[0]["categoryId"], existing["id"]);
        assert_eq!(
            documents[0]["url"],
            "https://example.test/one?view=full#start"
        );
        assert_eq!(documents[1]["categoryId"], documents[2]["categoryId"]);
        assert_eq!(documents[3]["categoryId"], existing["id"]);
        assert!(sites["http://other.test:8080"][0]["categoryId"].is_null());
        let snapshot = store.workspace_snapshot().unwrap();
        assert_eq!(snapshot["categories"].as_array().unwrap().len(), 2);
        assert!(snapshot["apps"].as_array().unwrap().contains(&existing_app));

        let (retried, sites) = import_apps(&store, selection()).unwrap();
        assert_eq!(
            (retried.created, retried.skipped, retried.categories_created),
            (0, 6, 0)
        );
        assert!(sites.is_empty());
    }

    #[test]
    fn failure_rolls_back_new_categories_and_apps() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_at(directory.path().to_path_buf()).unwrap();
        for invalid in [
            input("无效网址", "file:///private", Value::Null),
            input("含凭据网址", "https://user@example.test", Value::Null),
            input(
                "分组已删除",
                "https://other.test",
                json!({"kind": "existing", "id": "deleted"}),
            ),
            input(" ", "https://other.test", Value::Null),
        ] {
            assert!(import_apps(
                &store,
                vec![
                    input(
                        "有效",
                        "https://example.test/valid",
                        json!({"kind": "name", "name": "随应用创建"})
                    ),
                    invalid,
                ]
            )
            .is_err());
            let snapshot = store.workspace_snapshot().unwrap();
            assert_eq!(snapshot["categories"], json!([]));
            assert_eq!(snapshot["apps"], json!([]));
        }
    }

    #[test]
    fn automatic_icon_survives_only_for_unchanged_apps() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_at(directory.path().to_path_buf()).unwrap();
        let (_, mut origins) = import_apps(
            &store,
            vec![
                input("第一项", "https://example.test/first", Value::Null),
                input("第二项", "https://example.test/second", Value::Null),
                input("第三项", "https://example.test/third", Value::Null),
            ],
        )
        .unwrap();
        let apps = origins.remove("https://example.test").unwrap();
        let mut edited = apps[1].clone();
        edited["logo"] = json!("data:image/png;base64,manual");
        edited["name"] = json!("手动编辑");
        let edited = store.save("app", edited).unwrap();
        store.delete("app", text(&apps[2], "id"), Some(1)).unwrap();
        assert!(apply_imported_icon(
            &store,
            apps,
            "data:image/png;base64,automatic"
        ));

        let snapshot = store.workspace_snapshot().unwrap();
        let apps = snapshot["apps"].as_array().unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0]["logo"], "data:image/png;base64,automatic");
        assert_eq!(apps[1], edited);
    }
}
