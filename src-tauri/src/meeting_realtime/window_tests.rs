use super::*;
use tokio::net::TcpListener;

fn test_job(address: std::net::SocketAddr) -> meetings::Job {
    meetings::Job {
        generation: PathBuf::from("window-test-generation"),
        revision: 1,
        model: json!({"remoteModelId":"test-whisper"}),
        provider: json!({"baseUrl":format!("http://{address}/v1")}),
        key: "window-test-key".into(),
    }
}

async fn next_message<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> Message
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(4), ws.next())
        .await
        .expect("mock websocket timed out")
        .expect("mock websocket closed")
        .expect("mock websocket failed")
}

async fn push(tx: &mpsc::Sender<Command>, chunk: &str, pcm: Vec<u8>) {
    let (ack, wait) = oneshot::channel();
    tx.send(Command::Audio(chunk.into(), pcm, ack))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), wait)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn wait_text(result: &Mutex<Transcript>, expected: &str) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if result.lock().unwrap().text == expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completed window did not reach the live transcript");
}

#[tokio::test]
async fn short_windows_flush_revised_text_before_user_stop_without_audio_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let job = test_job(listener.local_addr().unwrap());
    let expected: Vec<u8> = (0..(WINDOW_PCM_BYTES * 2 + 16_000))
        .map(|index| (index % 251) as u8)
        .collect();
    let expected_server = expected.clone();
    let server = tokio::spawn(async move {
        let mut all_audio = Vec::new();
        for (index, final_text) in ["第一窗口完整。", "第二窗口完整。", "尾部。"]
            .into_iter()
            .enumerate()
        {
            let (stream, _) = tokio::time::timeout(Duration::from_secs(4), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let start = next_message(&mut ws).await;
            assert_eq!(
                serde_json::from_str::<Value>(start.to_text().unwrap()).unwrap()["type"],
                "start"
            );
            ws.send(Message::text(json!({"type":"ready"}).to_string()))
                .await
                .unwrap();
            let mut window_bytes = 0;
            loop {
                match next_message(&mut ws).await {
                    Message::Binary(bytes) => {
                        window_bytes += bytes.len();
                        all_audio.extend_from_slice(&bytes);
                        if index == 0 && window_bytes == bytes.len() {
                            ws.send(Message::text(
                                json!({"type":"transcript.delta","delta":"第"}).to_string(),
                            ))
                            .await
                            .unwrap();
                        }
                    }
                    Message::Text(raw) => {
                        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap()["type"], "stop");
                        assert_eq!(
                            window_bytes,
                            if index < 2 { WINDOW_PCM_BYTES } else { 16_000 }
                        );
                        ws.send(Message::text(
                            json!({"type":"transcript.done","text":final_text}).to_string(),
                        ))
                        .await
                        .unwrap();
                        break;
                    }
                    other => panic!("unexpected client frame: {other:?}"),
                }
            }
            // oMLX releases its per-model session on connection close.
            let _ = next_message(&mut ws).await;
            let _ = ws.close(None).await;
        }
        assert_eq!(all_audio, expected_server);
    });
    let socket = connect(&job).await.unwrap();
    let (tx, rx) = mpsc::channel(6);
    let result = Arc::new(Mutex::new(Transcript::default()));
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = events.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(raw) = body {
            captured
                .lock()
                .unwrap()
                .push(serde_json::from_str(&raw).unwrap());
        }
        Ok(())
    });
    let task = tokio::spawn(run(
        RecognitionStream::new(socket, job),
        rx,
        result.clone(),
        channel,
        "workspace".into(),
        "meeting".into(),
        "original-session".into(),
    ));
    // Saved chunks deliberately cross the four-second boundaries.
    push(&tx, "c1", expected[..96_000].to_vec()).await;
    push(&tx, "c2", expected[96_000..160_000].to_vec()).await;
    wait_text(&result, "第一窗口完整。").await;
    assert!(!result.lock().unwrap().ended);
    push(&tx, "c3", expected[160_000..256_000].to_vec()).await;
    wait_text(&result, "第一窗口完整。\n第二窗口完整。").await;
    assert!(!result.lock().unwrap().ended);
    push(&tx, "tail", expected[256_000..].to_vec()).await;
    tx.send(Command::Stop).await.unwrap();
    tokio::time::timeout(Duration::from_secs(4), task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), server)
        .await
        .unwrap()
        .unwrap();
    let current = result.lock().unwrap();
    assert!(current.ended);
    assert!(current.error.is_none(), "{:?}", current.error);
    assert_eq!(current.text, "第一窗口完整。\n第二窗口完整。\n尾部。");
    assert_eq!(current.duration, 8500);
    assert_eq!(current.sent, ["c1", "c2", "c3", "tail"]);
    let events = events.lock().unwrap();
    assert!(events
        .iter()
        .any(|e| e["text"] == "第一窗口完整。\n第二窗口完整。"));
    assert!(events.iter().all(|e| e["sessionId"] == "original-session"
        && e["workspaceId"] == "workspace"
        && e["meetingId"] == "meeting"));
}

