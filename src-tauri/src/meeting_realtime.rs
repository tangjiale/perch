//! 本机流式识别只读取已写入 SQLite 的会议音频。推理在阻塞线程上运行。
use crate::{
    meeting_speech::{self, r2t2, MeetingSpeechModel},
    meetings,
    storage::Store,
    AppState,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sherpa_onnx::{OnlineRecognizer, OnlineStream};
use std::{
    borrow::Cow,
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

mod speakers;
use speakers::{speaker_key, Turn, Word, Worker};

const MODEL_ID: &str = "local";
const SAMPLE_RATE: i32 = 16_000;
const PCM_BYTES_PER_MS: usize = 32;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const TRANSCRIPT_TOO_LARGE: &str = "实时转录文字超过大小限制，已收到的文字已保留";
const MIN_SPEAKER_TURN_MS: i64 = 550;
const SHORT_FRAGMENT_GAP_MS: i64 = 850;
const MAX_LINE_GAP_MS: i64 = 1_600;
const MAX_LINE_DURATION_MS: i64 = 30_000;
const AUDIO_LOOKBACK_MS: i64 = 300;

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
    model: MeetingSpeechModel,
    generation: PathBuf,
    offset: i64,
    first_sequence: i64,
    tx: mpsc::Sender<Command>,
    result: Mutex<Transcript>,
    stopping: AtomicBool,
    cancelled: AtomicBool,
    finished: Notify,
}

struct Segment {
    start_ms: i64,
    end_ms: i64,
    text: String,
    words: Vec<Word>,
}

#[derive(Default)]
struct Transcript {
    segments: Vec<Segment>,
    partial: Option<Segment>,
    turns: Vec<Turn>,
    coarse_timing: bool,
    speaker_settled_ms: Option<i64>,
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

#[derive(Clone)]
struct Events {
    channel: Channel<Value>,
    workspace: String,
    meeting: String,
    session: String,
    offset: i64,
}

impl Events {
    fn event(&self, kind: &str) -> Value {
        json!({"sessionId":self.session,"workspaceId":self.workspace,"meetingId":self.meeting,"type":kind})
    }

    fn send(&self, kind: &str, message: &str) {
        let mut event = self.event(kind);
        event["message"] = json!(message);
        let _ = self.channel.send(event);
    }

    fn preview(&self, current: &Transcript) {
        let mut event = self.event("delta");
        let entries = current.live_entries(&self.session, self.offset);
        event["text"] = json!(entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"));
        event["lines"] = json!(entries);
        let _ = self.channel.send(event);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptEntry {
    id: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
    start_ms: i64,
    end_ms: i64,
}

struct Line<'a> {
    text: Cow<'a, str>,
    speaker: Option<u32>,
    start_ms: i64,
    end_ms: i64,
    last_word_ms: i64,
}

impl Segment {
    fn split(&self, turns: &[Turn], coarse_timing: bool) -> Vec<Line<'_>> {
        let mut lines = Vec::new();
        let mut speaker = None;
        if coarse_timing && self.words.is_empty() && self.end_ms > self.start_ms {
            // 有道仅返回增量文字。整段能确认由一人主导时才标记，不虚构逐字时间。
            let duration = self.end_ms - self.start_ms;
            let first = turns.partition_point(|turn| turn.end_ms <= self.start_ms);
            let mut overlapping = turns[first..]
                .iter()
                .take_while(|turn| turn.start_ms < self.end_ms)
                .filter(|turn| {
                    turn.end_ms.min(self.end_ms) - turn.start_ms.max(self.start_ms) >= 150
                });
            if let Some(turn) = overlapping.next() {
                let overlap = turn.end_ms.min(self.end_ms) - turn.start_ms.max(self.start_ms);
                if overlap >= MIN_SPEAKER_TURN_MS
                    && overlap * 5 >= duration * 4
                    && overlapping.all(|other| other.speaker == turn.speaker)
                {
                    speaker = Some(turn.speaker);
                }
            }
        }
        let mut byte = 0;
        let mut start_ms = self.words.first().map_or(self.start_ms, |word| word.ms);
        let mut last_word_ms = start_ms;
        for (index, word) in self.words.iter().enumerate() {
            let next = speakers::turn_at(turns, word.ms)
                .filter(|turn| turn.end_ms - turn.start_ms >= MIN_SPEAKER_TURN_MS)
                .map(|turn| turn.speaker);
            if index == 0 {
                speaker = next;
            } else if speaker != next && word.ms > start_ms {
                lines.push(Line {
                    text: Cow::Borrowed(&self.text[byte..word.byte]),
                    speaker,
                    start_ms,
                    end_ms: word.ms,
                    last_word_ms,
                });
                start_ms = word.ms;
                byte = word.byte;
                speaker = next;
            }
            last_word_ms = word.ms;
        }
        lines.push(Line {
            text: Cow::Borrowed(&self.text[byte..]),
            speaker,
            start_ms,
            end_ms: self.end_ms,
            last_word_ms: self.words.last().map_or(self.end_ms, |word| word.ms),
        });
        lines
    }
}

impl Transcript {
    fn lines(&self) -> Vec<Line<'_>> {
        let lines = self
            .segments
            .iter()
            .chain(self.partial.iter())
            .flat_map(|segment| segment.split(&self.turns, self.coarse_timing))
            .collect();
        combine_lines(lines)
    }

    fn live_entries(&self, session: &str, offset: i64) -> Vec<TranscriptEntry> {
        let mut lines = self.lines();
        if let Some(settled_ms) = self.speaker_settled_ms {
            // 还可能合并短词或修正说话人的部分只更新同一行，确认边界后再分段。
            let lookahead = MAX_LINE_GAP_MS.max(SHORT_FRAGMENT_GAP_MS + MIN_SPEAKER_TURN_MS);
            let cutoff = settled_ms - lookahead;
            let count = lines
                .iter()
                .take_while(|line| line.end_ms <= cutoff)
                .count();
            let mut draft = lines.drain(count..);
            let active = draft.next().map(|mut first| {
                if first.start_ms > cutoff {
                    first.speaker = None;
                }
                for line in draft.by_ref() {
                    first.append(line);
                }
                first
            });
            drop(draft);
            lines.extend(active);
        }
        Self::render(lines, session, offset)
    }

    fn entries(&self, session: &str, offset: i64) -> Vec<TranscriptEntry> {
        Self::render(self.lines(), session, offset)
    }

    fn render(lines: Vec<Line<'_>>, session: &str, offset: i64) -> Vec<TranscriptEntry> {
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| TranscriptEntry {
                id: format!("realtime:{session}:{index}"),
                text: line.text.trim().to_owned(),
                speaker: line.speaker.map(speaker_key),
                start_ms: offset + line.start_ms,
                end_ms: offset + line.end_ms,
            })
            .collect()
    }
}

impl Line<'_> {
    fn append(&mut self, line: Line<'_>) {
        let text = self.text.to_mut();
        if text
            .ends_with(|ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ',' | '?' | '!'))
            && line.text.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        {
            text.push(' ');
        }
        text.push_str(&line.text);
        self.end_ms = self.end_ms.max(line.end_ms);
        self.last_word_ms = self.last_word_ms.max(line.last_word_ms);
    }
}

