use super::*;
use std::io::{Read, Write};

fn fixture() -> (tempfile::TempDir, Store, Value) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_at(dir.path().join("workspace")).unwrap();
    let meeting = save(
        &store,
        &store.workspace_id,
        json!({
            "id":"workflow", "title":"研发周会", "segments":[], "speakerNames":{}
        }),
    )
    .unwrap();
    (dir, store, meeting)
}

fn job_for(store: &Store, meeting: &Value) -> Job {
    Job {
        generation: store.generation.clone(),
        revision: meeting["revision"].as_i64().unwrap(),
        model: json!({"id":"minutes","remoteModelId":"fixture"}),
        provider: json!({}),
        key: String::new(),
    }
}

fn document(actions: Value) -> Value {
    json!({"summary":"讨论产品发布与验收安排。", "decisions":[], "actionItems":actions,
        "mindMap":{"label":"研发周会"}})
}

fn generate(store: &Store, meeting: &Value, actions: Value) -> Value {
    commit_analysis(
        store,
        &store.workspace_id,
        text(meeting, "id"),
        &job_for(store, meeting),
        &document(actions).to_string(),
        None,
    )
    .unwrap()
}

#[test]
fn context_defaults_persist_and_mark_changed_analysis_stale_without_overwriting_system_fields() {
    let (dir, store, mut meeting) = fixture();
    assert_eq!(
        meeting["context"],
        json!({"background":"","participants":[],"notes":"","glossary":[],"summaryStyle":"standard"})
    );
    assert_eq!(meeting["markers"], json!([]));
    meeting["context"] = json!({"background":"产品验收", "participants":[" 张三 ","李四","张三"],
        "notes":"人工补充：负责人尚待确认", "glossary":["栖点","oMLX"],"summaryStyle":"detailed"});
    meeting["markers"] = json!([{"id":"forged","atMs":0,"text":"前端不得覆盖标记"}]);
    meeting["actionStates"] = json!({"forged":{"completed":true,"taskId":"another"}});
    meeting = save(&store, &store.workspace_id, meeting).unwrap();
    assert_eq!(meeting["context"]["participants"], json!(["张三", "李四"]));
    assert_eq!(meeting["markers"], json!([]));
    assert!(meeting.get("actionStates").is_none());
    let input = generation_context(&meeting).unwrap();
    assert!(input.contains("用户人工补充") && input.contains("detailed") && input.contains("oMLX"));
    assert_eq!(glossary_prompt(&meeting), "栖点、oMLX");
    meeting = generate(&store, &meeting, json!([]));
    let unchanged = save(&store, &store.workspace_id, meeting.clone()).unwrap();
    assert_eq!(unchanged["analysis"]["stale"], false);
    let mut changed = unchanged;
    changed["context"]["notes"] = json!("新增人工备注");
    meeting = save(&store, &store.workspace_id, changed).unwrap();
    assert_eq!(meeting["analysis"]["stale"], true);
    let mut legacy_client = meeting.clone();
    legacy_client.as_object_mut().unwrap().remove("context");
    let saved = save(&store, &store.workspace_id, legacy_client).unwrap();
    assert_eq!(saved["context"], meeting["context"]);
    drop(store);
    let reopened = Store::open_at(dir.path().join("workspace")).unwrap();
    let persisted = read(&reopened.conn.lock().unwrap(), "workflow").unwrap();
    assert_eq!(persisted["context"], saved["context"]);
}

#[test]
fn malformed_context_and_stale_revisions_do_not_mutate_saved_meeting() {
    let (_dir, store, meeting) = fixture();
    for invalid in [
        json!(null),
        json!({"notes":1}),
        json!({"notes":"字".repeat(20_001)}),
        json!({"background":"字".repeat(5001)}),
        json!({"summaryStyle":"invent"}),
        json!({"participants":vec!["人";101]}),
        json!({"participants":[""]}),
        json!({"glossary":vec!["词";201]}),
        json!({"glossary":["字".repeat(101)]}),
        json!({"taskId":"forged"}),
    ] {
        let mut bad = meeting.clone();
        bad["context"] = invalid;
        assert!(save(&store, &store.workspace_id, bad).is_err());
    }
    let mut bad_revision = meeting.clone();
    bad_revision["revision"] = json!(0);
    assert!(save(&store, &store.workspace_id, bad_revision).is_err());
    assert_eq!(
        read(&store.conn.lock().unwrap(), "workflow").unwrap(),
        meeting
    );
}

