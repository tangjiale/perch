//! oMLX 0.7 实时协议。密钥仅存在原生握手，音频必须先落库再发送。
use crate::{data::text, integrations, meetings, AppState};
use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{ipc::Channel, State};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

const WAIT: Duration = Duration::from_secs(90);
// oMLX 0.7 的 Whisper 修订结果可能一直等到 stop 才输出；限制识别窗口而不停止采集。
const WINDOW_PCM_BYTES: usize = 16_000 * 2 * 4;
const MODEL_BUSY: &str = "该语音模型仍有实时会话，请先结束其他录音";
type Sessions = HashMap<String, Arc<Session>>;
static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
fn sessions() -> &'static Mutex<Sessions> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn key(workspace: &str, id: &str) -> String {
    format!("{workspace}\0{id}")
}
struct Session {
    id: String,
    generation: PathBuf,
    model_id: String,
    offset: i64,
    first_sequence: i64,
    tx: mpsc::Sender<Command>,
    result: Arc<Mutex<Transcript>>,
    task: Mutex<Option<tokio::task::AbortHandle>>,
}
#[derive(Default)]
struct Transcript {
    text: String,
    committed_text: String,
    window_text: String,
    error: Option<String>,
    ended: bool,
    sent: Vec<String>,
    duration: i64,
}
enum Command {
    Audio(String, Vec<u8>, oneshot::Sender<Result<(), String>>),
    Stop,
}

pub(crate) fn ensure_all_idle() -> Result<(), String> {
    if !sessions()
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .is_empty()
    {
        return Err("会议正在实时录音，请先停止录音再恢复或迁移工作空间".into());
    }
    Ok(())
}