fn combine_lines(mut lines: Vec<Line<'_>>) -> Vec<Line<'_>> {
    let speakers: Vec<_> = lines.iter().map(|line| line.speaker).collect();
    for index in 0..lines.len() {
        let line = &lines[index];
        if line.speaker.is_some()
            || line.last_word_ms - line.start_ms > SHORT_FRAGMENT_GAP_MS
            || line
                .text
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .take(3)
                .count()
                > 2
        {
            continue;
        }
        // 说话人边缘的一两个字可能没有落在模型区间内。邻近的两人不同则保留未标记。
        let before = index
            .checked_sub(1)
            .filter(|&before| line.start_ms - lines[before].last_word_ms <= SHORT_FRAGMENT_GAP_MS)
            .and_then(|before| speakers[before]);
        let after = lines
            .get(index + 1)
            .filter(|after| after.start_ms - line.last_word_ms <= SHORT_FRAGMENT_GAP_MS)
            .and_then(|_| speakers[index + 1]);
        if before.is_none() || after.is_none() || before == after {
            lines[index].speaker = before.or(after);
        }
    }
    let mut combined: Vec<Line<'_>> = Vec::new();
    for line in lines {
        let Some(previous) = combined.last_mut().filter(|previous| {
            previous.speaker == line.speaker
                && line.start_ms - previous.last_word_ms <= MAX_LINE_GAP_MS
                && line.end_ms - previous.start_ms <= MAX_LINE_DURATION_MS
        }) else {
            combined.push(line);
            continue;
        };
        previous.append(line);
    }
    combined
}

fn last_speaker_id(meeting: &Value) -> u32 {
    meeting["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|segment| segment["speaker"].as_str())
        .chain(
            meeting["speakerNames"]
                .as_object()
                .into_iter()
                .flat_map(|names| names.keys().map(String::as_str)),
        )
        .filter_map(|key| key.strip_prefix("local-speaker:")?.parse().ok())
        .max()
        .unwrap_or(0)
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
    let (tx, rx) = mpsc::channel(2);
    let session_key = key(&workspace_id, &id);
    let (session, path, speaker_setup) = {
        // 持有 Store 时登记，防止会议编辑、删除和工作空间迁移越过检查。
        let store = state.store.lock().map_err(|e| e.to_string())?;
        meetings::context(&store, &workspace_id, &store.generation)?;
        let (model, diarization) = meeting_speech::local_selection(&store)?;
        model.check_supported()?;
        let path = meeting_speech::model_path(&app, model)?;
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
            model,
            generation: store.generation.clone(),
            offset: value["durationMs"].as_i64().ok_or("录音时长无效")?,
            first_sequence: sequence,
            tx,
            result: Mutex::new(Transcript {
                coarse_timing: model == MeetingSpeechModel::R2t2,
                ..Transcript::default()
            }),
            stopping: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            finished: Notify::new(),
        });
        let mut all = sessions().lock().map_err(|_| "实时转录状态不可用")?;
        if !all.is_empty() {
            return Err("请先结束当前实时转录".into());
        }
        all.insert(session_key.clone(), session.clone());
        (
            session,
            path,
            diarization.then(|| {
                (
                    meeting_speech::speaker_model_path(&app),
                    last_speaker_id(&value),
                )
            }),
        )
    };
    let mut cleanup = SessionCleanup {
        key: session_key,
        session: session.clone(),
        armed: true,
    };
    let (ready, startup) = oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let events = Events {
            channel: on_event,
            workspace: workspace_id,
            meeting: id,
            session: session.id.clone(),
            offset: session.offset,
        };
        run(&path, session, rx, ready, events, speaker_setup);
    });
    tokio::time::timeout(Duration::from_secs(45), startup)
        .await
        .map_err(|_| "本机语音模型加载超时，请重试")?
        .map_err(|_| "本机语音模型启动已中断")??;
    cleanup.armed = false;
    Ok(())
}