#[test]
fn summary_size_budget_includes_context_and_markers_without_silent_truncation() {
    let (_dir, _store, mut meeting) = fixture();
    meeting["segments"] =
        json!([{"id":"segment","startMs":0,"endMs":1000,"text":"字".repeat(98_000)}]);
    assert!(generation_input(&meeting).is_ok());
    meeting["context"]["notes"] = json!("笔记".repeat(2000));
    assert!(generation_input(&meeting).unwrap_err().contains("合计"));
    meeting["context"]["notes"] = json!("");
    meeting["markers"] = json!([{"id":"a","atMs":0,"text":"字".repeat(2000)}]);
    assert!(generation_input(&meeting).is_err());
    meeting["segments"] = json!([]);
    assert!(generation_input(&meeting).unwrap_err().contains("先录音"));
}

#[test]
fn old_records_get_deterministic_ids_and_model_cannot_forge_action_state_or_model_usage() {
    let (_dir, store, meeting) = fixture();
    let mut raw = document(json!([
        {"task":"核对 发布","owner":"张三","deadline":"下周三","id":"injected","completed":true,"taskId":"secret-task"},
        {"task":"核对 发布","owner":"张三","deadline":"下周三"}
    ]));
    raw["modelId"] = json!("untrusted-model");
    raw["usage"] = json!({"totalTokens":99999});
    raw["actionStates"] = json!({"injected":{"taskId":"secret-task"}});
    let saved = commit_analysis(
        &store,
        &store.workspace_id,
        "workflow",
        &job_for(&store, &meeting),
        &raw.to_string(),
        None,
    )
    .unwrap();
    assert_eq!(saved["analysis"]["modelId"], "minutes");
    assert!(saved["analysis"].get("usage").is_none());
    let first = saved["analysis"]["actionItems"][0].clone();
    let second = &saved["analysis"]["actionItems"][1];
    assert_ne!(first["id"], second["id"]);
    assert_ne!(first["id"], "injected");
    assert_eq!(first["completed"], false);
    assert!(first.get("taskId").is_none());
    let completed = complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        text(&first, "id"),
        true,
        saved["revision"].as_i64().unwrap(),
    )
    .unwrap();
    let regenerated = generate(
        &store,
        &completed,
        json!([
            {"task":"核对   发布","owner":"张三","deadline":"下周三","completed":false},
            {"task":"核对 发布","owner":"张三","deadline":"下周三"}
        ]),
    );
    assert_eq!(regenerated["analysis"]["actionItems"][0]["id"], first["id"]);
    assert_eq!(regenerated["analysis"]["actionItems"][0]["completed"], true);
    assert_eq!(
        regenerated["analysis"]["actionItems"][1]["completed"],
        false
    );
    // 旧版本没有任何系统状态，曾被模型输出的同名字段也不作为可信状态。
    let conn = store.conn.lock().unwrap();
    let legacy =
        json!({"id":"legacy","revision":1,"title":"旧会议","speakerNames":{},"analysis":raw});
    conn.execute(
        "INSERT INTO meetings(id,revision,data) VALUES('legacy',1,?1)",
        [legacy.to_string()],
    )
    .unwrap();
    let a = read(&conn, "legacy").unwrap();
    let b = read(&conn, "legacy").unwrap();
    assert_eq!(a["context"]["summaryStyle"], "standard");
    assert_eq!(
        a["analysis"]["actionItems"][0]["id"],
        b["analysis"]["actionItems"][0]["id"]
    );
    assert_eq!(a["analysis"]["actionItems"][0]["completed"], false);
    assert!(a["analysis"]["actionItems"][0].get("taskId").is_none());
}