/// 即使落库失败、工作空间失效或调用被取消，也必须释放任务与会议编辑保护。
struct StopCleanup<'a> {
    workspace: &'a str,
    meeting: &'a str,
    session: &'a Session,
}
impl Drop for StopCleanup<'_> {
    fn drop(&mut self) {
        cancel(
            self.session,
            "实时转录收尾已取消，录音与已收到文字预览已保留",
        );
        let _ = remove_session(
            self.workspace,
            self.meeting,
            &self.session.id,
            &self.session.generation,
        );
    }
}
fn cancel(session: &Session, message: &str) {
    if let Ok(task) = session.task.lock() {
        if let Some(task) = task.as_ref() {
            task.abort();
        }
    }
    if let Ok(mut current) = session.result.lock() {
        if !current.ended {
            current.error = Some(message.into());
            current.ended = true;
        }
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
fn remove_session(
    workspace: &str,
    id: &str,
    session_id: &str,
    generation: &PathBuf,
) -> Result<(), String> {
    let mut all = sessions().lock().map_err(|_| "实时转录状态不可用")?;
    if all
        .get(&key(workspace, id))
        .is_some_and(|s| s.id == session_id && &s.generation == generation)
    {
        all.remove(&key(workspace, id));
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
fn ws_url(base: &str) -> Result<String, String> {
    let original = url::Url::parse(base).map_err(|_| "实时语音地址无效")?;
    if original.query().is_some() || original.fragment().is_some() {
        return Err("实时语音地址不能包含密钥参数或片段".into());
    }
    let mut url = integrations::endpoint(base, "audio/transcriptions/realtime")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("实时语音地址不能包含账号、密钥参数或片段".into());
    }
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err("实时语音地址必须使用 HTTP 或 HTTPS".into()),
    };
    url.set_scheme(scheme).map_err(|_| "实时语音地址无效")?;
    Ok(url.into())
}
fn service_error(detail: &str) -> String {
    if detail.to_ascii_lowercase().contains("api key") || detail.contains("401") {
        "语音服务认证失败：请在所选语音模型所属供应商中填写与 oMLX 设置一致的 API Key；录音已保留"
            .into()
    } else if detail.contains("403") {
        "语音服务拒绝访问（HTTP 403），请检查供应商 API Key 权限和访问限制；录音已保留".into()
    } else {
        // 供应商错误可能含密钥或内部地址，不回传原文。
        "实时语音服务失败，请确认 oMLX 0.7.0 及模型支持实时语音转录；录音已保留".into()
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
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    model_id: String,
    session_id: String,
    on_event: Channel<Value>,
) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 120 || session_id.chars().any(char::is_control) {
        return Err("实时会话标识无效".into());
    }
    // 持有 Store 时登记会话，防止保存/删除在登记前越过检查。
    let (job, rx, result) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let job = meetings::job(&store, &workspace_id, &id, &model_id, true)?;
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let value = meetings::read(&conn, &id)?;
        let sequence: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence)+1,0) FROM meeting_audio_chunks WHERE meeting_id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let offset = value["durationMs"].as_i64().ok_or("录音时长无效")?;
        let (tx, rx) = mpsc::channel(6);
        let result = Arc::new(Mutex::new(Transcript::default()));
        let session = Arc::new(Session {
            id: session_id.clone(),
            generation: job.generation.clone(),
            model_id: model_id.clone(),
            offset,
            first_sequence: sequence,
            tx: tx.clone(),
            result: result.clone(),
            task: Mutex::new(None),
        });
        let mut all = sessions().lock().map_err(|_| "实时转录状态不可用")?;
        if all
            .get(&key(&workspace_id, &id))
            .is_some_and(|old| old.generation != job.generation)
        {
            all.remove(&key(&workspace_id, &id));
        }
        if all.contains_key(&key(&workspace_id, &id)) {
            return Err("会议已有实时转录会话".into());
        }
        if all.len() >= 4 {
            return Err("实时转录会话过多，请先停止已有录音".into());
        }
        all.insert(key(&workspace_id, &id), session);
        (job, rx, result)
    };
    let socket = match tokio::time::timeout(WAIT, connect(&job))
        .await
        .unwrap_or_else(|_| Err("实时语音连接超时，请检查 oMLX 模型是否已加载".into()))
    {
        Ok(socket) => socket,
        Err(error) => {
            remove_session(&workspace_id, &id, &session_id, &job.generation)?;
            return Err(error);
        }
    };
    // 连接期间可能切换或恢复工作空间，禁止迟到会话进入录音流程。
    {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        if let Err(error) = meetings::context(&store, &workspace_id, &job.generation) {
            remove_session(&workspace_id, &id, &session_id, &job.generation)?;
            return Err(error);
        }
    }
    let session = get(&workspace_id, &id, &session_id)?;
    let task = tokio::spawn(run(
        RecognitionStream::new(socket, job),
        rx,
        result,
        on_event,
        workspace_id,
        id,
        session_id,
    ));
    *session.task.lock().map_err(|_| "实时转录状态不可用")? = Some(task.abort_handle());
    Ok(())
}

