//! 基于 audio.cpp 的本机流式语音模型（Confucius4-R2T2、Qwen3-ASR-0.6B）权重和运行包随需下载。录音只通过子进程的匿名管道传递。
use super::{download_file, model_installer, report_progress, verify, Download, ModelFile};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioCppModel {
    R2t2,
    Qwen3Asr06b,
}

impl AudioCppModel {
    fn weights(self) -> &'static ModelFile {
        match self {
            Self::R2t2 => &MODEL,
            Self::Qwen3Asr06b => &QWEN3_ASR_06B_MODEL,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::R2t2 => "Confucius4-R2T2",
            Self::Qwen3Asr06b => "Qwen3-ASR-0.6B",
        }
    }

    fn family(self) -> &'static str {
        match self {
            Self::R2t2 => "confucius4_r2t2",
            Self::Qwen3Asr06b => "qwen3_asr",
        }
    }

    fn extra_args(self) -> &'static [&'static str] {
        match self {
            Self::R2t2 => &[
                "--session-option",
                "confucius4_r2t2.chunk_size_ms=320",
                "--session-option",
                "confucius4_r2t2.unfixed_token_num=1",
            ],
            Self::Qwen3Asr06b => &["--request-option", "audio_chunk_duration_sec=4"],
        }
    }

    fn unsupported_message(self) -> &'static str {
        match self {
            Self::R2t2 => "Confucius4-R2T2 支持 Apple 芯片 macOS 和 Windows x64",
            Self::Qwen3Asr06b => "Qwen3-ASR-0.6B 支持 Apple 芯片 macOS 和 Windows x64",
        }
    }
}

static MODEL: ModelFile = ModelFile {
    source: Some("https://huggingface.co/Nairod785/Confucius4-R2T2-Q4_K_M-GGUF/resolve/b1ea19256fb77a8d8ab7b091dc75378e15952605"),
    name: "r2t2-q4_k_m.gguf",
    bytes: 1_186_939_968,
    sha256: "d740d6636f2ea2f3736800c3c88a6e22ecb6c0f26c567fe22b0572ae9c2c4ec8",
};

static QWEN3_ASR_06B_MODEL: ModelFile = ModelFile {
    source: Some("https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/047ec541cc3f7e50c583acd56e0177ae286c4078/Qwen3-ASR-0.6B-GGUF"),
    name: "qwen3-asr-0.6b-q8_0.gguf",
    bytes: 1_151_279_968,
    sha256: "5c53bbb9a6b2ade85c16c290c560658ca18b78a23c323378528f1f945683fcbe",
};

#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64"), all(target_os = "windows", target_arch = "x86_64")))]
const RELEASE_URL: Option<&str> =
    Some("https://github.com/0xShug0/audio.cpp/releases/download/v0.9.6");

#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
static MACOS_RUNTIME: ModelFile = ModelFile {
    source: RELEASE_URL,
    name: "audio-v0.9.6-bin-macos-arm64-metal.tar.gz",
    bytes: 40_423_741,
    sha256: "11848bf10d1142f18ec852051ff8dcd90c12b65b95e6f902b487bae565e3dfe0",
};

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
static WINDOWS_RUNTIME: ModelFile = ModelFile {
    source: RELEASE_URL,
    name: "audio-v0.9.6-bin-windows-x64-cpu-portable.zip",
    bytes: 35_202_438,
    sha256: "5d2e7fe24dbec0f8267db00ab2c6ccdd24799d916cb7beef3168eb66a53a4077",
};

fn runtime() -> Option<(&'static ModelFile, &'static str)> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some((&MACOS_RUNTIME, "metal"));
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some((&WINDOWS_RUNTIME, "cpu"));
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    return None;
}

pub(crate) fn unsupported_reason(model: AudioCppModel) -> Option<&'static str> {
    runtime().is_none().then_some(model.unsupported_message())
}

pub(crate) fn total_bytes(model: AudioCppModel) -> u64 {
    runtime().map_or(0, |(archive, _)| model.weights().bytes + archive.bytes)
}