#[test]
fn action_tasks_are_atomic_local_idempotent_and_preserve_full_escaped_context() {
    let (_dir, store, meeting) = fixture();
    store.save("task", json!({"id":"existing","title":"原有任务","source":"local","status":"todo","sortOrder":1024})).unwrap();
    let content = format!("<img src=x onerror=bad>{}", "完整事项".repeat(80));
    let mut meeting = generate(
        &store,
        &meeting,
        json!([{ "task":content,"owner":"张三<&>","deadline":"下周三" }]),
    );
    let action_id = text(&meeting["analysis"]["actionItems"][0], "id").to_owned();
    let original_revision = meeting["revision"].as_i64().unwrap();
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        original_revision - 1
    )
    .is_err());
    let result = action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        original_revision,
    )
    .unwrap();
    let task = &result["task"];
    assert_eq!(task["source"], "local");
    assert_eq!(task["status"], "todo");
    assert_eq!(task["sortOrder"], 2048);
    assert_eq!(task["revision"], 1);
    assert!(text(task, "title").chars().count() <= 200);
    for key in [
        "projectId",
        "remoteId",
        "connectionId",
        "remoteType",
        "schedule",
        "dueDate",
    ] {
        assert!(task.get(key).is_none_or(Value::is_null));
    }
    let notes = text(task, "notes");
    assert!(notes.contains("&lt;img") && !notes.contains("<img"));
    assert!(
        notes.contains("张三&lt;&amp;&gt;")
            && notes.contains("下周三")
            && notes.contains("研发周会")
    );
    assert!(notes.contains(&"完整事项".repeat(80)));
    assert_eq!(
        result["meeting"]["analysis"]["actionItems"][0]["taskId"],
        task["id"]
    );
    let repeated = action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        original_revision,
    )
    .unwrap();
    assert_eq!(repeated, result);
    {
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM entity_history WHERE entity_type='tasks' AND entity_id=?1",
                [text(task, "id")],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    meeting = result["meeting"].clone();
    assert!(complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        true,
        meeting["revision"].as_i64().unwrap()
    )
    .unwrap_err()
    .contains("我的任务"));
    let regenerated = generate(
        &store,
        &meeting,
        json!([{ "task":content,"owner":"张三<&>","deadline":"下周三","taskId":"forged" }]),
    );
    assert_eq!(
        regenerated["analysis"]["actionItems"][0]["taskId"],
        task["id"]
    );
    store.delete("task", text(task, "id"), Some(1)).unwrap();
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        regenerated["revision"].as_i64().unwrap()
    )
    .unwrap_err()
    .contains("被删除"));
    assert_eq!(
        store.snapshot().unwrap()["tasks"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn failure_after_task_insert_rolls_back_task_history_and_meeting_link() {
    let (_dir, store, meeting) = fixture();
    let meeting = generate(&store, &meeting, json!([{ "task":"核对发布" }]));
    let action_id = text(&meeting["analysis"]["actionItems"][0], "id");
    {
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("CREATE TRIGGER reject_action_link BEFORE UPDATE ON meetings BEGIN SELECT RAISE(ABORT,'模拟会议保存失败'); END;").unwrap();
    }
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        action_id,
        meeting["revision"].as_i64().unwrap()
    )
    .is_err());
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM entity_history WHERE entity_type='tasks'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(read(&conn, "workflow").unwrap(), meeting);
}

