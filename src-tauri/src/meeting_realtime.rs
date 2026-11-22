//! 本机流式识别只读取已写入 SQLite 的会议音频。推理在阻塞线程上运行。
use crate::{meeting_speech, meetings, storage::Store, AppState};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sherpa_onnx::{OnlineRecognizer, OnlineStream};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, State};
use tokio::sync::{mpsc, oneshot, Notify};

const MODEL_ID: &str = "local";
const SAMPLE_RATE: i32 = 16_000;
const PCM_BYTES_PER_MS: usize = 32;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const TRANSCRIPT_TOO_LARGE: &str = "实时转录文字超过大小限制，已收到的文字已保留";

type Sessions = HashMap<(String, String), Arc<Session>>;
static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();

fn sessions() -> &'static Mutex<Sessions> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(workspace: &str, id: &str) -> (String, String) {
    (workspace.into(), id.into())
}

struct Session {
    id: String,
    generation: PathBuf,
    offset: i64,
    first_sequence: i64,
    tx: mpsc::Sender<Command>,
    result: Mutex<Transcript>,
    stopping: AtomicBool,
    cancelled: AtomicBool,
    finished: Notify,
}

#[derive(Clone)]
struct Segment {
    start_ms: i64,
    end_ms: i64,
    text: String,
}

#[derive(Default)]
struct Transcript {
    segments: Vec<Segment>,
    partial: Option<Segment>,
    processed: i64,
    error: Option<String>,
    ended: bool,
}

enum Command {
    Audio(i64, Vec<u8>, oneshot::Sender<Result<(), String>>),
    Stop,
}

pub(crate) fn ensure_all_idle() -> Result<(), String> {
    if sessions()
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .is_empty()
    {
        Ok(())
    } else {
        Err("会议正在实时录音，请先停止录音再恢复或迁移工作空间".into())
    }
}

pub(crate) fn ensure_idle(workspace: &str, id: &str) -> Result<(), String> {
    if sessions()
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .contains_key(&key(workspace, id))
    {
        return Err("会议正在实时转录，请先停止录音再修改或重新转录".into());
    }
    Ok(())
}

fn get(workspace: &str, id: &str, session_id: &str) -> Result<Arc<Session>, String> {
    let all = sessions().lock().map_err(|_| "实时转录状态不可用")?;
    let session = all.get(&key(workspace, id)).ok_or("实时转录会话已结束")?;
    if session.id != session_id {
        return Err("实时转录会话已变化".into());
    }
    Ok(session.clone())
}

// 启动失败、收尾出错或调用被取消时均释放会话。阻塞推理会在当前解码步结束后退出。
struct SessionCleanup {
    key: (String, String),
    session: Arc<Session>,
    armed: bool,
}
impl Drop for SessionCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.session.cancelled.store(true, Ordering::Release);
        let _ = self.session.tx.try_send(Command::Stop);
        if let Ok(mut all) = sessions().lock() {
            if all
                .get(&self.key)
                .is_some_and(|item| Arc::ptr_eq(item, &self.session))
            {
                all.remove(&self.key);
            }
        }
    }
}

fn emit(
    channel: &Channel<Value>,
    workspace: &str,
    meeting: &str,
    session: &str,
    kind: &str,
    content: &str,
) {
    let mut event =
        json!({"sessionId":session,"workspaceId":workspace,"meetingId":meeting,"type":kind});
    event[if kind == "delta" { "text" } else { "message" }] = json!(content);
    let _ = channel.send(event);
}

#[tauri::command]
pub async fn meeting_realtime_start(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    model_id: String,
    session_id: String,
    on_event: Channel<Value>,
) -> Result<(), String> {
    if model_id != MODEL_ID {
        return Err("请选择本机实时转录".into());
    }
    if session_id.is_empty() || session_id.len() > 120 || session_id.chars().any(char::is_control) {
        return Err("实时会话标识无效".into());
    }
    let path = meeting_speech::model_path(&app)?;
    let (tx, rx) = mpsc::channel(2);
    let session_key = key(&workspace_id, &id);
    let session = {
        // 持有 Store 时登记，防止会议编辑、删除和工作空间迁移越过检查。
        let store = state.store.lock().map_err(|e| e.to_string())?;
        meetings::context(&store, &workspace_id, &store.generation)?;
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let value = meetings::read(&conn, &id)?;
        let sequence = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence)+1,0) FROM meeting_audio_chunks WHERE meeting_id=?1",
                [&id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let session = Arc::new(Session {
            id: session_id,
            generation: store.generation.clone(),
            offset: value["durationMs"].as_i64().ok_or("录音时长无效")?,
            first_sequence: sequence,
            tx,
            result: Mutex::new(Transcript::default()),
            stopping: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            finished: Notify::new(),
        });
        let mut all = sessions().lock().map_err(|_| "实时转录状态不可用")?;
        if !all.is_empty() {
            return Err("请先结束当前实时转录".into());
        }
        all.insert(session_key.clone(), session.clone());
        session
    };
    let mut cleanup = SessionCleanup {
        key: session_key,
        session: session.clone(),
        armed: true,
    };
    let (ready, startup) = oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let progress = |kind: &str, content: &str| {
            emit(&on_event, &workspace_id, &id, &session.id, kind, content)
        };
        run(&path, &session, rx, ready, progress);
    });
    tokio::time::timeout(Duration::from_secs(45), startup)
        .await
        .map_err(|_| "本机语音模型加载超时，请重试")?
        .map_err(|_| "本机语音模型启动已中断")??;
    cleanup.armed = false;
    Ok(())
}

