use crate::integrations::*;
#[tauri::command]
pub fn knowledge_export(
    state: State<AppState>,
    document_id: String,
    destination: String,
) -> Result<(), String> {
    use std::io::Write;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let doc = get(&store.snapshot()?, "documents", &document_id)?;
    let hash = text(&doc, "sha256");
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("原件标识无效".into());
    }
    let bytes =
        std::fs::read(store.generation.join("objects").join(hash)).map_err(|_| "原件不存在")?;
    if format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err("原件校验失败".into());
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| "无法导出，请选择尚不存在的文件名")?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|_| "导出失败".to_string())
}
pub(crate) fn model_fingerprint(provider: &Value, model: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([
                provider["id"],
                provider["revision"],
                provider["baseUrl"],
                model
            ]))
            .map_err(|e| e.to_string())?
        )
    ))
}
#[tauri::command]
pub async fn knowledge_retry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: String,
) -> Result<(), String> {
    let (mut doc, path) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let doc = get(&store.snapshot()?, "documents", &document_id)?;
        let hash = text(&doc, "sha256");
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("原件指纹无效".into());
        }
        let path = store.generation.join("objects").join(hash);
        (doc, path)
    };
    let extension = Path::new(text(&doc, "name"))
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let result = crate::parser::extract(&crate::parser::resources(&app)?, &path, &extension).await;
    match result {
        Ok(content) => {
            doc["status"] = json!(if content.trim().is_empty() {
                "needs_ocr"
            } else {
                "parsed"
            });
            doc["text"] = json!(content);
            doc["error"] = json!("");
        }
        Err(error) => {
            doc["status"] = json!("failed");
            doc["error"] = json!(error);
        }
    }
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .save("document", doc)?;
    Ok(())
}
#[tauri::command]
pub async fn knowledge_import(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    knowledge_id: String,
    paths: Vec<String>,
) -> Result<usize, String> {
    let parser_root = crate::parser::resources(&app)?;
    let mut count = 0;
    for path in paths {
        let path = Path::new(&path);
        let metadata = std::fs::metadata(path).map_err(|_| "文件不可访问")?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 30 * 1024 * 1024 {
            return Err("文件必须非空且不超过 30 MB".into());
        }
        let name = path
            .file_name()
            .ok_or("文件名无效")?
            .to_string_lossy()
            .to_string();
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if ![
            "txt", "md", "pdf", "doc", "docx", "ppt", "pptx", "xls", "xlsx",
        ]
        .contains(&ext.as_str())
        {
            return Err("不支持该文件格式".into());
        }
        let bytes = std::fs::read(path).map_err(|_| "读取文件失败")?;
        if bytes.is_empty() || bytes.len() > 30 * 1024 * 1024 {
            return Err("文件读取期间大小发生变化".into());
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let (destination, relative) = {
            let store = state.store.lock().map_err(|e| e.to_string())?;
            get(&store.snapshot()?, "knowledge", &knowledge_id)?;
            let relative = format!("objects/{hash}");
            (store.generation.join(&relative), relative)
        };
        if !destination.exists() {
            use std::io::Write;
            let temporary = destination.with_extension(uuid::Uuid::now_v7().to_string());
            let mut file = std::fs::File::create(&temporary).map_err(|_| "保存原件失败")?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "保存原件失败")?;
            std::fs::rename(temporary, &destination).map_err(|_| "保存原件失败")?;
        }
        let (extracted, status, error) =
            match crate::parser::extract(&parser_root, &destination, &ext).await {
                Ok(text) if text.trim().is_empty() => (
                    text,
                    if ext == "pdf" { "needs_ocr" } else { "failed" },
                    "未提取到正文，扫描件需要 OCR".to_string(),
                ),
                Ok(text) => (text, "parsed", String::new()),
                Err(error) => (String::new(), "failed", error),
            };
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if store.generation.join(&relative) != destination {
            return Err("导入期间存储目录发生变化，请重新导入".into());
        }
        store.save("document",json!({"id":uuid::Uuid::now_v7().to_string(),"knowledgeId":knowledge_id,"name":name,"size":bytes.len(),"assetPath":relative,"sha256":hash,"text":extracted,"status":status,"error":error,"chunks":0}))?;
        count += 1;
    }
    Ok(count)
}
#[tauri::command]
pub async fn knowledge_index(
    state: State<'_, AppState>,
    knowledge_id: String,
) -> Result<String, String> {
    let (snapshot, workspace) = {
        let s = state.store.lock().map_err(|e| e.to_string())?;
        (s.snapshot()?, s.workspace_id.clone())
    };
    let kb = get(&snapshot, "knowledge", &knowledge_id)?;
    let model = get(&snapshot, "models", text(&kb, "modelId"))?;
    if text(&model, "capability") != "embedding" {
        return Err("知识库必须绑定向量模型".into());
    }
    let provider = get(&snapshot, "providers", text(&model, "providerId"))?;
    let key = match credential(&workspace, text(&provider, "id"))?.get_password() {
        Ok(key) => key,
        Err(keyring::Error::NoEntry) => String::new(),
        Err(_) => return Err("无法读取模型凭据，请检查系统钥匙串".into()),
    };
    let generation = uuid::Uuid::now_v7().to_string();
    let fingerprint = model_fingerprint(&provider, &model)?;
    let mut dimension = None;
    let mut total = 0;
    let mut prepared = vec![];
    for doc in snapshot["documents"]
        .as_array()
        .ok_or("文档列表无效")?
        .iter()
        .filter(|d| text(d, "knowledgeId") == knowledge_id)
    {
        if !matches!(text(doc, "status"), "parsed" | "ready") {
            return Err(format!("{} 需要先完成解析", text(doc, "name")));
        }
        let text_content = text(doc, "text");
        if text_content.trim().is_empty() {
            return Err(format!("{} 尚无可索引正文", text(doc, "name")));
        }
        let chars: Vec<char> = text_content.chars().collect();
        let mut chunks = vec![];
        let mut start = 0;
        while start < chars.len() {
            let end = (start + 1000).min(chars.len());
            chunks.push(chars[start..end].iter().collect::<String>());
            if end == chars.len() {
                break;
            }
            start = end - 100;
        }
        if total + chunks.len() > 10_000 {
            return Err("单个知识库最多 10,000 个分块，请拆分知识库".into());
        }
        let mut records = vec![];
        for batch in chunks.chunks(16) {
            let vectors = embed(&provider, &model, &key, batch.to_vec()).await?;
            for (t, v) in batch.iter().zip(vectors) {
                if dimension.is_some_and(|d| d != v.len()) {
                    return Err("向量维度不一致，原索引保持不变".into());
                }
                dimension = Some(v.len());
                records.push(json!({"id":uuid::Uuid::now_v7().to_string(),"text":t,"vector":v,"documentRevision":doc["revision"],"sourceSha256":doc["sha256"]}));
            }
        }
        let mut next = doc.clone();
        next["indexedChunks"] = json!(records);
        next["chunks"] = json!(chunks.len());
        next["embeddingModelId"] = kb["modelId"].clone();
        next["generationId"] = json!(generation);
        next["modelFingerprint"] = json!(fingerprint);
        next["status"] = json!("ready");
        prepared.push(next);
        total += chunks.len();
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    if store.workspace_id != workspace {
        return Err("工作空间已切换，请重新索引".into());
    }
    let current = store.snapshot()?;
    if get(&current, "knowledge", &knowledge_id)?["revision"] != kb["revision"]
        || get(&current, "models", text(&model, "id"))? != model
        || get(&current, "providers", text(&provider, "id"))? != provider
    {
        return Err("索引期间配置发生变化，请重试".into());
    }
    for doc in &prepared {
        if get(&current, "documents", text(doc, "id"))?["revision"] != doc["revision"] {
            return Err("索引期间原文发生变化，请重试".into());
        }
    }
    let mut kb = kb;
    kb["activeGenerationId"] = json!(generation);
    kb["modelFingerprint"] = json!(fingerprint);
    kb["embeddingDimension"] = json!(dimension);
    kb["status"] = json!("ready");
    // 全部分块与活动代次一次提交，失败时旧索引仍可检索。
    let expected = vec![
        (
            "model",
            text(&model, "id").to_owned(),
            model["revision"].as_i64().ok_or("模型修订无效")?,
        ),
        (
            "provider",
            text(&provider, "id").to_owned(),
            provider["revision"].as_i64().ok_or("供应商修订无效")?,
        ),
    ];
    store.save_batch(
        prepared
            .into_iter()
            .map(|d| ("document", d))
            .chain(std::iter::once(("knowledge", kb)))
            .collect(),
        expected,
        false,
    )?;
    Ok(format!("索引完成，{total} 个分块"))
}
