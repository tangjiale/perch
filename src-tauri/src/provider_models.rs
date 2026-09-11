use crate::{
    integrations::{auth, checked, endpoint, get, http, read_secret, text},
    storage::Store,
    AppState,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};
use tauri::State;

const MAX_MODELS: usize = 5_000;
const MAX_PAGES: usize = 20;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogContext {
    provider_id: String,
    provider_revision: i64,
    workspace_id: String,
    generation: String,
}

#[derive(Debug, Serialize)]
pub struct RemoteModel {
    id: String,
    name: String,
}

#[derive(Serialize)]
pub struct ModelCatalog {
    #[serde(flatten)]
    context: CatalogContext,
    models: Vec<RemoteModel>,
}

#[derive(Deserialize)]
pub struct SelectedModel {
    id: String,
    name: String,
    capability: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    added: usize,
    skipped: usize,
}

fn generation(store: &Store) -> String {
    store
        .generation
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn context(store: &Store, provider: &Value) -> Result<CatalogContext, String> {
    Ok(CatalogContext {
        provider_id: text(provider, "id").to_owned(),
        provider_revision: provider["revision"].as_i64().ok_or("请先保存供应商配置")?,
        workspace_id: store.workspace_id.clone(),
        generation: generation(store),
    })
}

fn validate_context(store: &Store, catalog: &CatalogContext) -> Result<Value, String> {
    if store.workspace_id != catalog.workspace_id || generation(store) != catalog.generation {
        return Err("工作空间已切换或恢复，请重新拉取模型列表".into());
    }
    let provider = get(&store.snapshot()?, "providers", &catalog.provider_id)
        .map_err(|_| "供应商已删除，请重新配置")?;
    if provider["revision"].as_i64() != Some(catalog.provider_revision) {
        return Err("供应商配置已变化，请重新拉取模型列表".into());
    }
    Ok(provider)
}

fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn parse_page(value: Value) -> Result<(Vec<RemoteModel>, Option<String>), String> {
    let data = value["data"]
        .as_array()
        .ok_or("模型目录格式无效：需要 data 数组")?;
    if data.len() > MAX_MODELS {
        return Err("模型目录条目过多，请缩小供应商目录后重试".into());
    }
    let mut models = Vec::with_capacity(data.len());
    for item in data {
        let id = text(item, "id");
        if !valid_label(id) {
            return Err("模型目录包含无效的模型 ID".into());
        }
        let display_name = text(item, "display_name");
        let name = if valid_label(display_name) {
            display_name
        } else {
            id
        };
        models.push(RemoteModel {
            id: id.to_owned(),
            name: name.to_owned(),
        });
    }
    let has_more = match value.get("has_more") {
        Some(value) => value
            .as_bool()
            .ok_or("模型目录分页信息无效，请检查供应商接口")?,
        None => false,
    };
    let next = if has_more {
        let last_id = text(&value, "last_id");
        if !valid_label(last_id) || data.is_empty() {
            return Err("模型目录分页信息无效，请检查供应商接口".into());
        }
        Some(last_id.to_owned())
    } else {
        None
    };
    Ok((models, next))
}

async fn fetch_models(provider: &Value, key: &str) -> Result<Vec<RemoteModel>, String> {
    tokio::time::timeout(Duration::from_secs(60), fetch_pages(provider, key))
        .await
        .map_err(|_| "拉取模型列表超时，请稍后重试".to_string())?
}

async fn fetch_pages(provider: &Value, key: &str) -> Result<Vec<RemoteModel>, String> {
    let protocol = text(provider, "protocol");
    if ![
        "openai-completions",
        "openai-responses",
        "anthropic-messages",
    ]
    .contains(&protocol)
    {
        return Err("当前供应商协议不支持拉取模型列表".into());
    }
    let anthropic = protocol == "anthropic-messages";
    let base = endpoint(text(provider, "baseUrl"), "models")?;
    let client = http()?;
    let mut cursor: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut ids = HashSet::new();
    let mut models = Vec::new();
    for _ in 0..MAX_PAGES {
        let mut url = base.clone();
        if anthropic {
            url.query_pairs_mut().append_pair("limit", "1000");
            if let Some(after) = &cursor {
                url.query_pairs_mut().append_pair("after_id", after);
            }
        }
        let response = auth(
            client.get(url).timeout(Duration::from_secs(20)),
            provider,
            key,
        )
        .send()
        .await
        .map_err(|_| "无法连接模型目录，请检查 API 地址与网络")?;
        let response = checked(response).await?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_PAGE_BYTES as u64)
        {
            return Err("模型目录响应过大，请检查供应商接口".into());
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "读取模型目录失败，请重试")?;
            if bytes.len() + chunk.len() > MAX_PAGE_BYTES {
                return Err("模型目录响应过大，请检查供应商接口".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| "模型目录未返回有效 JSON，请确认 API 根地址包含正确的版本路径")?;
        let (page, next) = parse_page(value)?;
        for model in page {
            if ids.insert(model.id.clone()) {
                models.push(model);
                if models.len() > MAX_MODELS {
                    return Err("模型目录条目过多，请缩小供应商目录后重试".into());
                }
            }
        }
        match next {
            None => {
                models.sort_by(|left, right| left.id.cmp(&right.id));
                return Ok(models);
            }
            Some(next) if anthropic && cursors.insert(next.clone()) => cursor = Some(next),
            Some(_) if !anthropic => {
                return Err("该兼容接口返回了分页目录，暂不支持其分页协议，请手动添加模型".into())
            }
            Some(_) => return Err("模型目录分页重复，已停止拉取，请检查供应商接口".into()),
        }
    }
    Err("模型目录分页过多，未获取完整列表，请手动添加模型".into())
}

#[tauri::command]
pub async fn provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<ModelCatalog, String> {
    let (provider, catalog) = {
        let store = state.store.lock().map_err(|_| "工作空间不可用")?;
        let provider = get(&store.snapshot()?, "providers", &provider_id)?;
        let catalog = context(&store, &provider)?;
        (provider, catalog)
    };
    let key = read_secret(&catalog.workspace_id, &catalog.provider_id)?;
    let models = fetch_models(&provider, &key).await?;
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    validate_context(&store, &catalog)?;
    Ok(ModelCatalog {
        context: catalog,
        models,
    })
}

fn import_models(
    store: &Store,
    catalog: &CatalogContext,
    models: Vec<SelectedModel>,
) -> Result<ImportResult, String> {
    validate_context(store, catalog)?;
    if models.is_empty() || models.len() > MAX_MODELS {
        return Err("请选择 1 至 5000 个模型".into());
    }
    let snapshot = store.snapshot()?;
    let mut existing: HashSet<String> = snapshot["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| text(model, "providerId") == catalog.provider_id)
        .map(|model| text(model, "remoteModelId").to_owned())
        .collect();
    let mut changes = Vec::new();
    let mut skipped = 0;
    // 先验证整批输入，再以单事务落库；已添加的模型保留原有类型与启用配置。
    for model in models {
        if !valid_label(&model.id)
            || !valid_label(&model.name)
            || !["chat", "vision", "embedding"].contains(&model.capability.as_str())
        {
            return Err("模型 ID、名称或类型无效，请重新选择".into());
        }
        if !existing.insert(model.id.clone()) {
            skipped += 1;
            continue;
        }
        changes.push(("model", json!({
            "id": uuid::Uuid::now_v7().to_string(), "providerId": catalog.provider_id,
            "name": model.name, "remoteModelId": model.id, "capability": model.capability, "enabled": true
        })));
    }
    let added = changes.len();
    store.save_batch(
        changes,
        vec![(
            "provider",
            catalog.provider_id.clone(),
            catalog.provider_revision,
        )],
        false,
    )?;
    Ok(ImportResult { added, skipped })
}

#[tauri::command]
pub fn provider_models_import(
    state: State<AppState>,
    catalog: CatalogContext,
    models: Vec<SelectedModel>,
) -> Result<ImportResult, String> {
    // 与其它领域命令共享工作空间锁，使去重快照与提交之间不会插入并发写入。
    let store = state.store.lock().map_err(|_| "工作空间不可用")?;
    import_models(&store, &catalog, models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(
        responses: Vec<(u16, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(5), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0; 1];
                    stream.read_exact(&mut byte).await.unwrap();
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8(request).unwrap());
                stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            requests
        });
        (format!("http://{address}/v1"), task)
    }

    #[tokio::test]
    async fn openai_protocols_auth_empty_key_and_deduplication() {
        for (protocol, key) in [
            ("openai-completions", "test-only-key"),
            ("openai-responses", ""),
        ] {
            let (base, task) = server(vec![(
                200,
                json!({"data":[{"id":"b"},{"id":"a"},{"id":"b"}]}).to_string(),
            )])
            .await;
            let result = fetch_models(&json!({"baseUrl":base,"protocol":protocol}), key)
                .await
                .unwrap();
            assert_eq!(
                result
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b"]
            );
            let requests = task.await.unwrap();
            assert!(requests[0].starts_with("GET /v1/models HTTP/1.1"));
            assert_eq!(
                requests[0].contains("authorization: Bearer test-only-key"),
                !key.is_empty()
            );
        }
    }

    #[tokio::test]
    async fn anthropic_pagination_and_repeated_cursor() {
        let pages = vec![
            (200, json!({"data":[{"id":"first","display_name":"First"}],"has_more":true,"last_id":"first"}).to_string()),
            (200, json!({"data":[{"id":"second"}],"has_more":false}).to_string()),
        ];
        let (base, task) = server(pages).await;
        let models = fetch_models(
            &json!({"baseUrl":base,"protocol":"anthropic-messages"}),
            "test-only-key",
        )
        .await
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "First");
        let requests = task.await.unwrap();
        assert!(requests[0].contains("x-api-key: test-only-key"));
        assert!(requests[0].contains("anthropic-version: 2023-06-01"));
        assert!(requests[1].contains("after_id=first"));
        let page = json!({"data":[{"id":"same"}],"has_more":true,"last_id":"same"}).to_string();
        let (base, task) = server(vec![(200, page.clone()), (200, page)]).await;
        assert!(
            fetch_models(&json!({"baseUrl":base,"protocol":"anthropic-messages"}), "")
                .await
                .unwrap_err()
                .contains("分页重复")
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn errors_are_safe_and_malformed_pages_are_rejected() {
        for (status, body, expected) in [
            (401, "private-server-error", "HTTP 401"),
            (200, "<html>private-server-error</html>", "有效 JSON"),
            (200, "{\"models\":[]}", "data 数组"),
        ] {
            let (base, task) = server(vec![(status, body.to_owned())]).await;
            let error = fetch_models(&json!({"baseUrl":base,"protocol":"openai-completions"}), "")
                .await
                .unwrap_err();
            assert!(error.contains(expected));
            assert!(!error.contains("private-server-error"));
            task.await.unwrap();
        }
        assert!(parse_page(json!({"data":[{"id":""}]})).is_err());
        assert!(parse_page(json!({"data":[],"has_more":true,"last_id":"next"})).is_err());
        assert!(parse_page(json!({"data":[]})).unwrap().0.is_empty());
        assert!(parse_page(json!({"data":[],"has_more":"true"})).is_err());
        assert!(parse_page(json!({"data":vec![json!({"id":"test"}); MAX_MODELS + 1]})).is_err());
    }

    #[tokio::test]
    async fn oversized_responses_and_redirects_are_rejected() {
        for (status, body, expected) in [
            (200, " ".repeat(MAX_PAGE_BYTES + 1), "响应过大"),
            (302, "redirect".into(), "HTTP 302"),
        ] {
            let (base, task) = server(vec![(status, body)]).await;
            let error = fetch_models(
                &json!({"baseUrl":base,"protocol":"openai-completions"}),
                "test-only-key",
            )
            .await
            .unwrap_err();
            assert!(error.contains(expected));
            // 大响应在头部即被拒绝，测试服务的 body 写入可因客户端关闭而失败。
            let _ = task.await;
        }
    }

    fn selected(id: &str, capability: &str) -> SelectedModel {
        SelectedModel {
            id: id.into(),
            name: id.into(),
            capability: capability.into(),
        }
    }

    #[test]
    fn import_is_atomic_deduplicated_and_preserves_configuration() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let provider = store.save("provider", json!({"name":"Test"})).unwrap();
        let catalog = context(&store, &provider).unwrap();
        let other_provider = store.save("provider", json!({"name":"Other"})).unwrap();
        store.save("model", json!({"name":"Other model","providerId":other_provider["id"],"remoteModelId":"new","capability":"chat","enabled":true})).unwrap();
        store.save("model", json!({"name":"Existing","providerId":catalog.provider_id,"remoteModelId":"existing","capability":"embedding","enabled":false})).unwrap();
        let result = import_models(
            &store,
            &catalog,
            vec![
                selected("existing", "chat"),
                selected("new", "vision"),
                selected("new", "chat"),
            ],
        )
        .unwrap();
        assert_eq!((result.added, result.skipped), (1, 2));
        let snapshot = store.snapshot().unwrap();
        let existing = snapshot["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| text(model, "remoteModelId") == "existing")
            .unwrap();
        assert_eq!(existing["capability"], "embedding");
        assert_eq!(existing["enabled"], false);
        assert!(import_models(
            &store,
            &catalog,
            vec![selected("valid", "chat"), selected("bad", "invalid")]
        )
        .is_err());
        assert_eq!(
            store.snapshot().unwrap()["models"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn import_rejects_changed_provider_workspace_or_generation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let mut provider = store.save("provider", json!({"name":"Test"})).unwrap();
        let catalog = context(&store, &provider).unwrap();
        for field in ["workspace", "generation"] {
            let mut stale = catalog.clone();
            if field == "workspace" {
                stale.workspace_id = "stale".into();
            } else {
                stale.generation = "stale".into();
            }
            assert!(import_models(&store, &stale, vec![selected("a", "chat")])
                .unwrap_err()
                .contains("工作空间"));
        }
        provider["name"] = json!("Updated");
        store.save("provider", provider).unwrap();
        assert!(import_models(&store, &catalog, vec![selected("a", "chat")])
            .unwrap_err()
            .contains("供应商配置已变化"));
        assert!(store.snapshot().unwrap()["models"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