enum RecognitionEngine {
    Sherpa {
        stream: OnlineStream,
        recognizer: Arc<OnlineRecognizer>,
    },
    R2t2(r2t2::Stream),
}

impl RecognitionEngine {
    fn start(model: MeetingSpeechModel, path: &Path) -> Result<Self, String> {
        if model == MeetingSpeechModel::R2t2 {
            return r2t2::Stream::start(path).map(Self::R2t2);
        }
        let recognizer = meeting_speech::acquire(model, path)?;
        Ok(Self::Sherpa {
            stream: recognizer.create_stream(),
            recognizer,
        })
    }
}

struct Decoder {
    engine: RecognitionEngine,
    elapsed_ms: i64,
    segment_start_ms: i64,
    last_audible_ms: Option<i64>,
    text_bytes: usize,
    last_partial: String,
    speakers: Option<Worker>,
}

impl Decoder {
    fn new(engine: RecognitionEngine) -> Self {
        Self {
            engine,
            elapsed_ms: 0,
            segment_start_ms: 0,
            last_audible_ms: None,
            text_bytes: 0,
            last_partial: String::new(),
            speakers: None,
        }
    }

    fn feed(&mut self, pcm: &[u8], session: &Session, events: &Events) -> Result<(), String> {
        if pcm.is_empty() || !pcm.len().is_multiple_of(PCM_BYTES_PER_MS) {
            return Err("实时音频采样格式无效".into());
        }
        for frame in pcm.chunks(100 * PCM_BYTES_PER_MS) {
            if session.cancelled.load(Ordering::Acquire) {
                return Err("实时转录已取消".into());
            }
            let samples: Vec<f32> = frame
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&sample| f32::from(i16::from_le_bytes(sample)) / 32768.0)
                .collect();
            let frame_start_ms = self.elapsed_ms;
            self.elapsed_ms += (frame.len() / PCM_BYTES_PER_MS) as i64;
            if has_audible_signal(&samples) {
                if self.last_audible_ms.is_none()
                    && matches!(self.engine, RecognitionEngine::R2t2(_))
                {
                    self.segment_start_ms = frame_start_ms;
                }
                self.last_audible_ms = Some(self.elapsed_ms);
            }
            if let Some(speakers) = &mut self.speakers {
                speakers.feed(&samples, self.elapsed_ms);
            }
            match &mut self.engine {
                RecognitionEngine::Sherpa { stream, .. } => {
                    stream.accept_waveform(SAMPLE_RATE, &samples);
                    self.decode_sherpa(session, false, events)?;
                }
                RecognitionEngine::R2t2(stream) => {
                    let result = stream.feed_pcm(frame);
                    let delta = stream.take_delta();
                    self.append_r2t2(&delta, false, session, events)?;
                    result?;
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, session: &Session, events: &Events) -> Result<(), String> {
        let result = match &mut self.engine {
            RecognitionEngine::Sherpa { stream, .. } => {
                // 末尾补静音满足模型向前看的窗口，时间戳仍以实际录音长度为准。
                stream.accept_waveform(SAMPLE_RATE, &[0.0; 8_000]);
                stream.input_finished();
                self.decode_sherpa(session, true, events)
            }
            RecognitionEngine::R2t2(stream) => {
                let result = stream.finish();
                let delta = stream.take_delta();
                self.append_r2t2(&delta, true, session, events)?;
                result
            }
        };
        if let Some(speakers) = self.speakers.take() {
            speakers.finish(self.elapsed_ms);
        }
        result
    }

    fn decode_sherpa(
        &mut self,
        session: &Session,
        finish: bool,
        events: &Events,
    ) -> Result<(), String> {
        let RecognitionEngine::Sherpa { stream, recognizer } = &self.engine else {
            return Err("本机实时转录引擎不匹配".into());
        };
        let mut decoded = false;
        while recognizer.is_ready(stream) {
            if session.cancelled.load(Ordering::Acquire) {
                return Err("实时转录已取消".into());
            }
            recognizer.decode(stream);
            decoded = true;
        }
        if !decoded && !finish {
            return Ok(());
        }
        let endpoint = finish || recognizer.is_endpoint(stream);
        let result = recognizer
            .get_result(stream)
            .ok_or("本机实时转录结果无法读取")?;
        let text = audible_transcript(&result.text, self.segment_start_ms, self.last_audible_ms);
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
                words: self.speakers.as_ref().map_or_else(Vec::new, |_| {
                    speakers::word_times(text, &result, self.elapsed_ms)
                }),
            });
            if endpoint {
                if let Some(segment) = current.partial.take() {
                    self.text_bytes += segment.text.len() + 1;
                    current.segments.push(segment);
                }
            }
            if changed {
                events.preview(&current);
            }
        }
        if endpoint {
            if !finish {
                recognizer.reset(stream);
            }
            self.segment_start_ms = self.elapsed_ms;
            self.last_partial.clear();
        } else if changed {
            self.last_partial = text.into();
        }
        Ok(())
    }

    fn append_r2t2(
        &mut self,
        delta: &str,
        finish: bool,
        session: &Session,
        events: &Events,
    ) -> Result<(), String> {
        if session.cancelled.load(Ordering::Acquire) {
            return Err("实时转录已取消".into());
        }
        let audible = audible_transcript(delta, self.segment_start_ms, self.last_audible_ms);
        let end_ms = self.last_audible_ms.unwrap_or(self.elapsed_ms);
        let endpoint = finish
            || self.elapsed_ms - end_ms >= 1_200
            || self.elapsed_ms - self.segment_start_ms >= MAX_LINE_DURATION_MS
            || audible.ends_with(['。', '！', '？', '!', '?']);
        if audible.is_empty() && !endpoint {
            return Ok(());
        }
        let mut current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
        if !audible.is_empty() {
            let segment = current.partial.get_or_insert_with(|| Segment {
                start_ms: self.segment_start_ms,
                end_ms: self.segment_start_ms,
                text: String::new(),
                words: Vec::new(),
            });
            if self.text_bytes + segment.text.len() + delta.len() > MAX_TEXT_BYTES {
                return Err(TRANSCRIPT_TOO_LARGE.into());
            }
            let delta = if segment.text.is_empty() {
                delta.trim_start()
            } else {
                delta
            };
            segment.text.push_str(delta);
            segment.end_ms = end_ms.max(segment.start_ms + 1).min(self.elapsed_ms);
        }
        if let Some(mut segment) = endpoint.then(|| current.partial.take()).flatten() {
            segment.text.truncate(segment.text.trim_end().len());
            self.text_bytes += segment.text.len() + 1;
            self.segment_start_ms = segment.end_ms;
            current.segments.push(segment);
        }
        if !audible.is_empty() {
            events.preview(&current);
        }
        Ok(())
    }
}