async fn connect(job: &meetings::Job) -> Result<Socket, String> {
    let url = ws_url(text(&job.provider, "baseUrl"))?;
    let mut request = url.into_client_request().map_err(|_| "实时语音地址无效")?;
    if !job.key.is_empty() {
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", job.key)
                .parse()
                .map_err(|_| "供应商 API Key 格式无效")?,
        );
    }
    let (mut socket, _) = connect_async(request).await.map_err(|e| match e {
        tokio_tungstenite::tungstenite::Error::Http(response)
            if response.status().as_u16() == 401 || response.status().as_u16() == 403 =>
        {
            service_error(&response.status().as_u16().to_string())
        }
        _ => "实时语音连接失败，请检查供应商地址；录音已保留".into(),
    })?;
    // oMLX 0.7.0 在首帧验证 api_key；Authorization header 不能替代此字段。
    socket
        .send(Message::text(
            json!({"type":"start","model":text(&job.model,"remoteModelId"),"api_key":job.key})
                .to_string(),
        ))
        .await
        .map_err(|_| "实时语音握手发送失败")?;
    loop {
        let message = socket
            .next()
            .await
            .ok_or("实时语音握手期间连接中断")?
            .map_err(|_| "实时语音握手失败")?;
        if let Message::Text(raw) = message {
            let value: Value = serde_json::from_str(&raw).map_err(|_| "实时语音握手响应无效")?;
            match text(&value, "type") {
                "ready" => return Ok(socket),
                "error" => {
                    if text(&value, "detail")
                        .contains("realtime transcription session is already active")
                    {
                        return Err(MODEL_BUSY.into());
                    }
                    return Err(service_error(text(&value, "detail")));
                }
                _ => return Err("实时语音服务未返回就绪状态".into()),
            }
        }
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct RecognitionStream {
    socket: Option<Socket>,
    job: meetings::Job,
    window_bytes: usize,
}
impl RecognitionStream {
    fn new(socket: Socket, job: meetings::Job) -> Self {
        Self {
            socket: Some(socket),
            job,
            window_bytes: 0,
        }
    }
}
struct Progress<'a> {
    channel: &'a Channel<Value>,
    workspace: &'a str,
    meeting: &'a str,
    session: &'a str,
}
impl Progress<'_> {
    fn transcript(
        &self,
        result: &Mutex<Transcript>,
        raw: &str,
        finishing: bool,
    ) -> Result<bool, String> {
        if raw.len() > 4 * 1024 * 1024 {
            return Err("实时转录响应超过大小限制".into());
        }
        let value: Value = serde_json::from_str(raw).map_err(|_| "实时转录响应无效")?;
        if let Some((snapshot, done)) = accept_transcript(result, &value, finishing)? {
            emit(
                self.channel,
                self.workspace,
                self.meeting,
                self.session,
                "delta",
                &snapshot,
            );
            return Ok(done);
        }
        Ok(false)
    }
}
async fn receive(
    socket: &mut Option<Socket>,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    match socket {
        Some(socket) => socket.next().await,
        None => std::future::pending().await,
    }
}
async fn send(socket: &mut Socket, message: Message) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(15), socket.send(message))
        .await
        .map_err(|_| "实时语音发送超时，录音已保留")?
        .map_err(|_| "实时语音连接中断，录音已保留".to_string())
}

