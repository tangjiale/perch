use super::*;

fn session(generation: PathBuf, tx: mpsc::Sender<Command>) -> Session {
    Session {
        model: MeetingSpeechModel::Transcription,
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
            "INSERT INTO meeting_audio_chunks (meeting_id,chunk_id,sequence,duration_ms,pcm) VALUES ('meeting','audio',0,5000,zeroblob(160000))",
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
    let live_entries = {
        let mut current = session.result.lock().unwrap();
        current.segments.push(Segment {
            start_ms: 0,
            end_ms: 950,
            text: "第一段".into(),
            words: Vec::new(),
        });
        current.partial = Some(Segment {
            start_ms: 950,
            end_ms: 4_000,
            text: "甲，乙，甲。".into(),
            words: vec![
                Word { byte: 0, ms: 950 },
                Word { byte: 6, ms: 1900 },
                Word { byte: 12, ms: 2900 },
            ],
        });
        current.turns = vec![
            Turn {
                start_ms: 950,
                end_ms: 1900,
                speaker: 1,
            },
            Turn {
                start_ms: 1900,
                end_ms: 2900,
                speaker: 2,
            },
            Turn {
                start_ms: 2900,
                end_ms: 4000,
                speaker: 1,
            },
        ];
        json!(current.entries(&session.id, session.offset))
    };
    let meeting = commit_result(&store, &store.workspace_id, "meeting", &session).unwrap();
    let segments = meeting["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 5);
    assert_eq!(segments[0]["text"], "已校对");
    assert_eq!(segments[1]["startMs"], 1_000);
    assert!(segments[1].get("speaker").is_none());
    assert_eq!(segments[2]["startMs"], 1950);
    assert_eq!(segments[4]["endMs"], 5_000);
    assert_eq!(&segments[1..], live_entries.as_array().unwrap().as_slice());
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
fn combines_short_edges_and_pauses_but_keeps_confirmed_speaker_changes() {
    let current = Transcript {
        segments: vec![
            Segment {
                start_ms: 0,
                end_ms: 2400,
                text: "嗯，今天要做什么呢？".into(),
                words: vec![
                    Word { byte: 0, ms: 100 },
                    Word { byte: 6, ms: 350 },
                    Word { byte: 12, ms: 700 },
                    Word { byte: 21, ms: 1400 },
                    Word { byte: 24, ms: 1750 },
                ],
            },
            Segment {
                start_ms: 2400,
                end_ms: 4400,
                text: "先过一下计划。".into(),
                words: vec![Word { byte: 0, ms: 2660 }],
            },
            Segment {
                start_ms: 4400,
                end_ms: 5700,
                text: "我来补充。".into(),
                words: vec![Word { byte: 0, ms: 4500 }],
            },
        ],
        partial: Some(Segment {
            start_ms: 5700,
            end_ms: 7200,
            text: "接着说下一项。".into(),
            words: vec![Word { byte: 0, ms: 6000 }],
        }),
        turns: vec![
            Turn {
                start_ms: 300,
                end_ms: 1600,
                speaker: 1,
            },
            Turn {
                start_ms: 1680,
                end_ms: 1850,
                speaker: 3,
            },
            Turn {
                start_ms: 2550,
                end_ms: 4250,
                speaker: 1,
            },
            Turn {
                start_ms: 4400,
                end_ms: 5500,
                speaker: 2,
            },
            Turn {
                start_ms: 5900,
                end_ms: 7100,
                speaker: 1,
            },
        ],
        ..Transcript::default()
    };
    let entries = current.entries("live", 8000);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].text, "嗯，今天要做什么呢？先过一下计划。");
    assert_eq!(entries[0].start_ms, 8100);
    assert_eq!(entries[0].end_ms, 12400);
    assert_eq!(entries[2].id, "realtime:live:2");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.speaker.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("local-speaker:1"),
            Some("local-speaker:2"),
            Some("local-speaker:1")
        ]
    );
}

#[test]
fn leaves_a_short_word_between_different_speakers_unassigned() {
    let part = |text, speaker, at| Line {
        text: Cow::Borrowed(text),
        speaker,
        start_ms: at,
        end_ms: at + 200,
        last_word_ms: at + 100,
    };
    let lines = combine_lines(vec![
        part("先看第一项。", Some(1), 0),
        part("好。", None, 300),
        part("我来介绍。", Some(2), 600),
    ]);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1].speaker, None);
    assert_eq!(lines[1].text, "好。");
}