#[test]
fn local_task_helper_rejects_remote_links_existing_ids_and_rolls_back_transaction() {
    let (_dir, store, _) = fixture();
    {
        let mut conn = store.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        let task = json!({"id":"created","title":"本地行动项","source":"local","status":"todo"});
        Store::create_local_task_in_transaction(&tx, &store.workspace_id, task.clone()).unwrap();
        assert!(Store::create_local_task_in_transaction(&tx, &store.workspace_id, task).is_err());
        for key in ["projectId", "remoteId", "remoteType", "connectionId"] {
            let mut linked = json!({"id":"forbidden","title":"禁止远端关联","source":"local"});
            linked[key] = json!("remote");
            assert!(
                Store::create_local_task_in_transaction(&tx, &store.workspace_id, linked).is_err()
            );
        }
        assert!(Store::create_local_task_in_transaction(
            &tx,
            &store.workspace_id,
            json!({"id":"remote","title":"远端任务","source":"zentao"})
        )
        .is_err());
        // 调用方任一步失败即退出，未提交的本地任务及实体历史一起撤销。
    }
    let conn = store.conn.lock().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM entity_history WHERE entity_type='tasks'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn generated_context_validation_and_unsafe_sort_order_preserve_existing_data() {
    let (_dir, store, meeting) = fixture();
    for invalid in [
        json!({"background":"字".repeat(5001)}),
        json!({"participants":[1]}),
        json!({"participants":vec!["人";101]}),
    ] {
        let mut content = document(json!([]));
        for (key, value) in invalid.as_object().unwrap() {
            content[key] = value.clone();
        }
        assert!(commit_analysis(
            &store,
            &store.workspace_id,
            "workflow",
            &job_for(&store, &meeting),
            &content.to_string(),
            None
        )
        .is_err());
    }
    assert_eq!(
        read(&store.conn.lock().unwrap(), "workflow").unwrap(),
        meeting
    );
    let meeting = generate(&store, &meeting, json!([{"task":"保持本地任务"}]));
    store.save("task",json!({"id":"large","title":"极大历史排序值","source":"local","status":"todo","sortOrder":9_007_199_254_740_990i64})).unwrap();
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        text(&meeting["analysis"]["actionItems"][0], "id"),
        meeting["revision"].as_i64().unwrap()
    )
    .unwrap_err()
    .contains("排序"));
    assert_eq!(
        store.snapshot().unwrap()["tasks"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        read(&store.conn.lock().unwrap(), "workflow").unwrap(),
        meeting
    );
}

#[test]
fn stale_or_completed_actions_cannot_create_tasks_and_conflicts_keep_state() {
    let (_dir, store, meeting) = fixture();
    let mut meeting = generate(&store, &meeting, json!([{ "task":"核对发布" }]));
    let action_id = text(&meeting["analysis"]["actionItems"][0], "id").to_owned();
    let revision = meeting["revision"].as_i64().unwrap();
    assert!(complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        true,
        revision - 1
    )
    .is_err());
    assert!(complete_action(
        &store,
        "wrong-workspace",
        "workflow",
        &action_id,
        true,
        revision
    )
    .is_err());
    meeting = complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        true,
        revision,
    )
    .unwrap();
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        meeting["revision"].as_i64().unwrap()
    )
    .unwrap_err()
    .contains("已完成"));
    let same = complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        true,
        meeting["revision"].as_i64().unwrap(),
    )
    .unwrap();
    assert_eq!(same["revision"], meeting["revision"]);
    meeting["context"]["background"] = json!("新背景");
    meeting = save(&store, &store.workspace_id, meeting).unwrap();
    let revision = meeting["revision"].as_i64().unwrap();
    assert!(complete_action(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        false,
        revision
    )
    .unwrap_err()
    .contains("过期"));
    assert!(action_task(
        &store,
        &store.workspace_id,
        "workflow",
        &action_id,
        revision
    )
    .unwrap_err()
    .contains("过期"));
    assert_eq!(store.snapshot().unwrap()["tasks"], json!([]));
}