// 只有已正常完成上一窗口，且新窗口明确因旧会话尚未释放而被拒绝时才重试握手。
// 此时尚未发送新窗口音频，已发送的音频永不自动重放。
async fn next_window(job: &meetings::Job) -> Result<Socket, String> {
    for attempt in 0..4 {
        match tokio::time::timeout(WAIT, connect(job))
            .await
            .map_err(|_| "实时语音下一段连接超时，录音已保留")?
        {
            Ok(socket) => return Ok(socket),
            Err(error) if error == MODEL_BUSY && attempt < 3 => {
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(MODEL_BUSY.into())
}

/// 仅结束当前识别窗口；麦克风和会议会话保持打开，后续 PCM 仍会进入有界队列。
async fn finish_window(
    stream: &mut RecognitionStream,
    result: &Mutex<Transcript>,
    progress: &Progress<'_>,
) -> Result<(), String> {
    let Some(mut socket) = stream.socket.take() else {
        return Ok(());
    };
    let outcome = tokio::time::timeout(WAIT, async {
        send(&mut socket, Message::text(r#"{"type":"stop"}"#)).await?;
        loop {
            let message = socket
                .next()
                .await
                .ok_or("实时语音分段收尾中断，已收到文字已保留")?
                .map_err(|_| "实时语音分段收尾失败，已收到文字已保留")?;
            match message {
                Message::Text(raw) => {
                    if progress.transcript(result, &raw, true)? {
                        let mut current = result.lock().map_err(|_| "实时转录状态不可用")?;
                        current.committed_text = current.text.clone();
                        current.window_text.clear();
                        return Ok::<(), String>(());
                    }
                }
                Message::Ping(payload) => send(&mut socket, Message::Pong(payload)).await?,
                Message::Close(_) => return Err("实时语音分段未完成，已收到文字已保留".into()),
                _ => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("实时语音分段收尾超时，已收到文字和录音已保留".into()));
    // oMLX 先发 done，再关闭连接，最后释放模型会话；完成关闭握手后才能打开下一窗口。
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let _ = socket.close(None).await;
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    stream.window_bytes = 0;
    outcome
}

async fn run(
    mut stream: RecognitionStream,
    mut rx: mpsc::Receiver<Command>,
    result: Arc<Mutex<Transcript>>,
    channel: Channel<Value>,
    workspace: String,
    id: String,
    session: String,
) {
    let deadline = tokio::time::sleep(Duration::from_secs(2 * 60 * 60 + 120));
    tokio::pin!(deadline);
    let progress = Progress {
        channel: &channel,
        workspace: &workspace,
        meeting: &id,
        session: &session,
    };
    let outcome: Result<(), String> = async {
        loop {
            tokio::select! {
                _ = &mut deadline => return Err("实时转录达到时限，请停止并保存录音".into()),
                command = rx.recv() => {
                    match command {
                        Some(Command::Audio(chunk, pcm, ack)) => {
                            let sent = send_audio(&mut stream, &result, &progress, chunk, pcm).await;
                            let _ = ack.send(sent.clone());
                            sent?;
                            // 固定音频上限避免中文修订结果被搁置至整场 stop，后续窗口按需再连接。
                            if stream.window_bytes == WINDOW_PCM_BYTES {
                                finish_window(&mut stream, &result, &progress).await?;
                            }
                        }
                        Some(Command::Stop) => return finish_window(&mut stream, &result, &progress).await,
                        None => return Err("实时转录会话已取消".into()),
                    }
                }
                message = receive(&mut stream.socket) => {
                    let message = message.ok_or("实时语音连接中断，已收到的文字和录音已保留")?
                        .map_err(|_| "实时语音连接失败，录音已保留")?;
                    match message {
                        Message::Text(raw) => { progress.transcript(&result, &raw, false)?; }
                        Message::Ping(payload) => {
                            if let Some(socket) = &mut stream.socket { send(socket, Message::Pong(payload)).await?; }
                        }
                        Message::Close(_) => return Err("实时语音连接提前关闭，录音已保留".into()),
                        _ => {}
                    }
                }
            }
        }
    }.await;
    if let Some(socket) = &mut stream.socket {
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.close(None)).await;
    }
    if let Ok(mut current) = result.lock() {
        current.error = outcome.err();
        current.ended = true;
        if let Some(error) = &current.error {
            emit(&channel, &workspace, &id, &session, "error", error);
        }
    }
}

/// 校验候选正文后才修改已确认文字，坏帧不能污染失败收尾的数据。
fn accept_transcript(
    result: &Mutex<Transcript>,
    value: &Value,
    stopping: bool,
) -> Result<Option<(String, bool)>, String> {
    let kind = text(value, "type");
    if kind == "error" {
        return Err(service_error(text(value, "detail")));
    }
    let done = kind == "transcript.done";
    if kind != "transcript.delta" && !done {
        return Ok(None);
    }
    if done && !stopping {
        return Err("实时语音服务提前结束".into());
    }
    let fragment = value[if done { "text" } else { "delta" }]
        .as_str()
        .ok_or("实时转录正文格式无效，已收到文字已保留")?;
    let mut current = result.lock().map_err(|_| "实时转录状态不可用")?;
    let window_count = fragment.chars().count()
        + if done {
            0
        } else {
            current.window_text.chars().count()
        };
    let count = current.committed_text.chars().count()
        + window_count
        + usize::from(!current.committed_text.is_empty() && window_count > 0);
    if count > 100_000 {
        return Err("实时转录文字超过大小限制，已收到文字已保留".into());
    }
    let window = if done {
        fragment.to_owned()
    } else {
        format!("{}{}", current.window_text, fragment)
    };
    let candidate = if current.committed_text.is_empty() {
        window.clone()
    } else if window.is_empty() {
        current.committed_text.clone()
    } else {
        format!("{}\n{}", current.committed_text, window)
    };
    current.window_text = window;
    current.text = candidate.clone();
    Ok(Some((candidate, done)))
}

async fn send_audio(
    stream: &mut RecognitionStream,
    result: &Mutex<Transcript>,
    progress: &Progress<'_>,
    chunk: String,
    pcm: Vec<u8>,
) -> Result<(), String> {
    if result
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .sent
        .contains(&chunk)
    {
        return Err("该音频片段已经发送，禁止重复发送".into());
    }
    if pcm.is_empty() || !pcm.len().is_multiple_of(32) {
        return Err("实时音频采样格式无效".into());
    }
    let mut consumed = 0;
    while consumed < pcm.len() {
        if stream.window_bytes == WINDOW_PCM_BYTES {
            finish_window(stream, result, progress).await?;
        }
        if stream.socket.is_none() {
            stream.socket = Some(next_window(&stream.job).await?);
        }
        let size = (WINDOW_PCM_BYTES - stream.window_bytes).min(pcm.len() - consumed);
        let socket = stream.socket.as_mut().ok_or("实时语音连接不可用")?;
        send(
            socket,
            Message::Binary(pcm[consumed..consumed + size].to_vec().into()),
        )
        .await?;
        consumed += size;
        stream.window_bytes += size;
        result.lock().map_err(|_| "实时转录状态不可用")?.duration += size as i64 / 32;
    }
    result
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .sent
        .push(chunk);
    Ok(())
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
    let pcm = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        meetings::context(&store, &workspace_id, &session.generation)?;
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let (sequence, pcm): (i64, Vec<u8>) = conn
            .query_row(
                "SELECT sequence,pcm FROM meeting_audio_chunks WHERE meeting_id=?1 AND chunk_id=?2",
                params![id, chunk_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or("请先保存录音片段")?;
        let current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
        if current.ended {
            return Err(current.error.clone().unwrap_or("实时转录会话已结束".into()));
        }
        if sequence != session.first_sequence + current.sent.len() as i64 {
            return Err("实时音频片段顺序无效，请停止录音后重新转录".into());
        }
        if pcm.len() > 640_000 {
            return Err("实时音频片段过大".into());
        }
        pcm
    };
    let (ack, wait) = oneshot::channel();
    session
        .tx
        .try_send(Command::Audio(chunk_id, pcm, ack))
        .map_err(|_| "实时转录处理积压，请停止录音；已保存音频可重新转录")?;
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .map_err(|_| "实时音频发送超时，请停止录音")?
        .map_err(|_| "实时语音连接已中断")?
}

#[tauri::command]
pub async fn meeting_realtime_stop(
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    session_id: String,
) -> Result<Value, String> {
    let session = get(&workspace_id, &id, &session_id)?;
    let _cleanup = StopCleanup {
        workspace: &workspace_id,
        meeting: &id,
        session: &session,
    };
    let ended = session
        .result
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .ended;
    if !ended && session.tx.try_send(Command::Stop).is_err() {
        cancel(
            &session,
            "实时转录停止队列不可用，已取消连接；录音和已收到文字已保留",
        );
    }
    let wait = async {
        loop {
            if session
                .result
                .lock()
                .map_err(|_| "实时转录状态不可用")?
                .ended
            {
                return Ok::<(), String>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    match tokio::time::timeout(WAIT + Duration::from_secs(3), wait).await {
        Ok(result) => result?,
        Err(_) => cancel(
            &session,
            "实时语音收尾超时，连接已取消；录音和已收到文字已保留",
        ),
    }
    let store = state.store.lock().map_err(|e| e.to_string())?;
    // 先释放会话锁；失效工作空间绝不写回新数据库。
    if let Err(error) = meetings::context(&store, &workspace_id, &session.generation) {
        remove_session(&workspace_id, &id, &session.id, &session.generation)?;
        return Err(error);
    }
    let saved = commit_result(&store, &workspace_id, &id, &session)?;
    remove_session(&workspace_id, &id, &session.id, &session.generation)?;
    if let Some(error) = &session
        .result
        .lock()
        .map_err(|_| "实时转录状态不可用")?
        .error
    {
        return Err(error.clone());
    }
    Ok(saved)
}

fn commit_result(
    store: &crate::storage::Store,
    workspace_id: &str,
    id: &str,
    session: &Session,
) -> Result<Value, String> {
    meetings::context(store, workspace_id, &session.generation)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut value = meetings::read(&tx, id)?;
    let current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
    let segment_id = format!("realtime:{}", session.id);
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meeting_segments WHERE meeting_id=?1 AND id=?2)",
            params![id, segment_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(value);
    }
    let revision = value["revision"].as_i64().ok_or("会议版本无效")?;
    if !current.text.trim().is_empty() {
        let segment = json!({"id":format!("realtime:{}",session.id),"startMs":session.offset,"endMs":session.offset+current.duration,"text":current.text});
        // 单会话追加一段，保留之前所有校对文本与发言人姓名。
        meetings::insert_segment(&tx, id, &segment, Some(&format!("realtime:{}", session.id)))?;
        value["transcriptionModelId"] = json!(session.model_id);
        meetings::stale_analysis(&mut value);
        meetings::write_metadata(&tx, &mut value, revision)?;
    }
    let saved = meetings::read(&tx, id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}

#[cfg(test)]
mod window_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    fn test_job(address: std::net::SocketAddr) -> meetings::Job {
        meetings::Job {
            generation: PathBuf::from("test-generation"),
            revision: 1,
            model: json!({"remoteModelId":"test-whisper"}),
            provider: json!({"baseUrl":format!("http://{address}/v1")}),
            key: "test-key-not-a-credential".into(),
        }
    }
    #[tokio::test]
    async fn protocol_auth_ready_pcm_deltas_and_stop_tail() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(
                stream,
                |req: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(req.uri().path(), "/v1/audio/transcriptions/realtime");
                    assert!(req.uri().query().is_none());
                    assert_eq!(
                        req.headers()["authorization"],
                        "Bearer test-key-not-a-credential"
                    );
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let start: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                start,
                json!({"type":"start","model":"test-whisper","api_key":"test-key-not-a-credential"})
            );
            ws.send(Message::text("{\"type\":\"ready\"}"))
                .await
                .unwrap();
            let pcm = ws.next().await.unwrap().unwrap();
            assert_eq!(pcm.into_data().len(), 32_000);
            ws.send(Message::text(
                "{\"type\":\"transcript.delta\",\"delta\":\"你好\"}",
            ))
            .await
            .unwrap();
            let stop: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(stop["type"], "stop");
            ws.send(Message::text(
                "{\"type\":\"transcript.delta\",\"delta\":\"世界\"}",
            ))
            .await
            .unwrap();
            ws.send(Message::text(
                "{\"type\":\"transcript.done\",\"text\":\"你好世界。\"}",
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });
        let socket = connect(&test_job(address)).await.unwrap();
        let (tx, rx) = mpsc::channel(6);
        let result = Arc::new(Mutex::new(Transcript::default()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(raw) = body {
                captured
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str::<Value>(&raw).unwrap());
            }
            Ok(())
        });
        let task = tokio::spawn(run(
            RecognitionStream::new(socket, test_job(address)),
            rx,
            result.clone(),
            channel,
            "w".into(),
            "m".into(),
            "s".into(),
        ));
        let (ack, wait) = oneshot::channel();
        tx.send(Command::Audio("c".into(), vec![0; 32_000], ack))
            .await
            .unwrap();
        wait.await.unwrap().unwrap();
        tx.send(Command::Stop).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        let current = result.lock().unwrap();
        assert!(current.ended);
        assert!(current.error.is_none());
        assert_eq!(current.text, "你好世界。");
        assert_eq!(current.duration, 1000);
        assert_eq!(current.sent, vec!["c"]);
        let events = events.lock().unwrap();
        assert_eq!(events[0]["text"], "你好");
        assert_eq!(events[1]["text"], "你好世界");
        assert_eq!(events[2]["text"], "你好世界。");
        assert_eq!(events[0]["sessionId"], "s");
        assert!(events.iter().all(|e| !e.to_string().contains("test-key")));
    }
    #[tokio::test]
    async fn rejected_start_and_http_401_are_actionable_without_secret_echo() {
        for http in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                if http {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0; 4096];
                    let _ = stream.read(&mut buf).await;
                    stream
                        .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                        .await
                        .unwrap();
                } else {
                    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                    let _ = ws.next().await;
                    ws.send(Message::text("{\"type\":\"error\",\"detail\":\"Invalid API key test-key-not-a-credential\"}")).await.unwrap();
                }
            });
            let error = connect(&test_job(address)).await.err().unwrap();
            assert!(error.contains("认证失败"));
            assert!(error.contains("供应商"));
            assert!(!error.contains("test-key"));
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn disconnect_keeps_partial_text_and_reports_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = ws.next().await;
            ws.send(Message::text("{\"type\":\"ready\"}"))
                .await
                .unwrap();
            let _ = ws.next().await;
            ws.send(Message::text(
                "{\"type\":\"transcript.delta\",\"delta\":\"保留文字\"}",
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
        });
        let socket = connect(&test_job(address)).await.unwrap();
        let (tx, rx) = mpsc::channel(6);
        let result = Arc::new(Mutex::new(Transcript::default()));
        let task = tokio::spawn(run(
            RecognitionStream::new(socket, test_job(address)),
            rx,
            result.clone(),
            Channel::new(|_| Ok(())),
            "w".into(),
            "m".into(),
            "s".into(),
        ));
        let (ack, wait) = oneshot::channel();
        tx.send(Command::Audio("c".into(), vec![0; 32_000], ack))
            .await
            .unwrap();
        wait.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        let current = result.lock().unwrap();
        assert!(current.ended);
        assert!(current.error.is_some());
        assert_eq!(current.text, "保留文字");
    }
    #[tokio::test]
    async fn malformed_or_oversized_final_frames_keep_previous_valid_text() {
        for bad in [
            json!({"type":"transcript.done"}),
            json!({"type":"transcript.done","text":12}),
            json!({"type":"transcript.done","text":"坏".repeat(100_001)}),
            json!({"type":"transcript.delta","delta":"坏".repeat(100_001)}),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                let _ = ws.next().await;
                ws.send(Message::text("{\"type\":\"ready\"}"))
                    .await
                    .unwrap();
                let _ = ws.next().await;
                ws.send(Message::text(
                    "{\"type\":\"transcript.delta\",\"delta\":\"有效正文\"}",
                ))
                .await
                .unwrap();
                let _ = ws.next().await;
                ws.send(Message::text(bad.to_string())).await.unwrap();
                let _ = ws.next().await;
            });
            let socket = connect(&test_job(address)).await.unwrap();
            let (tx, rx) = mpsc::channel(6);
            let result = Arc::new(Mutex::new(Transcript::default()));
            let task = tokio::spawn(run(
                RecognitionStream::new(socket, test_job(address)),
                rx,
                result.clone(),
                Channel::new(|_| Ok(())),
                "w".into(),
                "m".into(),
                "s".into(),
            ));
            let (ack, wait) = oneshot::channel();
            tx.send(Command::Audio("c".into(), vec![0; 32000], ack))
                .await
                .unwrap();
            wait.await.unwrap().unwrap();
            tx.send(Command::Stop).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            server.await.unwrap();
            let current = result.lock().unwrap();
            assert_eq!(current.text, "有效正文");
            assert!(current.ended);
            assert!(current.error.is_some());
        }
    }

    #[tokio::test]
    async fn failed_stop_cleanup_unlocks_meeting_and_aborts_actor() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::storage::Store::open_at(dir.path().join("workspace")).unwrap();
        let workspace = store.workspace_id.clone();
        let (tx, _rx) = mpsc::channel(6);
        // 满队列无法发送 Stop 时仍能明确取消，不等待两小时总时限。
        for _ in 0..6 {
            tx.try_send(Command::Stop).unwrap();
        }
        let actor = tokio::spawn(std::future::pending::<()>());
        let session = Arc::new(Session {
            id: "cleanup".into(),
            generation: store.generation.clone(),
            model_id: "test".into(),
            offset: 0,
            first_sequence: 0,
            tx,
            result: Arc::new(Mutex::new(Transcript {
                text: "有效预览".into(),
                ..Default::default()
            })),
            task: Mutex::new(Some(actor.abort_handle())),
        });
        sessions()
            .lock()
            .unwrap()
            .insert(key(&workspace, "missing"), session.clone());
        assert!(ensure_all_idle().is_err());
        {
            let _cleanup = StopCleanup {
                workspace: &workspace,
                meeting: "missing",
                session: &session,
            };
            assert!(session.tx.try_send(Command::Stop).is_err());
            cancel(&session, "停止队列已满，连接已取消");
            // 模拟会议被删除或数据库提交失败；清理不能依赖 commit_result 成功。
            assert!(commit_result(&store, &workspace, "missing", &session).is_err());
        }
        assert!(ensure_idle(&workspace, "missing").is_ok());
        assert!(ensure_all_idle().is_ok());
        assert!(actor.await.unwrap_err().is_cancelled());
        let current = session.result.lock().unwrap();
        assert_eq!(current.text, "有效预览");
        assert!(current.ended);
        assert!(current.error.as_deref().unwrap().contains("已取消"));
    }

    #[test]
    fn partial_result_preserves_audio_old_edits_and_rejects_restored_generation() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::storage::Store::open_at(dir.path().join("workspace")).unwrap();
        let workspace = store.workspace_id.clone();
        {
            let conn = store.conn.lock().unwrap();
            let metadata = json!({"id":"m","revision":3,"title":"保留标题","speakerNames":{"old-speaker":"张三"},"analysis":{"stale":false,"actionItems":[]}});
            conn.execute(
                "INSERT INTO meetings(id,revision,data) VALUES('m',3,?1)",
                [metadata.to_string()],
            )
            .unwrap();
            for (sequence, chunk) in [(0, "old"), (1, "new")] {
                conn.execute("INSERT INTO meeting_audio_chunks(meeting_id,chunk_id,sequence,duration_ms,pcm) VALUES('m',?1,?2,1000,?3)",params![chunk,sequence,vec![0u8;32000]]).unwrap();
            }
            meetings::insert_segment(&conn,"m",&json!({"id":"old-edit","startMs":0,"endMs":1000,"text":"已人工校对","speaker":"old-speaker"}),Some("old")).unwrap();
        }
        let (tx, _) = mpsc::channel(6);
        let session = Arc::new(Session {
            id: "partial".into(),
            generation: store.generation.clone(),
            model_id: "whisper".into(),
            offset: 1000,
            first_sequence: 1,
            tx,
            task: Mutex::new(None),
            result: Arc::new(Mutex::new(Transcript {
                text: "收到的部分文字".into(),
                error: Some("断线".into()),
                ended: true,
                sent: vec!["new".into()],
                duration: 1000,
                ..Default::default()
            })),
        });
        sessions()
            .lock()
            .unwrap()
            .insert(key(&workspace, "m"), session.clone());
        let cleanup = StopCleanup {
            workspace: &workspace,
            meeting: "m",
            session: &session,
        };
        assert!(ensure_idle(&workspace, "m").is_err());
        let marked =
            meetings::add_marker(&store, &workspace, "m", "marker-live", 1500, "核对发布范围")
                .unwrap();
        assert_eq!(marked["markers"][0]["text"], "核对发布范围");
        let saved = commit_result(&store, &workspace, "m", &session).unwrap();
        assert_eq!(saved["markers"], marked["markers"]);
        assert_eq!(saved["segments"][0]["text"], "已人工校对");
        assert_eq!(saved["segments"][1]["text"], "收到的部分文字");
        assert!(saved["segments"][1].get("speaker").is_none());
        assert_eq!(saved["speakerNames"]["old-speaker"], "张三");
        assert_eq!(saved["audioChunks"], 2);
        assert_eq!(saved["durationMs"], 2000);
        assert_eq!(saved["analysis"]["stale"], true);
        // 收尾重入不能重复插入正文，也不能重写人工校对。
        let again = commit_result(&store, &workspace, "m", &session).unwrap();
        assert_eq!(again["segments"].as_array().unwrap().len(), 2);
        assert_eq!(again["revision"], saved["revision"]);
        drop(cleanup);
        store.generation = PathBuf::from("restored-generation");
        assert!(commit_result(&store, &workspace, "m", &session)
            .unwrap_err()
            .contains("恢复或切换"));
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            meetings::read(&conn, "m").unwrap()["revision"],
            saved["revision"]
        );
    }
    #[test]
    fn realtime_urls_reject_credentials_and_non_http_schemes() {
        for url in [
            "file:///tmp/model",
            "http://user:pass@localhost/v1",
            "http://localhost/v1?api_key=secret",
        ] {
            assert!(ws_url(url).is_err());
        }
        assert_eq!(
            ws_url("https://localhost/v1/").unwrap(),
            "wss://localhost/v1/audio/transcriptions/realtime"
        );
    }
}
