//! 禅道 22.0 api/v1/entries/{file,files}.php：Token 下载及 imgFile 上传。
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

struct Context {
    task: Value,
    connection: Value,
    project: String,
    workspace: String,
    generation: std::path::PathBuf,
}
fn context(state: &AppState, task_id: &str) -> Result<Context, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let snapshot = store.snapshot()?;
    let task = get(&snapshot, "tasks", task_id)?;
    if task["source"] != "zentao" || task["remoteType"] != "execution" {
        return Err("请先保存并同步禅道执行，再上传或读取图片".into());
    }
    let connection = get(&snapshot, "connections", text(&task, "connectionId"))?;
    if connection["enabled"] == false {
        return Err("禅道连接已停用".into());
    }
    let project = get(&snapshot, "projects", text(&task, "projectId"))?;
    if project["source"] != "zentao" || project["connectionId"] != task["connectionId"] {
        return Err("执行的禅道项目关联无效".into());
    }
    Ok(Context {
        task,
        connection,
        project: remote_id(&project["remoteId"])?,
        workspace: store.workspace_id.clone(),
        generation: store.generation.clone(),
    })
}
fn unchanged(state: &AppState, ctx: &Context) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != ctx.workspace || store.generation != ctx.generation {
        return Err("数据目录已切换，请重新打开任务".into());
    }
    let snapshot = store.snapshot()?;
    let connection = get(&snapshot, "connections", text(&ctx.connection, "id"))?;
    let task = get(&snapshot, "tasks", text(&ctx.task, "id"))?;
    if connection["revision"] != ctx.connection["revision"]
        || task["revision"] != ctx.task["revision"]
    {
        return Err("连接或任务已变化，请重新打开任务".into());
    }
    Ok(())
}
fn adapter<'a>(state: &'a AppState, ctx: &Context) -> Result<Adapter<'a>, String> {
    let token = credential(&ctx.workspace, text(&ctx.connection, "id"))?
        .get_password()
        .map_err(|_| "请先保存禅道令牌")?;
    Ok(
        Adapter::personal(&ctx.connection, token)?.with_auth(crate::zentao_auth::AutoAuth::new(
            state,
            &ctx.workspace,
            &ctx.generation,
            &ctx.connection,
        )?),
    )
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
    fn server(status: u16, mime: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mime = mime.to_owned();
        let handle = std::thread::spawn(move || {
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
            String::from_utf8_lossy(&request).into_owned()
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