pub(crate) fn is_installed(model: AudioCppModel, path: &Path) -> bool {
    let weights = model.weights();
    runtime().is_some_and(|(archive, _)| {
        [archive, weights].iter().all(|file| {
            fs::symlink_metadata(path.join(file.name))
                .is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
        })
    })
}

pub(super) async fn install(
    model: AudioCppModel,
    path: &Path,
    status: &Mutex<Download>,
) -> Result<(), String> {
    let name = model.display_name();
    let weights = model.weights();
    let (archive, _) = runtime().ok_or_else(|| format!("当前设备不支持 {name}"))?;
    let (_lock, client) = model_installer(path).await?;
    let mut completed = 0;
    for file in [archive, weights] {
        let target = path.join(file.name);
        let verified = tokio::task::spawn_blocking({
            let target = target.clone();
            move || verify(&target, file)
        })
        .await
        .map_err(|_| format!("无法校验 {name} 文件"))?;
        if !verified {
            download_file(&client, path, &target, file, |bytes| {
                report_progress(status, completed + bytes);
            })
            .await?;
        }
        completed += file.bytes;
        report_progress(status, completed);
    }
    Ok(())
}

const READY: &str = "audio_input=stdin format=s16le rate=16000 channels=1";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);
const INPUT_QUEUE_PACKETS: usize = 64;
const PCM_BYTES_PER_SECOND: usize = 32_000;
const SILENCE_BYTES: usize = PCM_BYTES_PER_SECOND * 4 / 10;
// 上游单流音频长度有限。安静时接续，连续讲话也不超过硬上限。
const WINDOW_TARGET_BYTES: usize = 24 * PCM_BYTES_PER_SECOND;
const WINDOW_LIMIT_BYTES: usize = 32 * PCM_BYTES_PER_SECOND;
const MAX_PACKET_BYTES: usize = 32_000;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct Output {
    text: String,
    error: Option<&'static str>,
}

impl Output {
    fn accept(&mut self, line: &str) {
        if self.error.is_some() {
            return;
        }
        let delta = if let Some(delta) = line.strip_prefix("partial_text=") {
            Some(delta)
        } else {
            // EOF 的最终结果可能重复全部已提交文字。只补提交前缀之后的尾段。
            line.strip_prefix("text_output=")
                .and_then(|text| text.strip_prefix(&self.text))
        };
        if let Some(delta) = delta {
            if self.text.len() + delta.len() <= MAX_TEXT_BYTES {
                self.text.push_str(delta);
            } else {
                self.error = Some("Confucius4-R2T2 转录文本过长");
            }
        }
    }
}

pub(crate) struct Stream {
    input: Option<SyncSender<Vec<u8>>>,
    output: Arc<Mutex<Output>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    stopped: Receiver<Result<(), String>>,
    completion: Option<Result<(), String>>,
    consumed: usize,
}

impl Stream {
    // 调用方持有模型目录读锁直到会话结束，避免使用中的文件被移除。
    pub(crate) fn start(engine_model: AudioCppModel, path: &Path) -> Result<Self, String> {
        let name = engine_model.display_name();
        let weights = engine_model.weights();
        let (archive, backend) = runtime().ok_or_else(|| format!("当前设备不支持 {name}"))?;
        if !is_installed(engine_model, path) {
            return Err(format!("请先在会议语音设置中下载 {name}"));
        }
        if !verify(&path.join(weights.name), weights) || !verify(&path.join(archive.name), archive)
        {
            return Err(format!("{name} 文件校验失败，请重新下载"));
        }
        let model = fs::canonicalize(path.join(weights.name))
            .map_err(|_| format!("无法定位 {name} 权重"))?;
        let directory = tempfile::tempdir().map_err(|_| "无法创建语音运行临时目录")?;
        unpack_runtime(&path.join(archive.name), directory.path())?;
        let exe = find_executable(directory.path(), 3)?;
        Self::start_with(move |cancelled| {
            create_process(
                engine_model,
                &exe,
                &model,
                backend,
                directory.path(),
                cancelled,
            )
        })
    }