struct Decoder {
    stream: OnlineStream,
    recognizer: Arc<OnlineRecognizer>,
    elapsed_ms: i64,
    segment_start_ms: i64,
    text_bytes: usize,
    last_partial: String,
}

impl Decoder {
    fn new(recognizer: Arc<OnlineRecognizer>) -> Self {
        Self {
            stream: recognizer.create_stream(),
            recognizer,
            elapsed_ms: 0,
            segment_start_ms: 0,
            text_bytes: 0,
            last_partial: String::new(),
        }
    }

    fn feed(
        &mut self,
        pcm: &[u8],
        session: &Session,
        progress: &impl Fn(&str, &str),
    ) -> Result<(), String> {
        if pcm.is_empty() || !pcm.len().is_multiple_of(PCM_BYTES_PER_MS) {
            return Err("实时音频采样格式无效".into());
        }
        for frame in pcm.chunks(100 * PCM_BYTES_PER_MS) {
            if session.cancelled.load(Ordering::Acquire) {
                return Err("实时转录已取消".into());
            }
            let samples: Vec<f32> = frame
                .chunks_exact(2)
                .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0)
                .collect();
            self.stream.accept_waveform(SAMPLE_RATE, &samples);
            self.elapsed_ms += (frame.len() / PCM_BYTES_PER_MS) as i64;
            self.decode(session, false, progress)?;
        }
        Ok(())
    }

    fn finish(&mut self, session: &Session, progress: &impl Fn(&str, &str)) -> Result<(), String> {
        // 末尾补静音满足模型向前看的窗口，时间戳仍以实际录音长度为准。
        self.stream.accept_waveform(SAMPLE_RATE, &[0.0; 8_000]);
        self.stream.input_finished();
        self.decode(session, true, progress)
    }

    fn decode(
        &mut self,
        session: &Session,
        finish: bool,
        progress: &impl Fn(&str, &str),
    ) -> Result<(), String> {
        let mut decoded = false;
        while self.recognizer.is_ready(&self.stream) {
            if session.cancelled.load(Ordering::Acquire) {
                return Err("实时转录已取消".into());
            }
            self.recognizer.decode(&self.stream);
            decoded = true;
        }
        if !decoded && !finish {
            return Ok(());
        }
        let endpoint = finish || self.recognizer.is_endpoint(&self.stream);
        let result = self
            .recognizer
            .get_result(&self.stream)
            .ok_or("本机实时转录结果无法读取")?;
        let text = result.text.trim();
        if self.text_bytes + text.len() > MAX_TEXT_BYTES {
            return Err(TRANSCRIPT_TOO_LARGE.into());
        }
        let changed = self.last_partial != text;
        if changed || endpoint {
            let mut current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
            current.partial = (!text.is_empty()).then(|| Segment {
                start_ms: self.segment_start_ms,
                end_ms: self.elapsed_ms,
                text: text.into(),
            });
            if endpoint {
                if let Some(segment) = current.partial.take() {
                    self.text_bytes += segment.text.len() + 1;
                    current.segments.push(segment);
                }
            }
            if changed {
                let preview = current
                    .segments
                    .iter()
                    .chain(current.partial.iter())
                    .map(|segment| segment.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                drop(current);
                progress("delta", &preview);
            }
        }
        if endpoint {
            if !finish {
                self.recognizer.reset(&self.stream);
            }
            self.segment_start_ms = self.elapsed_ms;
            self.last_partial.clear();
        } else if changed {
            self.last_partial = text.into();
        }
        Ok(())
    }
}

