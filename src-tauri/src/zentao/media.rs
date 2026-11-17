//! 禅道 22.0 api/v1/entries/{file,files}.php：Token 下载及 imgFile 上传。
use super::execution_context::{adapter, context, unchanged};
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

const LIMIT: usize = 5 * 1024 * 1024;

fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[255, 216, 255]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

// 只提取已知禅道文件 ID，绝不直接请求 HTML 中提供的地址。
fn file_id(base: &str, source: &str) -> Result<String, String> {
    let source = source.trim();
    if source.len() > 2048 {
        return Err("图片地址过长".into());
    }
    if let Some(inner) = source.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let (id, ext) = inner.split_once('.').ok_or("无效禅道图片引用")?;
        if !["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.to_ascii_lowercase().as_str()) {
            return Err("不支持的图片格式".into());
        }
        return remote_id(&json!(id));
    }
    let root = endpoint(base, "")?;
    let url = root.join(source).map_err(|_| "无效图片地址")?;
    if url.origin() != root.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with(root.path())
    {
        return Err("仅支持当前禅道站点内的图片引用".into());
    }
    let relative = url.path().strip_prefix(root.path()).ok_or("无效图片地址")?;
    if relative == "index.php" || relative.is_empty() {
        let pairs: HashMap<_, _> = url.query_pairs().collect();
        if pairs.get("m").map(|s| s.as_ref()) == Some("file")
            && pairs.get("f").map(|s| s.as_ref()) == Some("read")
        {
            return remote_id(&json!(pairs.get("fileID").ok_or("图片地址缺少文件编号")?));
        }
    }
    if let Some(name) = relative.strip_prefix("file-read-") {
        if let Some((id, ext)) = name.split_once('.') {
            if ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.to_ascii_lowercase().as_str()) {
                return remote_id(&json!(id));
            }
        }
    }
    Err("不支持的禅道图片地址，请在禅道核对原始附件".into())
}

fn referenced(base: &str, html: &str, id: &str) -> bool {
    let doc = scraper::Html::parse_fragment(html);
    let selector = scraper::Selector::parse("img[src]").expect("静态选择器");
    doc.select(&selector).any(|img| {
        img.value()
            .attr("src")
            .is_some_and(|source| file_id(base, source).ok().as_deref() == Some(id))
    })
}