#[tokio::test]
async fn next_window_auth_failure_keeps_committed_text_and_never_replays_old_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let job = test_job(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        next_message(&mut ws).await;
        ws.send(Message::text("{\"type\":\"ready\"}"))
            .await
            .unwrap();
        assert_eq!(
            next_message(&mut ws).await.into_data().len(),
            WINDOW_PCM_BYTES
        );
        assert_eq!(
            serde_json::from_str::<Value>(next_message(&mut ws).await.to_text().unwrap()).unwrap()
                ["type"],
            "stop"
        );
        ws.send(Message::text(
            "{\"type\":\"transcript.done\",\"text\":\"已确认正文\"}",
        ))
        .await
        .unwrap();
        let _ = next_message(&mut ws).await;
        let _ = ws.close(None).await;
        let (stream, _) = tokio::time::timeout(Duration::from_secs(4), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let start = next_message(&mut ws).await;
        assert_eq!(
            serde_json::from_str::<Value>(start.to_text().unwrap()).unwrap()["type"],
            "start"
        );
        ws.send(Message::text(
            "{\"type\":\"error\",\"detail\":\"401 invalid api key\"}",
        ))
        .await
        .unwrap();
        // No audio is allowed before ready, including a replay of the prior window.
        let after_rejection = tokio::time::timeout(Duration::from_secs(4), ws.next())
            .await
            .expect("rejected connection was not released");
        assert!(!matches!(after_rejection, Some(Ok(Message::Binary(_)))));
    });
    let socket = connect(&job).await.unwrap();
    let (tx, rx) = mpsc::channel(6);
    let result = Arc::new(Mutex::new(Transcript::default()));
    let task = tokio::spawn(run(
        RecognitionStream::new(socket, job),
        rx,
        result.clone(),
        Channel::new(|_| Ok(())),
        "w".into(),
        "m".into(),
        "s".into(),
    ));
    push(&tx, "first", vec![1; WINDOW_PCM_BYTES]).await;
    wait_text(&result, "已确认正文").await;
    let (ack, wait) = oneshot::channel();
    tx.send(Command::Audio("next".into(), vec![2; 16_000], ack))
        .await
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(4), wait)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    tokio::time::timeout(Duration::from_secs(4), task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), server)
        .await
        .unwrap()
        .unwrap();
    let current = result.lock().unwrap();
    assert_eq!(current.text, "已确认正文");
    assert_eq!(current.sent, ["first"]);
    assert_eq!(current.duration, 4000);
    assert!(current.ended);
    assert!(current.error.as_ref().unwrap().contains("认证失败"));
}

#[test]
fn revised_window_snapshot_preserves_prefix_and_rejects_oversize_without_mutation() {
    let result = Mutex::new(Transcript {
        text: "第一段。\n未校正".into(),
        committed_text: "第一段。".into(),
        window_text: "未校正".into(),
        ..Transcript::default()
    });
    let revised = accept_transcript(
        &result,
        &json!({"type":"transcript.done","text":"第二段修正。"}),
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(revised, ("第一段。\n第二段修正。".into(), true));
    let oversized = "字".repeat(100_000);
    assert!(accept_transcript(
        &result,
        &json!({"type":"transcript.done","text":oversized}),
        true,
    )
    .is_err());
    let current = result.lock().unwrap();
    assert_eq!(current.text, "第一段。\n第二段修正。");
    assert_eq!(current.committed_text, "第一段。");
    assert_eq!(current.window_text, "第二段修正。");
}

#[tokio::test]
async fn next_window_only_retries_explicit_busy_before_sending_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let job = test_job(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for attempt in 0..3 {
            let (stream, _) = tokio::time::timeout(Duration::from_secs(4), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let start = next_message(&mut ws).await;
            assert_eq!(
                serde_json::from_str::<Value>(start.to_text().unwrap()).unwrap()["type"],
                "start"
            );
            let frame = if attempt < 2 {
                json!({"type":"error","detail":"realtime transcription session is already active"})
            } else {
                json!({"type":"ready"})
            };
            ws.send(Message::text(frame.to_string())).await.unwrap();
            let after = tokio::time::timeout(Duration::from_secs(4), ws.next())
                .await
                .unwrap();
            assert!(!matches!(after, Some(Ok(Message::Binary(_)))));
        }
    });
    let mut socket = tokio::time::timeout(Duration::from_secs(4), next_window(&job))
        .await
        .unwrap()
        .unwrap();
    socket.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(4), server)
        .await
        .unwrap()
        .unwrap();
}