fn has_audible_signal(samples: &[f32]) -> bool {
    // 阈值仅排除近乎静音的底噪。突出的短音节保留给识别器判断。
    const MIN_PEAK: f32 = 0.004;
    const MIN_MEAN_SQUARE: f32 = 0.000_001;
    !samples.is_empty()
        && (samples.iter().any(|sample| sample.abs() >= MIN_PEAK)
            || samples.iter().map(|sample| sample * sample).sum::<f32>()
                >= MIN_MEAN_SQUARE * samples.len() as f32)
}

fn audible_transcript(text: &str, start_ms: i64, last_audible_ms: Option<i64>) -> &str {
    // 解码器可能在音频已送入后才报告端点，给边界附近的真实语音留一点余量。
    if last_audible_ms.is_some_and(|end| end >= start_ms.saturating_sub(AUDIO_LOOKBACK_MS)) {
        text.trim()
    } else {
        ""
    }
}

fn run(
    path: &Path,
    session: Arc<Session>,
    mut rx: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), String>>,
    events: Events,
    speaker_setup: Option<(Result<PathBuf, String>, u32)>,
) {
    let _model_guard = match meeting_speech::model_read_lock(path) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let engine = match RecognitionEngine::start(session.model, path) {
        Ok(engine) => engine,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut decoder = Decoder::new(engine);
    if session.cancelled.load(Ordering::Acquire) || ready.send(Ok(())).is_err() {
        return;
    }
    if let Some((path, last_id)) = speaker_setup {
        match path {
            Ok(path) => {
                if let Ok(mut current) = session.result.lock() {
                    current.speaker_settled_ms = Some(0);
                }
                decoder.speakers = Some(Worker::start(
                    path,
                    session.clone(),
                    events.clone(),
                    last_id,
                ))
            }
            Err(error) => events.send("speakerError", &error),
        }
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
                    let result = decoder.feed(&pcm, &session, &events);
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
                Command::Stop => return decoder.finish(&session, &events),
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
            events.send("error", error);
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
    let entries = current.entries(&session.id, session.offset);
    for entry in &entries {
        meetings::insert_segment(&tx, id, &json!(entry), Some(&entry.id))?;
    }
    drop(current);
    if !entries.is_empty() {
        value["transcriptionModelId"] = if session.model == MeetingSpeechModel::Transcription {
            json!(MODEL_ID)
        } else {
            json!(session.model)
        };
        meetings::stale_analysis(&mut value);
        meetings::write_metadata(&tx, &mut value, revision)?;
    }
    let saved = meetings::read(&tx, id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}

#[cfg(test)]
mod window_tests;