async fn bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("禅道图片或响应超过大小限制".into());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "禅道图片响应中断；上传时请先核对附件，勿直接重复提交")?;
        if bytes.len() + chunk.len() > limit {
            return Err("禅道图片或响应超过大小限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_image(adapter: &Adapter<'_>, id: &str) -> Result<String, String> {
    let response = adapter
        .read(endpoint(&adapter.base, &format!("files/{id}"))?)
        .await?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = bounded(response, LIMIT).await?;
    let (detected, _) = image_type(&bytes).ok_or("禅道未返回有效图片，可能无权限或附件已删除")?;
    if mime != detected && mime != "application/octet-stream" {
        return Err("禅道图片类型与内容不一致".into());
    }
    Ok(format!("data:{detected};base64,{}", STANDARD.encode(bytes)))
}

async fn upload_image(
    adapter: &Adapter<'_>,
    base: &str,
    name: &str,
    bytes: Vec<u8>,
    mime: &str,
    ext: &str,
) -> Result<String, String> {
    // filesEntry::post -> file::ajaxUpload 的固定字段为 imgFile。POST 不续登、不重放。
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(format!(
            "{}.{}",
            name.rsplit_once('.').map_or(name, |(stem, _)| stem),
            ext
        ))
        .mime_str(mime)
        .map_err(|_| "图片类型无效")?;
    let response = adapter
        .client
        .post(endpoint(&adapter.base, "files")?)
        .header("Token", adapter.token()?)
        .multipart(reqwest::multipart::Form::new().part("imgFile", part))
        .send()
        .await
        .map_err(|_| "图片上传结果未知，请先在禅道核对附件，勿直接重复上传")?;
    let response = checked_zentao(response).await?;
    let body: Value = serde_json::from_slice(&bounded(response, 64 * 1024).await?)
        .map_err(|_| "图片上传结果无法解析，请先在禅道核对附件")?;
    let id = remote_id(&body["id"])?;
    if file_id(base, text(&body, "url"))? != id {
        return Err("上传返回的图片编号与地址不一致，请在禅道核对附件".into());
    }
    Ok(id)
}

#[tauri::command]
pub async fn zentao_bug_image_read(
    state: State<'_, AppState>,
    bug_id: String,
    source: String,
) -> Result<Value, String> {
    let ctx = bug_context(&state, &bug_id)?;
    file_id(text(&ctx.connection, "baseUrl"), &source)?;
    let token = credential(&ctx.workspace, text(&ctx.connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter =
        Adapter::personal(&ctx.connection, token)?.with_auth(crate::zentao_auth::AutoAuth::new(
            &state,
            &ctx.workspace,
            &ctx.generation,
            &ctx.connection,
        )?);
    let data_url = read_bug_image(
        &adapter,
        text(&ctx.connection, "baseUrl"),
        &ctx.bug,
        &source,
    )
    .await?;
    bug_unchanged(&state, &ctx)?;
    // 文件编号只用于下载，不将临时 data URL 写入 BUG 缓存或远端步骤。
    Ok(json!({"dataUrl": data_url}))
}

/// 历史图片仅可读取指定真实动作的评论引用，不能借用步骤或其他动作的引用。
#[tauri::command]
pub async fn zentao_bug_history_image_read(
    state: State<'_, AppState>,
    bug_id: String,
    action_id: String,
    source: String,
) -> Result<Value, String> {
    let ctx = bug_context(&state, &bug_id)?;
    file_id(text(&ctx.connection, "baseUrl"), &source)?;
    remote_id(&json!(action_id))?;
    let token = credential(&ctx.workspace, text(&ctx.connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    let adapter =
        Adapter::personal(&ctx.connection, token)?.with_auth(crate::zentao_auth::AutoAuth::new(
            &state,
            &ctx.workspace,
            &ctx.generation,
            &ctx.connection,
        )?);
    let scope = super::history_media_cache::scope(&ctx.workspace, &ctx.generation, &ctx.connection);
    let history_key = super::history_media_cache::history_key(&scope, &ctx.bug);
    let data_url = read_bug_history_image_cached(
        (&scope, &history_key),
        &adapter,
        text(&ctx.connection, "baseUrl"),
        &ctx.bug,
        &action_id,
        &source,
        || bug_unchanged(&state, &ctx),
    )
    .await?;
    Ok(json!({"dataUrl":data_url}))
}

#[cfg(test)]
async fn read_bug_history_image(
    adapter: &Adapter<'_>,
    site_base: &str,
    bug: &Value,
    action_id: &str,
    source: &str,
    unchanged: impl Fn() -> Result<(), String>,
) -> Result<String, String> {
    read_bug_history_image_cached(
        ("", ""),
        adapter,
        site_base,
        bug,
        action_id,
        source,
        unchanged,
    )
    .await
}

async fn read_bug_history_image_cached(
    cache_keys: (&str, &str),
    adapter: &Adapter<'_>,
    site_base: &str,
    bug: &Value,
    action_id: &str,
    source: &str,
    unchanged: impl Fn() -> Result<(), String>,
) -> Result<String, String> {
    let (scope, history_key) = cache_keys;
    let id = file_id(site_base, source)?;
    let action_id = remote_id(&json!(action_id))?;
    unchanged()?;
    let history = if let Some(history) = super::history_media_cache::history(history_key) {
        history
    } else {
        let account = adapter.account().await?;
        unchanged()?;
        let remote_id = remote_id(&bug["remoteId"])?;
        let remote = adapter.bug_history(&remote_id).await?;
        super::bugs::verify_read_baseline(bug, &remote, &account)?;
        let history = super::bug_history::history(&remote, &remote_id)?;
        unchanged()?;
        if !history_key.is_empty() {
            super::history_media_cache::put_history(history_key.to_owned(), history.clone());
        }
        history
    };
    let entry = history["entries"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| text(entry, "id") == action_id))
        .ok_or("当前 BUG 不存在指定历史动作")?;
    if !referenced(site_base, text(entry, "comment"), &id) {
        return Err("该图片不在指定 BUG 历史备注中，请重新加载核对".into());
    }
    unchanged()?;
    let image_key = super::history_media_cache::image_key(scope, &id);
    let lock = super::history_media_cache::lock(&image_key);
    let _guard = lock.lock().await;
    unchanged()?;
    let data_url = if !scope.is_empty() {
        super::history_media_cache::image(&image_key)
    } else {
        None
    };
    let data_url = match data_url {
        Some(image) => image,
        None => {
            let image = read_image(adapter, &id).await?;
            unchanged()?;
            if !scope.is_empty() {
                super::history_media_cache::put_image(image_key, image.clone());
            }
            image
        }
    };
    unchanged()?;
    Ok(data_url)
}

struct BugContext {
    bug: Value,
    connection: Value,
    workspace: String,
    generation: std::path::PathBuf,
}

fn bug_context(state: &AppState, bug_id: &str) -> Result<BugContext, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    let bug = get(&snapshot, "bugs", bug_id)?;
    let connection = get(&snapshot, "connections", text(&bug, "connectionId"))?;
    if connection["enabled"] == false {
        return Err("该禅道连接已停用".into());
    }
    remote_id(&bug["remoteId"])?;
    Ok(BugContext {
        bug,
        connection,
        workspace: store.workspace_id.clone(),
        generation: store.generation.clone(),
    })
}

fn bug_unchanged(state: &AppState, ctx: &BugContext) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != ctx.workspace || store.generation != ctx.generation {
        return Err("工作空间已切换，请重新打开 BUG".into());
    }
    let snapshot = store.snapshot()?;
    let connection = get(&snapshot, "connections", text(&ctx.connection, "id"))?;
    let bug = get(&snapshot, "bugs", text(&ctx.bug, "id"))?;
    if connection != ctx.connection || bug != ctx.bug {
        return Err("连接或 BUG 已变化，请重新打开 BUG".into());
    }
    Ok(())
}

async fn read_bug_image(
    adapter: &Adapter<'_>,
    site_base: &str,
    bug: &Value,
    source: &str,
) -> Result<String, String> {
    // Adapter.base 是 API 根；附件引用属于站点根，不能以 API 路径校验。
    let id = file_id(site_base, source)?;
    let account = adapter.account().await?;
    let remote = adapter
        .bug_request(&remote_id(&bug["remoteId"])?, None)
        .await?;
    super::bugs::verify_read_baseline(bug, &remote, &account)?;
    if !referenced(site_base, text(&remote, "steps"), &id) {
        return Err("该图片不在当前禅道 BUG 复现步骤中，请重新同步核对".into());
    }
    read_image(adapter, &id).await
}

#[tauri::command]
pub async fn zentao_task_image_read(
    state: State<'_, AppState>,
    task_id: String,
    source: String,
) -> Result<Value, String> {
    let ctx = context(&state, &task_id)?;
    let id = file_id(text(&ctx.connection, "baseUrl"), &source)?;
    let adapter = adapter(&state, &ctx)?;
    let account = adapter.account().await?;
    let remote = adapter
        .execution_request(&remote_id(&ctx.task["remoteId"])?, None)
        .await?;
    verify_execution_scope(&remote, &ctx.project, &account)?;
    if !referenced(text(&ctx.connection, "baseUrl"), text(&remote, "desc"), &id) {
        return Err("该图片不在当前禅道执行描述中，请重新同步核对".into());
    }
    let data_url = read_image(&adapter, &id).await?;
    unchanged(&state, &ctx)?;
    Ok(json!({"dataUrl": data_url}))
}

#[tauri::command]
pub async fn zentao_task_image_upload(
    state: State<'_, AppState>,
    task_id: String,
    name: String,
    data_url: String,
) -> Result<Value, String> {
    if name.trim().is_empty()
        || name.len() > 255
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err("图片文件名无效".into());
    }
    if data_url.len() > LIMIT.div_ceil(3) * 4 + 64 {
        return Err("单张图片不能超过 5 MB".into());
    }
    let (header, encoded) = data_url.split_once(',').ok_or("图片数据无效")?;
    let bytes = STANDARD.decode(encoded).map_err(|_| "图片编码无效")?;
    if bytes.len() > LIMIT {
        return Err("单张图片不能超过 5 MB".into());
    }
    let (mime, ext) = image_type(&bytes).ok_or("只支持 PNG、JPEG、GIF、WebP 图片")?;
    if header != format!("data:{mime};base64") {
        return Err("图片类型与内容不一致".into());
    }
    let ctx = context(&state, &task_id)?;
    let _guard = SyncGuard::acquire(&ctx.workspace, text(&ctx.connection, "id"))?;
    let adapter = adapter(&state, &ctx)?;
    let account = adapter.account().await?;
    let remote = adapter
        .execution_request(&remote_id(&ctx.task["remoteId"])?, None)
        .await?;
    verify_execution_edit(
        text(&ctx.connection, "baseUrl"),
        &ctx.task,
        &remote,
        &json!({"desc":text(&ctx.task, "notes")}),
        &ctx.project,
        &account,
    )?;
    unchanged(&state, &ctx)?;
    let id = upload_image(
        &adapter,
        text(&ctx.connection, "baseUrl"),
        &name,
        bytes,
        mime,
        ext,
    )
    .await?;
    unchanged(&state, &ctx)?;
    Ok(json!({"source": format!("{{{id}.{ext}}}"), "fileId": id, "dataUrl":data_url}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bug_fixture() -> (Value, Value) {
        let steps = r#"<p><img src="/index.php?m=file&amp;f=read&amp;fileID=708&amp;t=png"></p>"#;
        (
            json!({"id":"b","connectionId":"c","remoteId":"419","title":"截图 BUG","steps":steps,"status":"active","assignedTo":"me","severity":2,"priority":3,"productId":"1","remoteEditedDate":"2026-10-13 12:00:00"}),
            json!({"id":419,"title":"截图 BUG","steps":steps,"status":"active","assignedTo":{"account":"me"},"severity":2,"pri":3,"product":1,"lastEditedDate":"2026-10-13 12:00:00"}),
        )
    }

    fn history_fixture() -> (Value, Value) {
        let (old, mut remote) = bug_fixture();
        remote["actions"] = json!([{"id":12,"objectType":"bug","objectID":419,"action":"commented","actor":"me","date":"2026-10-28 12:00:00","comment":"<img src=\"index.php?m=file&amp;f=read&amp;t=png&amp;fileID=768\">","extra":"","history":[]}]);
        (old, remote)
    }
    #[tokio::test]
    async fn history_image_expired_authorization_must_revalidate_before_cached_pixels() {
        let (old, mut remote) = history_fixture();
        remote["actions"][0]["comment"] = json!("图片引用已删除");
        let (base, handle) = crate::zentao::tests::auth_server(vec![
            (200, json!({"profile":{"account":"me"}})),
            (200, remote.clone()),
        ]);
        let scope = super::super::history_media_cache::scope(
            &uuid::Uuid::now_v7().to_string(),
            std::path::Path::new("generation"),
            &json!({"baseUrl":base}),
        );
        let key = super::super::history_media_cache::history_key(&scope, &old);
        let (_, initial) = history_fixture();
        super::super::history_media_cache::put_history(
            key.clone(),
            super::super::bug_history::history(&initial, "419").unwrap(),
        );
        super::super::history_media_cache::put_image(
            super::super::history_media_cache::image_key(&scope, "768"),
            "data:image/png;base64,cached".into(),
        );
        super::super::history_media_cache::expire_history(&key);
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &test_adapter(&base),
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(())
        )
        .await
        .is_err());
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("/bugs/419?fields=actions"));
        assert!(!requests.iter().any(|r| r.contains("/files/")));
        super::super::history_media_cache::invalidate(&key);
        assert!(super::super::history_media_cache::history(&key).is_none());
    }

    #[tokio::test]
    async fn history_image_cache_does_not_store_failed_downloads() {
        let (old, remote) = history_fixture();
        let (base, handle) = sequence_server(vec![
            (200, "text/html".into(), b"login".to_vec()),
            (
                200,
                "image/png".into(),
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            ),
        ]);
        let scope = super::super::history_media_cache::scope(
            &uuid::Uuid::now_v7().to_string(),
            std::path::Path::new("generation"),
            &json!({"baseUrl":base}),
        );
        let key = super::super::history_media_cache::history_key(&scope, &old);
        super::super::history_media_cache::put_history(
            key.clone(),
            super::super::bug_history::history(&remote, "419").unwrap(),
        );
        let adapter = test_adapter(&base);
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(())
        )
        .await
        .is_err());
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(())
        )
        .await
        .is_ok());
        assert_eq!(
            handle
                .join()
                .unwrap()
                .matches("GET /api.php/v1/files/768 ")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn history_image_cache_downloads_once_and_checks_each_action_before_hits() {
        let (old, remote) = history_fixture();
        let (base, handle) = sequence_server(vec![(
            200,
            "image/png".into(),
            b"\x89PNG\r\n\x1a\nfixture".to_vec(),
        )]);
        let scope = super::super::history_media_cache::scope(
            &uuid::Uuid::now_v7().to_string(),
            std::path::Path::new("generation"),
            &json!({"baseUrl":base,"revision":1,"syncAccount":"me"}),
        );
        let key = super::super::history_media_cache::history_key(&scope, &old);
        super::super::history_media_cache::put_history(
            key.clone(),
            super::super::bug_history::history(&remote, "419").unwrap(),
        );
        let adapter = test_adapter(&base);
        let first = read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(()),
        );
        let second = read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(()),
        );
        let (a, b) = tokio::join!(first, second);
        assert_eq!(a.unwrap(), b.unwrap());
        let requests = handle.join().unwrap();
        assert_eq!(requests.matches("GET ").count(), 1);
        assert!(requests.contains("GET /api.php/v1/files/768 "));
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Ok(())
        )
        .await
        .is_ok());
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "99",
            "{768.png}",
            || Ok(())
        )
        .await
        .is_err());
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{999.png}",
            || Ok(())
        )
        .await
        .is_err());
        assert!(read_bug_history_image_cached(
            (&scope, &key),
            &adapter,
            &base,
            &old,
            "12",
            "{768.png}",
            || Err("工作空间变化".into())
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn history_image_reads_only_verified_comment_via_fixed_file_endpoint() {
        let (old, remote) = history_fixture();
        let (base, handle) = sequence_server(vec![
            (
                200,
                "application/json".into(),
                json!({"profile":{"account":"me"}}).to_string().into_bytes(),
            ),
            (
                200,
                "application/json".into(),
                remote.to_string().into_bytes(),
            ),
            (
                200,
                "image/png".into(),
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            ),
        ]);
        let image = read_bug_history_image(
            &test_adapter(&base),
            &base,
            &old,
            "12",
            "index.php?m=file&f=read&t=png&fileID=768",
            || Ok(()),
        )
        .await
        .unwrap();
        assert!(image.starts_with("data:image/png;base64,"));
        let requests = handle.join().unwrap();
        assert!(requests.contains("GET /api.php/v1/bugs/419?fields=actions HTTP/1.1"));
        assert!(requests.contains("GET /api.php/v1/files/768 HTTP/1.1"));
        assert!(!requests.contains("GET /index.php"));
    }
    #[tokio::test]
    async fn history_image_rejects_wrong_action_scope_reference_and_changed_baseline_before_file() {
        for case in [
            "action",
            "foreign",
            "duplicate",
            "bug",
            "version",
            "owner",
            "unreferenced",
            "other-comment",
        ] {
            let (old, mut remote) = history_fixture();
            match case {
                "foreign" => remote["actions"][0]["objectID"] = json!(420),
                "duplicate" => {
                    let duplicate = remote["actions"][0].clone();
                    remote["actions"].as_array_mut().unwrap().push(duplicate);
                }
                "bug" => remote["id"] = json!(420),
                "version" => remote["lastEditedDate"] = json!("2026-10-28 12:00:00"),
                "owner" => remote["assignedTo"] = json!("other"),
                "other-comment" => {
                    let mut other = remote["actions"][0].clone();
                    other["id"] = json!(13);
                    remote["actions"][0]["comment"] = json!("");
                    remote["actions"].as_array_mut().unwrap().push(other);
                }
                _ => (),
            }
            let (base, handle) = crate::zentao::tests::auth_server(vec![
                (200, json!({"profile":{"account":"me"}})),
                (200, remote),
            ]);
            let action = if case == "action" { "99" } else { "12" };
            let source = if case == "unreferenced" {
                "{999.png}"
            } else {
                "{768.png}"
            };
            assert!(
                read_bug_history_image(
                    &test_adapter(&base),
                    &base,
                    &old,
                    action,
                    source,
                    || Ok(())
                )
                .await
                .is_err(),
                "{case}"
            );
            let requests = handle.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(!requests.iter().any(|r| r.contains("/files/")));
        }
    }
    #[tokio::test]
    async fn history_image_checks_external_source_before_network_and_local_revision_before_file() {
        let (old, remote) = history_fixture();
        let adapter = test_adapter("http://127.0.0.1:1");
        let called = std::cell::Cell::new(false);
        assert!(read_bug_history_image(
            &adapter,
            "http://127.0.0.1:1",
            &old,
            "12",
            "https://outside.example/index.php?m=file&f=read&fileID=768",
            || {
                called.set(true);
                Ok(())
            }
        )
        .await
        .is_err());
        assert!(!called.get());
        let (base, handle) = crate::zentao::tests::auth_server(vec![
            (200, json!({"profile":{"account":"me"}})),
            (200, remote),
        ]);
        let checks = std::cell::Cell::new(0);
        assert!(read_bug_history_image(
            &test_adapter(&base),
            &base,
            &old,
            "12",
            "{768.png}",
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 3 {
                    Err("工作空间或版本已变化".into())
                } else {
                    Ok(())
                }
            }
        )
        .await
        .is_err());
        assert_eq!(handle.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn bug_read_checks_identity_association_and_reference_before_download() {
        let (old, remote) = bug_fixture();
        for case in [
            "assigned",
            "null",
            "missing",
            "product",
            "project",
            "deleted",
            "id",
            "steps",
            "unreferenced",
        ] {
            let mut changed = remote.clone();
            match case {
                "assigned" => changed["assignedTo"] = json!("other"),
                "null" => changed["assignedTo"] = Value::Null,
                "missing" => {
                    changed.as_object_mut().unwrap().remove("assignedTo");
                }
                "product" => changed["product"] = json!(2),
                "project" => changed["project"] = json!(2),
                "deleted" => changed["deleted"] = json!(true),
                "id" => changed["id"] = json!(420),
                "steps" => changed["steps"] = json!("远端已修改"),
                _ => (),
            }
            let (base, handle) = crate::zentao::tests::auth_server(vec![
                (200, json!({"profile":{"account":"me"}})),
                (200, changed),
            ]);
            let source = if case == "unreferenced" {
                "{709.png}"
            } else {
                "{708.png}"
            };
            assert!(
                read_bug_image(&test_adapter(&base), &base, &old, source)
                    .await
                    .is_err(),
                "{case}"
            );
            let requests = handle.join().unwrap();
            assert!(requests[0].starts_with("GET /api.php/v1/user "));
            assert!(requests[1].starts_with("GET /api.php/v1/bugs/419 "));
            assert!(requests
                .iter()
                .all(|r| r.to_lowercase().contains("token: test-token")));
        }
    }

    #[test]
    fn bug_read_accepts_only_own_just_closed_cache() {
        let (mut old, mut remote) = bug_fixture();
        old["status"] = json!("closed");
        old["assignedTo"] = json!("");
        old["lastAssignedTo"] = json!("me");
        remote["status"] = json!("closed");
        remote["assignedTo"] = Value::Null;
        super::super::bugs::verify_baseline(&old, &remote, "me").unwrap();
        assert!(super::super::bugs::verify_baseline(&old, &remote, "other").is_err());
        remote["assignedTo"] = json!("other");
        assert!(super::super::bugs::verify_baseline(&old, &remote, "me").is_err());
    }

    #[test]
    fn bug_read_rejects_workspace_connection_and_cache_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(tmp.path().join("workspace")).unwrap();
        store
            .save("connection", json!({"id":"c","name":"模拟"}))
            .unwrap();
        let (bug, _) = bug_fixture();
        store.save_batch(vec![("bug", bug)], vec![], true).unwrap();
        let state = AppState {
            store: std::sync::Mutex::new(store),
        };
        let ctx = bug_context(&state, "b").unwrap();
        bug_unchanged(&state, &ctx).unwrap();
        {
            let store = state.store.lock().unwrap();
            let mut changed = ctx.bug.clone();
            changed["title"] = json!("更新缓存");
            store
                .save_batch(vec![("bug", changed)], vec![], true)
                .unwrap();
        }
        assert!(bug_unchanged(&state, &ctx).is_err());
        let ctx = bug_context(&state, "b").unwrap();
        {
            let store = state.store.lock().unwrap();
            let mut changed = ctx.connection.clone();
            changed["name"] = json!("更新连接");
            store.save("connection", changed).unwrap();
        }
        assert!(bug_unchanged(&state, &ctx).is_err());
        let ctx = bug_context(&state, "b").unwrap();
        state.store.lock().unwrap().generation = tmp.path().join("other");
        assert!(bug_unchanged(&state, &ctx).is_err());
    }
    fn server(status: u16, mime: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        sequence_server(vec![(status, mime.to_owned(), body)])
    }
    fn sequence_server(
        responses: Vec<(u16, String, Vec<u8>)>,
    ) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = String::new();
            for (status, mime, body) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|s| {
                                s.strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                write!(socket, "HTTP/1.1 {status} Response\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                socket.write_all(&body).unwrap();
                requests.push_str(&String::from_utf8_lossy(&request));
            }
            requests
        });
        (base, handle)
    }
    fn test_adapter(base: &str) -> Adapter<'static> {
        Adapter::personal(
            &json!({"baseUrl":base,"apiVersion":"v1"}),
            "test-token".into(),
        )
        .unwrap()
    }
    #[tokio::test]
    async fn bug_image_download_resolves_same_site_source_and_returns_binary_data() {
        for directory in ["", "/zentao"] {
            for representation in ["root", "absolute", "relative", "placeholder"] {
                let (mut bug, mut remote) = bug_fixture();
                let steps = format!(
                    r#"<p><img src="{directory}/index.php?m=file&amp;f=read&amp;fileID=708&amp;t=png"></p>"#
                );
                bug["steps"] = json!(steps);
                remote["steps"] = bug["steps"].clone();
                let (base, handle) = sequence_server(vec![
                    (
                        200,
                        "application/json".into(),
                        json!({"profile":{"account":"me"}}).to_string().into_bytes(),
                    ),
                    (
                        200,
                        "application/json".into(),
                        remote.to_string().into_bytes(),
                    ),
                    (
                        200,
                        "image/png".into(),
                        b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                    ),
                ]);
                let site_base = format!("{base}{directory}");
                let path = "index.php?m=file&f=read&fileID=708&t=png";
                let source = match representation {
                    "root" => format!("{directory}/{path}"),
                    "absolute" => format!("{site_base}/{path}"),
                    "relative" => path.into(),
                    _ => "{708.png}".into(),
                };
                let image = read_bug_image(&test_adapter(&site_base), &site_base, &bug, &source)
                    .await
                    .unwrap();
                assert!(image.starts_with("data:image/png;base64,"));
                let requests = handle.join().unwrap();
                assert!(requests.contains(&format!("GET {directory}/api.php/v1/bugs/419 ")));
                assert!(requests.contains(&format!("GET {directory}/api.php/v1/files/708 ")));
                assert!(!requests.contains(&format!("GET {directory}/index.php")));
                assert_eq!(
                    requests.to_lowercase().matches("token: test-token").count(),
                    3
                );
            }
        }
        assert!(file_id(
            "https://example.test/zentao",
            "/index.php?m=file&f=read&fileID=708&t=png"
        )
        .is_err());
        assert!(file_id(
            "https://example.test/zentao",
            "/other/index.php?m=file&f=read&fileID=708&t=png"
        )
        .is_err());
    }
    #[tokio::test]
    async fn download_uses_authenticated_v1_binary_endpoint() {
        let (base, server) = server(200, "image/png", b"\x89PNG\r\n\x1a\nfixture".to_vec());
        let image = read_image(&test_adapter(&base), "708").await.unwrap();
        assert!(image.starts_with("data:image/png;base64,"));
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /api.php/v1/files/708 "));
        assert!(request.to_lowercase().contains("token: test-token"));
    }
    #[tokio::test]
    async fn download_rejects_html_and_mime_mismatch() {
        for (mime, bytes) in [
            ("text/html", b"<html>login</html>".to_vec()),
            ("image/jpeg", b"\x89PNG\r\n\x1a\nfixture".to_vec()),
        ] {
            let (base, server) = server(200, mime, bytes);
            assert!(read_image(&test_adapter(&base), "708").await.is_err());
            server.join().unwrap();
        }
    }
    #[tokio::test]
    async fn upload_uses_imgfile_multipart_and_validates_returned_id() {
        let (base, server) = server(
            200,
            "application/json",
            br#"{"id":708,"url":"index.php?m=file&f=read&t=png&fileID=708"}"#.to_vec(),
        );
        assert_eq!(
            upload_image(
                &test_adapter(&base),
                &base,
                "example.png",
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                "image/png",
                "png"
            )
            .await
            .unwrap(),
            "708"
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /api.php/v1/files "));
        assert!(request.contains("name=\"imgFile\"; filename=\"example.png\""));
        assert!(request.to_lowercase().contains("content-type: image/png"));
        let (base, server) = self::server(
            200,
            "application/json",
            br#"{"id":708,"url":"https://evil.test/file-read-708.png"}"#.to_vec(),
        );
        assert!(upload_image(
            &test_adapter(&base),
            &base,
            "example.png",
            vec![1],
            "image/png",
            "png"
        )
        .await
        .is_err());
        server.join().unwrap();
    }
    #[test]
    fn source_resolution_is_fixed_to_file_ids() {
        let base = "https://example.test/zentao";
        for source in [
            "{708.png}",
            "index.php?m=file&f=read&t=png&fileID=708",
            "/zentao/file-read-708.png",
            "https://example.test/zentao/file-read-708.png",
        ] {
            assert_eq!(file_id(base, source).unwrap(), "708");
        }
        for source in [
            "https://evil.test/file-read-708.png",
            "/other/file-read-708.png",
            "javascript:alert(1)",
            "file:///tmp/file-read-708.png",
            "{0.png}",
            "{708.svg}",
            "index.php?m=file&f=delete&fileID=708",
            "../file-read-708.png",
        ] {
            assert!(file_id(base, source).is_err(), "{source}");
        }
    }
    #[test]
    fn only_description_images_authorize_download() {
        let base = "https://example.test/";
        assert!(referenced(base, r#"<p><img src="{708.png}" /></p>"#, "708"));
        assert!(referenced(
            base,
            r#"<img src="index.php?m=file&amp;f=read&amp;t=png&amp;fileID=708">"#,
            "708"
        ));
        assert!(!referenced(
            base,
            r#"<a href="{708.png}">image</a><img src="{709.png}" alt="{708.png}">"#,
            "708"
        ));
        assert!(image_type(b"<html>login</html>").is_none());
        assert!(image_type(b"<svg onload='alert(1)'>").is_none());
        assert_eq!(
            image_type(b"\x89PNG\r\n\x1a\nbody"),
            Some(("image/png", "png"))
        );
    }
}