#[test]
fn markers_are_append_only_idempotent_bounded_and_survive_more_audio() {
    let (_dir, store, meeting) = fixture();
    let encoded = STANDARD.encode(wav(&vec![0; 32_000]));
    let meeting = append(
        &store,
        &store.workspace_id,
        text(&meeting, "id"),
        "first",
        &encoded,
    )
    .unwrap();
    let meeting = generate(&store, &meeting, json!([]));
    let marked = add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-1",
        1000,
        "这是重点，负责人待确认",
    )
    .unwrap();
    assert_eq!(marked["analysis"]["stale"], true);
    let repeated = add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-1",
        1000,
        "这是重点，负责人待确认",
    )
    .unwrap();
    assert_eq!(repeated, marked);
    assert!(add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-1",
        1000,
        "不同内容"
    )
    .is_err());
    assert!(add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-2",
        1001,
        "越界"
    )
    .is_err());
    assert!(add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-2",
        -1,
        "越界"
    )
    .is_err());
    assert!(add_marker(
        &store,
        &store.workspace_id,
        "workflow",
        "mark-2",
        0,
        &"字".repeat(2001)
    )
    .is_err());
    assert!(add_marker(&store, "wrong", "workflow", "mark-2", 0, "跨工作空间").is_err());
    let appended = append(&store, &store.workspace_id, "workflow", "second", &encoded).unwrap();
    assert_eq!(appended["markers"], marked["markers"]);
    assert_eq!(appended["durationMs"], 2000);
    let input = generation_context(&appended).unwrap();
    assert!(
        input.contains("用户重点标记") && input.contains("1000") && input.contains("负责人待确认")
    );
    let mut attempted_replace = appended;
    attempted_replace["markers"] = json!([]);
    assert_eq!(
        save(&store, &store.workspace_id, attempted_replace).unwrap()["markers"],
        marked["markers"]
    );
    assert!(meeting["markers"].as_array().unwrap().is_empty());
}

fn http_response(
    body: String,
    content_type: &str,
) -> (std::net::SocketAddr, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let content_type = content_type.to_owned();
    let task = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 8192];
        loop {
            let count = socket.read(&mut buf).unwrap();
            assert!(count > 0);
            request.extend(&buf[..count]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (address, task)
}

#[tokio::test]
async fn glossary_only_enters_supported_plain_http_transcription() {
    for diarized in [false, true] {
        let body = if diarized {
            json!({"segments":[{"start":0,"end":1,"speaker":"a","text":"栖点"}]})
        } else {
            json!({"text":"栖点"})
        };
        let (address, server) = http_response(body.to_string(), "application/json");
        let job = Job {
            generation: PathBuf::new(),
            revision: 1,
            key: String::new(),
            provider: json!({"baseUrl":format!("http://{address}/v1")}),
            model: json!({"remoteModelId":"fixture","speechMode":if diarized {"diarization"} else {"transcription"}}),
        };
        request_transcription(&job, vec![0; 32_000], 0, "", Some("栖点、oMLX"))
            .await
            .unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
        assert_eq!(request.contains("name=\"prompt\""), !diarized);
        assert_eq!(request.contains("栖点、oMLX"), !diarized);
    }
}

#[tokio::test]
async fn summary_request_contains_context_and_uses_transcript_as_fact_source() {
    let (_dir, store, mut meeting) = fixture();
    meeting["context"] = json!({"background":"季度验收","participants":["李四"],"notes":"人工备注","glossary":["栖点"],"summaryStyle":"brief"});
    meeting["markers"] = json!([{"id":"mark","atMs":0,"text":"待确认议题"}]);
    let input = generation_context(&meeting).unwrap() + "[0-1000] 未标注：发布负责人尚未确定。\n";
    let content = document(json!([])).to_string();
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":content}}]})
    );
    let (address, server) = http_response(body, "text/event-stream");
    let mut job = job_for(&store, &meeting);
    job.provider =
        json!({"baseUrl":format!("http://{address}/v1"),"protocol":"openai-completions"});
    generate_text(&job, &input).await.unwrap();
    let request = server.join().unwrap();
    let payload: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    let system = text(&payload["messages"][0], "content");
    let sent = text(&payload["messages"][1], "content");
    assert!(system.contains("实际转录为事实来源") && system.contains("不得输出行动项的 id"));
    for value in [
        "季度验收",
        "李四",
        "人工备注",
        "栖点",
        "brief",
        "待确认议题",
        "负责人尚未确定",
    ] {
        assert!(sent.contains(value));
    }
    assert!(sent.contains("用户人工补充"));
}