    fn start_with(
        start: impl Fn(&AtomicBool) -> Result<Process, String> + Send + 'static,
    ) -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let process = start(&cancelled)?;
        let (input, audio) = mpsc::sync_channel(INPUT_QUEUE_PACKETS);
        let (done, stopped) = mpsc::sync_channel(1);
        let output = Arc::new(Mutex::new(Output::default()));
        let worker = thread::Builder::new()
            .spawn({
                let output = output.clone();
                let cancelled = cancelled.clone();
                move || {
                    let result = run_processes(start, process, audio, &output, &cancelled);
                    let _ = done.send(result);
                }
            })
            .map_err(|_| "无法启动 Confucius4-R2T2 转录线程")?;
        Ok(Self {
            input: Some(input),
            output,
            cancelled,
            worker: Some(worker),
            stopped,
            completion: None,
            consumed: 0,
        })
    }

    pub(crate) fn feed_pcm(&mut self, pcm: &[u8]) -> Result<(), String> {
        if self.completion.is_none() {
            self.completion = self.poll_stop();
        }
        if let Some(result) = &self.completion {
            return result.clone().and(Err("Confucius4-R2T2 输入已结束".into()));
        }
        send_audio(&self.input, pcm)
    }

    fn poll_stop(&self) -> Option<Result<(), String>> {
        match self.stopped.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Confucius4-R2T2 转录线程已关闭".into()))
            }
        }
    }

    pub(crate) fn take_delta(&mut self) -> String {
        take_delta(&self.output, &mut self.consumed)
    }

    pub(crate) fn finish(&mut self) -> Result<(), String> {
        self.input.take();
        if let Some(worker) = self.worker.take() {
            if self.completion.is_none() {
                self.completion = Some(match self.stopped.recv_timeout(FLUSH_TIMEOUT) {
                    Ok(result) => result,
                    Err(RecvTimeoutError::Timeout) => {
                        Err("Confucius4-R2T2 等待最后一段转录超时".into())
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        Err("Confucius4-R2T2 转录线程已关闭".into())
                    }
                });
            }
            self.cancelled.store(true, Ordering::Release);
            let _ = worker.join();
        }
        self.completion.clone().unwrap_or(Ok(()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.input.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_processes(
    start: impl Fn(&AtomicBool) -> Result<Process, String>,
    first: Process,
    audio: Receiver<Vec<u8>>,
    output: &Mutex<Output>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut process = Some(first);
    let mut window_bytes = 0;
    let mut silence_bytes = 0;
    while !cancelled.load(Ordering::Acquire) {
        if let Some(child) = &mut process {
            publish(output, child)?;
        }
        match audio.recv_timeout(Duration::from_millis(20)) {
            Ok(pcm) => {
                let child = match &mut process {
                    Some(child) => child,
                    None => process.insert(start(cancelled)?),
                };
                if let Err(error) = child.feed_pcm(&pcm) {
                    let _ = child.finish(cancelled);
                    publish(output, child)?;
                    return Err(error);
                }
                window_bytes += pcm.len();
                silence_bytes = if pcm
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .any(|&sample| i16::from_le_bytes(sample).unsigned_abs() >= 132)
                {
                    0
                } else {
                    silence_bytes + pcm.len()
                };
                if window_bytes >= WINDOW_LIMIT_BYTES
                    || (window_bytes >= WINDOW_TARGET_BYTES && silence_bytes >= SILENCE_BYTES)
                {
                    let result = child.finish(cancelled);
                    publish(output, child)?;
                    result?;
                    process = None;
                    window_bytes = 0;
                    silence_bytes = 0;
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Some(child) = &mut process {
        let result = if cancelled.load(Ordering::Acquire) {
            Ok(())
        } else {
            child.finish(cancelled)
        };
        publish(output, child)?;
        result?;
    }
    Ok(())
}

fn publish(output: &Mutex<Output>, child: &mut Process) -> Result<(), String> {
    let first = child.consumed == 0;
    let delta = child.take_delta();
    if delta.is_empty() {
        return Ok(());
    }
    let mut output = output.lock().map_err(|_| "无法读取转录增量")?;
    let separator = first && !output.text.is_empty() && !output.text.ends_with(char::is_whitespace);
    if output.text.len() + delta.len() + usize::from(separator) > MAX_TEXT_BYTES {
        return Err("Confucius4-R2T2 转录文本过长".into());
    }
    if separator {
        output.text.push('\n');
    }
    output.text.push_str(&delta);
    Ok(())
}

fn take_delta(output: &Mutex<Output>, consumed: &mut usize) -> String {
    let Ok(output) = output.lock() else {
        return String::new();
    };
    let text = output.text[*consumed..].to_owned();
    *consumed = output.text.len();
    text
}

fn send_audio(input: &Option<SyncSender<Vec<u8>>>, pcm: &[u8]) -> Result<(), String> {
    if pcm.is_empty() || !pcm.len().is_multiple_of(2) || pcm.len() > MAX_PACKET_BYTES {
        return Err("Confucius4-R2T2 音频分片格式无效".into());
    }
    input
        .as_ref()
        .ok_or("Confucius4-R2T2 输入已结束")?
        .try_send(pcm.to_vec())
        .map_err(|error| match error {
            TrySendError::Full(_) => "Confucius4-R2T2 处理跟不上录音速度".into(),
            TrySendError::Disconnected(_) => "Confucius4-R2T2 音频管道意外关闭".into(),
        })
}

struct Process {
    child: Child,
    input: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<io::Result<()>>>,
    reader: Option<JoinHandle<()>>,
    output: Arc<Mutex<Output>>,
    consumed: usize,
}

fn create_process(
    engine_model: AudioCppModel,
    exe: &Path,
    model: &Path,
    backend: &str,
    directory: &Path,
    cancelled: &AtomicBool,
) -> Result<Process, String> {
    let mut command = Command::new(exe);
    command
        .current_dir(directory)
        .args([
            "--task",
            "asr",
            "--mode",
            "streaming",
            "--family",
            engine_model.family(),
            "--backend",
            backend,
            "--input-format",
            "s16le",
            "--input-rate",
            "16000",
            "--input-channels",
            "1",
            "--audio",
            "-",
        ])
        .args(engine_model.extra_args())
        .arg("--model")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    Process::spawn(command, cancelled)
}

impl Process {
    fn spawn(mut command: Command, cancelled: &AtomicBool) -> Result<Self, String> {
        let child = command
            .spawn()
            .map_err(|_| "无法启动 Confucius4-R2T2 本机引擎")?;
        let mut stream = Self {
            child,
            input: None,
            writer: None,
            reader: None,
            output: Arc::default(),
            consumed: 0,
        };
        let mut stdin = stream.child.stdin.take().ok_or("无法打开语音输入管道")?;
        let stdout = stream.child.stdout.take().ok_or("无法打开转录输出管道")?;
        let (ready, wait_ready) = mpsc::sync_channel(1);
        let output = stream.output.clone();
        stream.reader = Some(
            thread::Builder::new()
                .spawn(move || read_output(stdout, output, ready))
                .map_err(|_| "无法启动转录输出线程")?,
        );
        let (input, rx) = mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE_PACKETS);
        stream.input = Some(input);
        stream.writer = Some(
            thread::Builder::new()
                .spawn(move || {
                    let mut has_audio = false;
                    while let Ok(pcm) = rx.recv() {
                        stdin.write_all(&pcm)?;
                        has_audio = true;
                    }
                    // 补 320 ms 静音，给模型保留尾词上下文，再关闭 stdin 执行刷新。
                    if has_audio {
                        stdin.write_all(&[0; 10_240])?;
                    }
                    Ok(())
                })
                .map_err(|_| "无法启动语音输入线程")?,
        );
        let started = Instant::now();
        while !cancelled.load(Ordering::Acquire) && started.elapsed() < START_TIMEOUT {
            match wait_ready.recv_timeout(Duration::from_millis(20)) {
                Ok(()) => return Ok(stream),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("Confucius4-R2T2 无法加载到本机引擎".into());
                }
            }
        }
        Err("Confucius4-R2T2 启动超时或已取消".into())
    }

    fn feed_pcm(&mut self, pcm: &[u8]) -> Result<(), String> {
        self.check_output()?;
        send_audio(&self.input, pcm)
    }

    fn take_delta(&mut self) -> String {
        take_delta(&self.output, &mut self.consumed)
    }

    fn finish(&mut self, cancelled: &AtomicBool) -> Result<(), String> {
        // 发送端关闭后写线程排空队列并关闭 stdin，CLI 随后执行模型 flush。
        self.input.take();
        let start = Instant::now();
        let result = loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => break Ok(()),
                Ok(Some(_)) => break Err("Confucius4-R2T2 转录进程意外退出"),
                Ok(None)
                    if start.elapsed() < FLUSH_TIMEOUT && !cancelled.load(Ordering::Acquire) =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => break Err("Confucius4-R2T2 等待最后一段转录超时"),
                Err(_) => break Err("无法检查 Confucius4-R2T2 进程"),
            }
        };
        if result.is_err() {
            self.terminate();
        }
        let writer = self.writer.take().and_then(|thread| thread.join().ok());
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
        result?;
        if writer.is_some_and(|result| result.is_err()) {
            return Err("Confucius4-R2T2 音频写入未完成".into());
        }
        self.check_output()
    }

    fn check_output(&self) -> Result<(), String> {
        let output = self.output.lock().map_err(|_| "无法读取转录增量")?;
        output.error.map_or(Ok(()), |error| Err(error.into()))
    }

    fn terminate(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.terminate();
        if let Some(thread) = self.writer.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.reader.take() {
            let _ = thread.join();
        }
    }
}

fn read_output(stdout: impl Read, output: Arc<Mutex<Output>>, ready: SyncSender<()>) {
    let mut reader = BufReader::new(stdout);
    let mut ready = Some(ready);
    let mut line = Vec::new();
    let error = loop {
        line.clear();
        let count = match reader
            .by_ref()
            .take((MAX_TEXT_BYTES + 32) as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(0) if ready.is_some() => break "Confucius4-R2T2 未就绪",
            Ok(0) => return,
            Ok(count) => count,
            Err(_) => break "无法读取 Confucius4-R2T2 转录结果",
        };
        if count >= MAX_TEXT_BYTES + 32 {
            break "Confucius4-R2T2 转录响应过长";
        }
        let Ok(line) = std::str::from_utf8(&line) else {
            break "Confucius4-R2T2 转录响应格式无效";
        };
        let line = line.trim_end_matches(['\r', '\n']);
        if ready.is_some() {
            if line == READY {
                let _ = ready.take().unwrap().send(());
            }
        } else if let Ok(mut output) = output.lock() {
            output.accept(line);
        }
    };
    if let Ok(mut output) = output.lock() {
        output.error = Some(error);
    }
}

fn unpack_runtime(archive: &Path, target: &Path) -> Result<(), String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        // 输入是已校验发布哈希的精确包，使用系统自带 BSD tar 解出配套 dylib。
        let status = Command::new("/usr/bin/tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "无法解压 Confucius4-R2T2 运行包")?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| "无法解压 Confucius4-R2T2 运行包".into())
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        let archive = fs::File::open(archive).map_err(|_| "无法读取 Confucius4-R2T2 运行包")?;
        zip::ZipArchive::new(archive)
            .and_then(|mut archive| archive.extract(target))
            .map_err(|_| "无法解压 Confucius4-R2T2 运行包".into())
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        let _ = (archive, target);
        Err("当前设备不支持 Confucius4-R2T2".into())
    }
}

fn find_executable(directory: &Path, depth: usize) -> Result<PathBuf, String> {
    let name = if cfg!(target_os = "windows") {
        "audiocpp_cli.exe"
    } else {
        "audiocpp_cli"
    };
    let exe = directory.join(name);
    if fs::symlink_metadata(&exe).is_ok_and(|meta| meta.is_file()) {
        return Ok(exe);
    }
    if depth > 0 {
        let entries = fs::read_dir(directory).map_err(|_| "无法读取语音运行包内容")?;
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                if let Ok(exe) = find_executable(&entry.path(), depth - 1) {
                    return Ok(exe);
                }
            }
        }
    }
    Err("Confucius4-R2T2 运行包缺少转录程序".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_asset_sha256_digests_are_valid_64_char_hex() {
        for file in [&MODEL, &QWEN3_ASR_06B_MODEL, &MACOS_RUNTIME, &WINDOWS_RUNTIME] {
            assert_eq!(
                file.sha256.len(),
                64,
                "{} 的 SHA-256 长度必须为 64 位十六进制",
                file.name
            );
            assert!(
                file.sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{} 的 SHA-256 包含无效字符",
                file.name
            );
        }
    }

    #[test]
    fn stdout_deltas_keep_spaces_and_final_output_only_supplies_tail() {
        let mut output = Output::default();
        output.accept("partial_text=你好");
        output.accept("partial_text_snapshot=另一版");
        output.accept("partial_text= world");
        output.accept("text_output=你好 world。");
        assert_eq!(output.text, "你好 world。");
    }

    #[cfg(unix)]
    fn fake_process(cancelled: &AtomicBool) -> Result<Process, String> {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s\\n' 'audio_input=stdin format=s16le rate=16000 channels=1'; dd bs=1 count=1 of=/dev/null 2>/dev/null; printf '%s\\n' 'partial_text=片段' 'partial_text= hello'; cat >/dev/null; printf '%s\\n' 'text_output=片段 hello。'",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Process::spawn(command, cancelled)
    }

    #[cfg(unix)]
    #[test]
    fn live_pipe_delivers_text_before_eof_and_restarts_after_long_audio() {
        let mut stream = Stream::start_with(fake_process).unwrap();
        let speaking = [1_u8; PCM_BYTES_PER_SECOND];
        stream.feed_pcm(&speaking).unwrap();
        let start = Instant::now();
        let mut text = String::new();
        while !text.ends_with("hello") && start.elapsed() < Duration::from_secs(2) {
            text.push_str(&stream.take_delta());
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(text, "片段 hello");
        for _ in 0..32 {
            stream.feed_pcm(&speaking).unwrap();
        }
        stream.finish().unwrap();
        text.push_str(&stream.take_delta());
        assert_eq!(text, "片段 hello。\n片段 hello。");
        assert!(stream.take_delta().is_empty());
        assert!(stream.feed_pcm(&speaking).is_err());
    }

    #[test]
    #[ignore = "使用锁定的运行包、权重和公开 16 kHz PCM 实测长录音"]
    fn native_pcm_crosses_runtime_window_and_flushes() {
        let root = PathBuf::from(std::env::var_os("PERCH_TEST_R2T2_DIR").unwrap());
        let pcm = fs::read(std::env::var_os("PERCH_TEST_R2T2_PCM").unwrap()).unwrap();
        assert!(pcm.len() > WINDOW_LIMIT_BYTES + 5 * PCM_BYTES_PER_SECOND);
        let mut stream = Stream::start(AudioCppModel::R2t2, &root).unwrap();
        let started = Instant::now();
        let mut transcript = String::new();
        let mut first = None;
        let mut samples = 0;
        for packet in pcm.chunks(5_120) {
            samples += packet.len() / 2;
            let captured = Duration::from_micros(samples as u64 * 1_000_000 / 16_000);
            thread::sleep(captured.saturating_sub(started.elapsed()));
            stream.feed_pcm(packet).unwrap();
            let delta = stream.take_delta();
            if !delta.is_empty() && first.is_none() {
                first = Some(started.elapsed());
            }
            transcript.push_str(&delta);
        }
        let stop = Instant::now();
        stream.finish().unwrap();
        let tail = stream.take_delta();
        transcript.push_str(&tail);
        assert!(first.is_some());
        assert!(transcript.lines().count() >= 2, "未得到跨窗口转录");
        eprintln!(
            "首个增量 {:?}，录音 {:?}，刷新 {:?}，{} 字，尾段 {} 字",
            first.unwrap(),
            stop.duration_since(started),
            stop.elapsed(),
            transcript.chars().count(),
            tail.chars().count()
        );
    }
}