#[test]
fn live_rows_only_split_after_the_speaker_boundary_settles() {
    let mut transcript = Transcript {
        partial: Some(Segment {
            start_ms: 0,
            end_ms: 5_000,
            text: "先说第一项，我来补充一下。".into(),
            words: vec![
                Word { byte: 0, ms: 100 },
                Word {
                    byte: 18,
                    ms: 1_900,
                },
                Word {
                    byte: 30,
                    ms: 3_800,
                },
            ],
        }),
        turns: vec![
            Turn {
                start_ms: 0,
                end_ms: 1_800,
                speaker: 1,
            },
            Turn {
                start_ms: 1_800,
                end_ms: 5_000,
                speaker: 2,
            },
        ],
        speaker_settled_ms: Some(1_000),
        ..Transcript::default()
    };
    let initial = transcript.live_entries("test", 0);
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].text, "先说第一项，我来补充一下。");
    assert_eq!(initial[0].speaker, None);

    transcript.turns[1].speaker = 1;
    assert_eq!(json!(transcript.live_entries("test", 0)), json!(initial));
    transcript.turns[1].speaker = 2;
    transcript.speaker_settled_ms = Some(3_500);
    let settled = transcript.live_entries("test", 0);
    assert_eq!(settled.len(), 2);
    assert_eq!(settled[0].id, initial[0].id);
    assert_eq!(settled[0].text, "先说第一项，");
    assert_eq!(settled[1].speaker.as_deref(), Some("local-speaker:2"));

    transcript.turns[1].end_ms = 3_600;
    transcript.turns.push(Turn {
        start_ms: 3_600,
        end_ms: 5_000,
        speaker: 3,
    });
    assert_eq!(transcript.entries("test", 0).len(), 3);
    assert_eq!(json!(transcript.live_entries("test", 0)), json!(settled));
}

#[test]
fn coarse_timing_labels_only_an_unambiguous_speaker_interval() {
    let segment = Segment {
        start_ms: 1_000,
        end_ms: 3_000,
        text: "今天讨论项目进度。".into(),
        words: Vec::new(),
    };
    let speaker_one = Turn {
        start_ms: 1_100,
        end_ms: 2_900,
        speaker: 1,
    };
    assert_eq!(segment.split(&[speaker_one], true)[0].speaker, Some(1));
    assert_eq!(segment.split(&[speaker_one], false)[0].speaker, None);
    let alternating = [
        Turn {
            end_ms: 2_000,
            ..speaker_one
        },
        Turn {
            start_ms: 2_000,
            end_ms: 3_000,
            speaker: 2,
        },
    ];
    assert_eq!(segment.split(&alternating, true)[0].speaker, None);
}

#[test]
#[ignore = "需要有道模型和运行包，使用公开音频按录音节奏回放"]
fn r2t2_emits_live_lines_and_preserves_text_when_stopped() {
    let path = PathBuf::from(std::env::var_os("PERCH_TEST_R2T2_DIR").unwrap());
    let _guard = meeting_speech::model_read_lock(&path).unwrap();
    let wave =
        sherpa_onnx::Wave::read(&std::env::var("PERCH_TEST_PUBLIC_SPEECH_WAV").unwrap()).unwrap();
    assert_eq!(wave.sample_rate(), SAMPLE_RATE);
    let (tx, _) = mpsc::channel(2);
    let mut session = session(PathBuf::new(), tx);
    session.model = MeetingSpeechModel::R2t2;
    session.result.lock().unwrap().coarse_timing = true;
    let events = Events {
        channel: Channel::new(|_| Ok(())),
        workspace: "fixture".into(),
        meeting: "meeting".into(),
        session: session.id.clone(),
        offset: session.offset,
    };
    let engine = RecognitionEngine::start(session.model, &path).unwrap();
    let mut decoder = Decoder::new(engine);
    let mut first_preview = None;
    for chunk in wave.samples()[..(8 * SAMPLE_RATE as usize)].chunks(2_560) {
        let pcm = chunk
            .iter()
            .flat_map(|sample| ((sample * 32_768.0) as i16).to_le_bytes())
            .collect::<Vec<_>>();
        decoder.feed(&pcm, &session, &events).unwrap();
        std::thread::sleep(Duration::from_millis(160));
        let current = session.result.lock().unwrap();
        if current.partial.is_some() || !current.segments.is_empty() {
            first_preview.get_or_insert(decoder.elapsed_ms);
        }
    }
    let first_preview = first_preview.expect("结束录音前应产生转录行");
    let snapshot = |current: &Transcript| {
        current
            .entries(&session.id, session.offset)
            .into_iter()
            .flat_map(|entry| {
                entry
                    .text
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect::<Vec<_>>()
            })
            .collect::<String>()
    };
    let live = snapshot(&session.result.lock().unwrap());
    decoder.finish(&session, &events).unwrap();
    let final_text = snapshot(&session.result.lock().unwrap());
    assert!(!live.is_empty() && final_text.starts_with(&live));
    eprintln!("有道公开中文回放首字对应已送入音频 {first_preview} ms");
}

#[test]
fn isolated_near_silence_cannot_create_transcript_rows() {
    assert!(!has_audible_signal(&[]));
    assert!(!has_audible_signal(&[0.0; 1600]));
    assert!(!has_audible_signal(&[0.0002; 1600]));
    assert_eq!(audible_transcript("明", 30_000, None), "");
    assert_eq!(audible_transcript("come in.", 41_000, Some(20_000)), "");

    let mut short_syllable = [0.0; 1600];
    short_syllable[780..820].fill(0.012);
    assert!(has_audible_signal(&short_syllable));
    assert!(has_audible_signal(&[0.002; 1600]));
    assert_eq!(audible_transcript("好。", 41_000, Some(41_090)), "好。");
    assert_eq!(audible_transcript("嗯。", 41_000, Some(40_830)), "嗯。");
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
