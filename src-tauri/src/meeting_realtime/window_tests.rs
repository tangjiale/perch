use super::*;

fn session(generation: PathBuf, tx: mpsc::Sender<Command>) -> Session {
    Session {
        id: "run".into(),
        generation,
        offset: 1_000,
        first_sequence: 1,
        tx,
        result: Mutex::new(Transcript::default()),
        stopping: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        finished: Notify::new(),
    }
}

#[test]
fn final_and_partial_segments_append_with_recording_offset() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open_at(temp.path().join("workspace")).unwrap();
    let (tx, _) = mpsc::channel(2);
    let session = session(store.generation.clone(), tx);
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO meetings VALUES (?1, 1, ?2)",
            params![
                "meeting",
                json!({"id":"meeting","title":"会议","revision":1}).to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meeting_audio_chunks (meeting_id,chunk_id,sequence,duration_ms,pcm) VALUES ('meeting','audio',0,3000,zeroblob(96000))",
            [],
        ).unwrap();
        meetings::insert_segment(
            &conn,
            "meeting",
            &json!({"id":"previous","startMs":0,"endMs":1000,"text":"已校对"}),
            None,
        )
        .unwrap();
    }
    {
        let mut current = session.result.lock().unwrap();
        current.segments.push(Segment {
            start_ms: 0,
            end_ms: 950,
            text: "第一段".into(),
            words: Vec::new(),
        });
        current.partial = Some(Segment {
            start_ms: 950,
            end_ms: 2_000,
            text: "甲，乙，甲。".into(),
            words: vec![
                Word { byte: 0, ms: 950 },
                Word { byte: 6, ms: 1300 },
                Word { byte: 12, ms: 1600 },
            ],
        });
        current.turns = vec![
            Turn {
                start_ms: 950,
                end_ms: 1300,
                speaker: 1,
            },
            Turn {
                start_ms: 1300,
                end_ms: 1600,
                speaker: 2,
            },
            Turn {
                start_ms: 1600,
                end_ms: 2000,
                speaker: 1,
            },
        ];
    }
    let meeting = commit_result(&store, &store.workspace_id, "meeting", &session).unwrap();
    let segments = meeting["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 5);
    assert_eq!(segments[0]["text"], "已校对");
    assert_eq!(segments[1]["startMs"], 1_000);
    assert!(segments[1].get("speaker").is_none());
    assert_eq!(segments[2]["startMs"], 1950);
    assert_eq!(segments[4]["endMs"], 3_000);
    assert_eq!(
        segments[2..]
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<String>(),
        "甲，乙，甲。"
    );
    assert_eq!(
        segments[2..]
            .iter()
            .filter_map(|part| part["speaker"].as_str())
            .collect::<Vec<_>>(),
        ["local-speaker:1", "local-speaker:2", "local-speaker:1"]
    );
    assert_eq!(meeting["transcriptionModelId"], MODEL_ID);
    assert_eq!(meeting["revision"], 2);
    assert!(commit_result(&store, "other-workspace", "meeting", &session).is_err());
}

#[test]
fn cancelling_a_session_releases_its_workspace_guard() {
    let session_key = key("cleanup-test-workspace", "meeting");
    let (tx, mut rx) = mpsc::channel(2);
    let session = Arc::new(session(PathBuf::from("cleanup-test-generation"), tx));
    sessions()
        .lock()
        .unwrap()
        .insert(session_key.clone(), session.clone());
    assert!(ensure_idle(&session_key.0, &session_key.1).is_err());
    drop(SessionCleanup {
        key: session_key.clone(),
        session: session.clone(),
        armed: true,
    });
    assert!(session.cancelled.load(Ordering::Acquire));
    assert!(matches!(rx.try_recv(), Ok(Command::Stop)));
    assert!(ensure_idle(&session_key.0, &session_key.1).is_ok());
}