fn run(
    path: &Path,
    session: &Session,
    mut rx: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), String>>,
    progress: impl Fn(&str, &str),
) {
    let _model_guard = match meeting_speech::model_read_lock(path) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let recognizer = match meeting_speech::acquire(path) {
        Ok(recognizer) => recognizer,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut decoder = Decoder::new(recognizer);
    if session.cancelled.load(Ordering::Acquire) || ready.send(Ok(())).is_err() {
        return;
    }
    let result = (|| {
        let mut expected = session.first_sequence;
        while let Some(command) = rx.blocking_recv() {
            if session.cancelled.load(Ordering::Acquire) {
                break;
            }
            match command {
                Command::Audio(sequence, pcm, ack) => {
                    if sequence != expected {
                        let _ = ack.send(Err("实时音频片段顺序无效，请停止录音后重新转录".into()));
                        continue;
                    }
                    let result = decoder.feed(&pcm, session, &progress);
                    if result.is_ok() {
                        expected += 1;
                        session
                            .result
                            .lock()
                            .map_err(|_| "实时转录状态不可用")?
                            .processed += 1;
                    }
                    let _ = ack.send(result.clone());
                    result?;
                }
                Command::Stop => return decoder.finish(session, &progress),
            }
        }
        Ok(())
    })();
    if let Ok(mut current) = session.result.lock() {
        current.ended = true;
        current.error = result.err();
    }
    session.finished.notify_one();
    if let Ok(current) = session.result.lock() {
        if let Some(error) = &current.error {
            progress("error", error);
        }
    }
}

#[tauri::command]
pub async fn meeting_realtime_push(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    session_id: String,
    chunk_id: String,
) -> Result<(), String> {
    let session = get(&workspace_id, &id, &session_id)?;
    if session.stopping.load(Ordering::Acquire) {
        return Err("实时转录正在收尾".into());
    }
    let (sequence, pcm) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        meetings::context(&store, &workspace_id, &session.generation)?;
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let (sequence, pcm): (i64, Vec<u8>) = conn
            .query_row(
                "SELECT sequence,pcm FROM meeting_audio_chunks WHERE meeting_id=?1 AND chunk_id=?2",
                params![id, chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("请先保存录音片段")?;
        let current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
        if current.ended {
            return Err(current.error.clone().unwrap_or("实时转录会话已结束".into()));
        }
        if sequence != session.first_sequence + current.processed {
            return Err("实时音频片段顺序无效，请停止录音后重新转录".into());
        }
        if pcm.len() > 640_000 {
            return Err("实时音频片段过大".into());
        }
        (sequence, pcm)
    };
    let (ack, wait) = oneshot::channel();
    session
        .tx
        .try_send(Command::Audio(sequence, pcm, ack))
        .map_err(|_| "实时转录处理积压，已保存音频可重新转录")?;
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .map_err(|_| "本机实时转录响应超时")?
        .map_err(|_| "本机实时转录已中断")?
}

#[tauri::command]
pub async fn meeting_realtime_stop(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    session_id: String,
) -> Result<Value, String> {
    let session = get(&workspace_id, &id, &session_id)?;
    if session.stopping.swap(true, Ordering::AcqRel) {
        return Err("实时转录正在收尾".into());
    }
    let _cleanup = SessionCleanup {
        key: key(&workspace_id, &id),
        session: session.clone(),
        armed: true,
    };
    let ended = session
        .result
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .ended;
    let timed_out = !ended
        && tokio::time::timeout(Duration::from_secs(20), async {
            let _ = session.tx.send(Command::Stop).await;
            session.finished.notified().await;
        })
        .await
        .is_err();
    if timed_out {
        session.cancelled.store(true, Ordering::Release);
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let saved = commit_result(&store, &workspace_id, &id, &session)?;
    let current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
    if timed_out {
        return Err("本机转录收尾超时，录音和已收到文字已保留".into());
    }
    if let Some(error) = &current.error {
        return Err(error.clone());
    }
    Ok(saved)
}

fn commit_result(
    store: &Store,
    workspace_id: &str,
    id: &str,
    session: &Session,
) -> Result<Value, String> {
    meetings::context(store, workspace_id, &session.generation)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut value = meetings::read(&tx, id)?;
    let revision = value["revision"].as_i64().ok_or("会议版本无效")?;
    let current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
    let segments = current.segments.iter().chain(current.partial.iter());
    let mut changed = false;
    for (index, segment) in segments.enumerate() {
        let segment_id = format!("realtime:{}:{index}", session.id);
        let entry = json!({
            "id":segment_id,
            "startMs":session.offset + segment.start_ms,
            "endMs":session.offset + segment.end_ms,
            "text":segment.text,
        });
        meetings::insert_segment(&tx, id, &entry, Some(&segment_id))?;
        changed = true;
    }
    drop(current);
    if changed {
        value["transcriptionModelId"] = json!(MODEL_ID);
        meetings::stale_analysis(&mut value);
        meetings::write_metadata(&tx, &mut value, revision)?;
    }
    let saved = meetings::read(&tx, id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}

#[cfg(test)]
mod window_tests;
