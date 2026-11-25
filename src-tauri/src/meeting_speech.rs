//! 会议语音的可重新下载资源保存在本机应用数据目录，与工作空间备份分离。
use crate::{
    meetings,
    storage::{default_data_root, Store},
    AppState,
};
use fs2::FileExt;
use futures_util::StreamExt;
use reqwest::{header, StatusCode};
use ring::digest;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig, OnlineRecognizer,
    OnlineRecognizerConfig, SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig,
};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};

pub(crate) mod r2t2;

const PREFERENCES_KEY: &str = "meeting-speech";

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechPreferences {
    #[serde(flatten)]
    mode: SpeechMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_model_id: Option<MeetingSpeechModel>,
    #[serde(default, skip_serializing_if = "is_false")]
    local_speaker_diarization: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Default, Deserialize, Serialize)]
#[serde(
    tag = "mode",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
enum SpeechMode {
    #[default]
    Local,
    Api {
        model_id: String,
    },
}

#[tauri::command]
pub fn meeting_speech_preferences(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<SpeechPreferences, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    meetings::workspace(&store, &workspace_id)?;
    read_preferences(&store)
}

fn read_preferences(store: &Store) -> Result<SpeechPreferences, String> {
    let conn = store.conn.lock().map_err(|e| e.to_string())?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [PREFERENCES_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let preferences: SpeechPreferences = match raw {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "会议语音设置格式无效")?,
        None => SpeechPreferences::default(),
    };
    if let Some(model) = preferences.local_model_id {
        model.transcription()?;
    }
    Ok(preferences)
}

pub(crate) fn local_selection(store: &Store) -> Result<(MeetingSpeechModel, bool), String> {
    let preferences = read_preferences(store)?;
    Ok((
        preferences
            .local_model_id
            .unwrap_or_default()
            .transcription()?,
        matches!(preferences.mode, SpeechMode::Local) && preferences.local_speaker_diarization,
    ))
}

#[tauri::command]
pub fn save_meeting_speech_preferences(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    mut value: SpeechPreferences,
) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    meetings::workspace(&store, &workspace_id)?;
    if let SpeechMode::Api { model_id } = &value.mode {
        meetings::enabled_speech_pair(&store.snapshot()?, model_id)?;
    }
    let model = value.local_model_id.unwrap_or_default().transcription()?;
    if matches!(value.mode, SpeechMode::Local) {
        model.check_supported()?;
    }
    if let Some(id) = &value.input_device_id {
        if id.len() > 1024 || id.chars().any(char::is_control) {
            return Err("麦克风设备标识无效".into());
        }
        if id.is_empty() {
            value.input_device_id = None;
        }
    }
    if !matches!(value.mode, SpeechMode::Local) {
        value.local_speaker_diarization = false;
    }
    let raw = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    store.conn.lock().map_err(|e| e.to_string())?
        .execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![PREFERENCES_KEY, raw],
        )
        .map_err(|e| e.to_string())?;
    drop(store);
    if matches!(value.mode, SpeechMode::Local) {
        if let Ok(path) = model_path(&app, model) {
            let _ = prepare_in_background(model, path);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn prepare_meeting_speech_model(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    let preferences = meeting_speech_preferences(state, workspace_id)?;
    if matches!(preferences.mode, SpeechMode::Local) {
        let model = preferences
            .local_model_id
            .unwrap_or_default()
            .transcription()?;
        model.check_supported()?;
        prepare_in_background(model, model_path(&app, model)?)?;
    }
    Ok(())
}

const RELEASE: &str =
    "sherpa-onnx-x-asr-160ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05";
const RELEASE_480: &str =
    "sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05";
const ZIPFORMER_ZH_RELEASE: &str = "sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30";
const ZIPFORMER_ZH_SOURCE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30/resolve/ad658fa0201659a09ea3c176129a191c77ecae8f";
const SPEAKER_RELEASE: &str = "pyannote3-campplus-zh-en-v1";
const ASR_PACKAGE: ModelFile = ModelFile {
    source: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"),
    name: concat!(
        "sherpa-onnx-x-asr-160ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05",
        ".tar.bz2"
    ),
    bytes: 133_898_007,
    sha256: "8a6fca056e1a342546edd78be4d50274e2c01898e7b8ae8fc336f6410319c399",
};
const FILES: [ModelFile; 4] = [
    ModelFile {
        source: None,
        name: "encoder.int8.onnx",
        bytes: 155_278_641,
        sha256: "68633dced8b8891be5b9309496cd939bbeba9188f1c1774957196f11a491aa25",
    },
    ModelFile {
        source: None,
        name: "decoder.onnx",
        bytes: 11_309_084,
        sha256: "a1cbc9eac2d5e3fb6617a218c67ad6daaa7f4e0fd225f08b2c22ab0413c8c257",
    },
    ModelFile {
        source: None,
        name: "joiner.int8.onnx",
        bytes: 2_581_422,
        sha256: "aedb7fa697b2ab43f20499826fff7c997eea7d67db77be97769aeeeb726e63b3",
    },
    ModelFile {
        source: None,
        name: "tokens.txt",
        bytes: 58_806,
        sha256: "b818a60878b9aae978cbb8ad594acbd403d76d1af2e31ef4197c84e2dbdba27c",
    },
];
const ASR_480_PACKAGE: ModelFile = ModelFile {
    name: concat!(
        "sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05",
        ".tar.bz2"
    ),
    bytes: 133_895_136,
    sha256: "fa5f63d618e5a01526e275a358bb7772e403f84808a4769fba52cffd8160bf74",
    ..ASR_PACKAGE
};
const ASR_480_FILES: [ModelFile; 4] = [
    ModelFile {
        sha256: "908596dcc137a73b95be908ca55e88caa1b3dbbe8027c171615f4b0609c5eb1e",
        ..FILES[0]
    },
    FILES[1],
    FILES[2],
    FILES[3],
];
const ZIPFORMER_ZH_FILES: [ModelFile; 4] = [
    ModelFile {
        source: Some(ZIPFORMER_ZH_SOURCE),
        name: "encoder.int8.onnx",
        bytes: 161_141_793,
        sha256: "5ac51e27981bb4dab01bb9be4958453ba50c3b61c063ddda0eab23fd3671aa4f",
    },
    ModelFile {
        source: Some(ZIPFORMER_ZH_SOURCE),
        name: "decoder.onnx",
        bytes: 5_165_083,
        sha256: "06522ad63cec0fdf6809f4e1db9bb4f7d710c34582e3b35db62ac60eccafac7e",
    },
    ModelFile {
        source: Some(ZIPFORMER_ZH_SOURCE),
        name: "joiner.int8.onnx",
        bytes: 1_033_416,
        sha256: "b34584dc6f561089e1d747fedebb3765f2caa72c927ef54d7ca55e5ae40a814b",
    },
    ModelFile {
        source: Some(ZIPFORMER_ZH_SOURCE),
        name: "tokens.txt",
        bytes: 20_628,
        sha256: "6193c7ea1c96d0d9a1e9652789b40d13a8a913b434a5451e93158f5a09fd6652",
    },
];

#[derive(Clone, Copy)]
struct ModelFile {
    source: Option<&'static str>,
    name: &'static str,
    bytes: u64,
    sha256: &'static str,
}

enum Download {
    Idle,
    Running(u64),
    Failed(String),
}
static DOWNLOAD: Mutex<Download> = Mutex::new(Download::Idle);
static DOWNLOAD_480: Mutex<Download> = Mutex::new(Download::Idle);
static ZIPFORMER_ZH_DOWNLOAD: Mutex<Download> = Mutex::new(Download::Idle);
static R2T2_DOWNLOAD: Mutex<Download> = Mutex::new(Download::Idle);
static QWEN3_ASR_06B_DOWNLOAD: Mutex<Download> = Mutex::new(Download::Idle);
static SPEAKER_DOWNLOAD: Mutex<Download> = Mutex::new(Download::Idle);
const SPEAKER_FILES: [ModelFile; 2] = [
    ModelFile {
        source: Some("https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/9403a6902bb58e3d5ae8c7e77c3422de279db2e0"),
        name: "model.onnx",
        bytes: 5_992_913,
        sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    },
    ModelFile {
        source: Some("https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/a8055d6daef723a6a4ecf4de233708575bf4994f"),
        name: "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
        bytes: 28_281_165,
        sha256: "53cab20ba8f2eb2f6e2d89ecc18e88ef3222169b61b4419cbc13666744a4574b",
    },
];

enum Engine {
    Cold,
    Loading,
    Ready(Arc<OnlineRecognizer>),
    Failed(String),
}

struct CachedEngine {
    target: Option<(MeetingSpeechModel, PathBuf)>,
    state: Engine,
}

impl CachedEngine {
    fn matches(&self, model: MeetingSpeechModel, path: &Path) -> bool {
        self.target
            .as_ref()
            .is_some_and(|(loaded_model, loaded_path)| {
                *loaded_model == model && loaded_path == path
            })
    }

    fn loading(&mut self, model: MeetingSpeechModel, path: &Path) {
        self.target = Some((model, path.into()));
        self.state = Engine::Loading;
    }
}

static ENGINE: Mutex<CachedEngine> = Mutex::new(CachedEngine {
    target: None,
    state: Engine::Cold,
});
static ENGINE_CHANGED: Condvar = Condvar::new();

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    state: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
    runtime: &'static str,
    runtime_error: Option<String>,
    model_path: PathBuf,
    can_uninstall: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsupported_reason: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MeetingSpeechModel {
    #[default]
    Transcription,
    #[serde(rename = "x-asr-480")]
    XAsr480,
    #[serde(rename = "zipformer-zh")]
    ZipformerZh,
    R2t2,
    #[serde(rename = "qwen3-asr-0.6b")]
    Qwen3Asr06b,
    Speaker,
}

impl MeetingSpeechModel {
    pub(crate) fn audio_cpp_model(self) -> Option<r2t2::AudioCppModel> {
        match self {
            Self::R2t2 => Some(r2t2::AudioCppModel::R2t2),
            Self::Qwen3Asr06b => Some(r2t2::AudioCppModel::Qwen3Asr06b),
            _ => None,
        }
    }

    fn directory(self) -> &'static str {
        match self {
            Self::Transcription => RELEASE,
            Self::XAsr480 => RELEASE_480,
            Self::ZipformerZh => ZIPFORMER_ZH_RELEASE,
            Self::R2t2 => "confucius4-r2t2-q4",
            Self::Qwen3Asr06b => "qwen3-asr-0.6b-q8",
            Self::Speaker => SPEAKER_RELEASE,
        }
    }

    fn download(self) -> &'static Mutex<Download> {
        match self {
            Self::Transcription => &DOWNLOAD,
            Self::XAsr480 => &DOWNLOAD_480,
            Self::ZipformerZh => &ZIPFORMER_ZH_DOWNLOAD,
            Self::R2t2 => &R2T2_DOWNLOAD,
            Self::Qwen3Asr06b => &QWEN3_ASR_06B_DOWNLOAD,
            Self::Speaker => &SPEAKER_DOWNLOAD,
        }
    }

    fn files(self) -> &'static [ModelFile] {
        match self {
            Self::Transcription => &FILES,
            Self::XAsr480 => &ASR_480_FILES,
            Self::ZipformerZh => &ZIPFORMER_ZH_FILES,
            Self::R2t2 | Self::Qwen3Asr06b => &[],
            Self::Speaker => &SPEAKER_FILES,
        }
    }

    fn archive(self) -> Option<&'static ModelFile> {
        match self {
            Self::Transcription => Some(&ASR_PACKAGE),
            Self::XAsr480 => Some(&ASR_480_PACKAGE),
            _ => None,
        }
    }

    fn total_bytes(self) -> u64 {
        if let Some(engine_model) = self.audio_cpp_model() {
            return r2t2::total_bytes(engine_model);
        }
        self.archive().map_or_else(
            || self.files().iter().map(|file| file.bytes).sum(),
            |file| file.bytes,
        )
    }

    fn unsupported_reason(self) -> Option<&'static str> {
        if let Some(engine_model) = self.audio_cpp_model() {
            return r2t2::unsupported_reason(engine_model);
        }
        None
    }

    pub(crate) fn check_supported(self) -> Result<(), String> {
        self.unsupported_reason()
            .map_or(Ok(()), |reason| Err(reason.into()))
    }

    fn transcription(self) -> Result<Self, String> {
        if self == Self::Speaker {
            return Err("请选择本机转录模型".into());
        }
        Ok(self)
    }

    fn installed(self, path: &Path) -> bool {
        if let Some(engine_model) = self.audio_cpp_model() {
            return r2t2::is_installed(engine_model, path);
        }
        has_files(path, self.files())
    }
}

fn cache_paths(app: &AppHandle, model: MeetingSpeechModel) -> Result<(PathBuf, PathBuf), String> {
    let models = default_data_root()?.join("models");
    check_cache_directory(&models)?;
    let root = models.join("meeting-speech");
    check_cache_directory(&root)?;
    let legacy_root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "无法定位本机模型目录")?
        .join("speech-models");
    check_cache_directory(&legacy_root)?;
    Ok((
        root.join(model.directory()),
        legacy_root.join(model.directory()),
    ))
}

fn check_cache_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err("会议模型目录格式无效".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(write_error(error)),
    }
}

fn migrate_model(path: &Path, legacy: &Path) -> Result<(), String> {
    if check_cache_directory(path)? || !check_cache_directory(legacy)? {
        return Ok(());
    }
    let _guard = model_lock(path, true)?;
    if !check_cache_directory(path)? && check_cache_directory(legacy)? {
        fs::rename(legacy, path).map_err(|error| format!("无法迁移会议模型到新目录（{error}）"))?;
    }
    Ok(())
}

fn resolved_model_path(app: &AppHandle, model: MeetingSpeechModel) -> Result<PathBuf, String> {
    let (path, legacy) = cache_paths(app, model)?;
    migrate_model(&path, &legacy)?;
    Ok(path)
}

pub(crate) fn model_path(app: &AppHandle, model: MeetingSpeechModel) -> Result<PathBuf, String> {
    resolved_model_path(app, model.transcription()?)
}

pub(crate) fn speaker_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    resolved_model_path(app, MeetingSpeechModel::Speaker)
}

fn has_files(path: &Path, files: &[ModelFile]) -> bool {
    files.iter().all(|file| {
        fs::symlink_metadata(path.join(file.name))
            .is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
    })
}

#[tauri::command]
pub fn meeting_speaker_model_status(app: AppHandle) -> Result<ModelStatus, String> {
    model_status(speaker_model_path(&app)?, MeetingSpeechModel::Speaker)
}

#[tauri::command]
pub fn install_meeting_speaker_model(app: AppHandle) -> Result<(), String> {
    let path = speaker_model_path(&app)?;
    {
        let mut download = SPEAKER_DOWNLOAD.lock().map_err(|_| "模型下载状态不可用")?;
        if matches!(*download, Download::Running(_)) {
            return Ok(());
        }
        *download = Download::Running(0);
    }
    tauri::async_runtime::spawn(async move {
        let result = install(&path, &SPEAKER_FILES, &SPEAKER_DOWNLOAD).await;
        if let Ok(mut download) = SPEAKER_DOWNLOAD.lock() {
            *download = match result {
                Ok(()) => Download::Idle,
                Err(error) => Download::Failed(error),
            };
        }
    });
    Ok(())
}

fn speaker_config(path: &Path) -> Result<OfflineSpeakerDiarizationConfig, String> {
    if matches!(
        *SPEAKER_DOWNLOAD.lock().map_err(|_| "模型下载状态不可用")?,
        Download::Running(_)
    ) || !has_files(path, &SPEAKER_FILES)
    {
        return Err("请先在会议语音设置中下载说话人识别模型".into());
    }
    if !SPEAKER_FILES
        .iter()
        .all(|file| verify(&path.join(file.name), file))
    {
        let error = "说话人识别模型文件校验失败，请重新下载".to_string();
        if let Ok(mut download) = SPEAKER_DOWNLOAD.lock() {
            if matches!(*download, Download::Idle) {
                *download = Download::Failed(error.clone());
            }
        }
        return Err(error);
    }
    let model_file = |file: &ModelFile| {
        path.join(file.name)
            .into_os_string()
            .into_string()
            .map_err(|_| "本机模型目录不是有效的 UTF-8 路径")
    };
    let mut config = OfflineSpeakerDiarizationConfig::default();
    config.segmentation.pyannote.model = Some(model_file(&SPEAKER_FILES[0])?);
    config.segmentation.num_threads = 2;
    config.embedding.model = Some(model_file(&SPEAKER_FILES[1])?);
    config.embedding.num_threads = 2;
    Ok(config)
}

pub(crate) fn load_speaker_model(path: &Path) -> Result<OfflineSpeakerDiarization, String> {
    OfflineSpeakerDiarization::create(&speaker_config(path)?)
        .ok_or_else(|| "无法加载本机说话人识别模型".into())
}

pub(crate) fn load_live_speaker_models(
    path: &Path,
) -> Result<(OfflineSpeakerDiarization, SpeakerEmbeddingExtractor), String> {
    let mut config = speaker_config(path)?;
    config.segmentation.num_threads = 1;
    config.embedding.num_threads = 1;
    let extractor = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
        model: config.embedding.model.clone(),
        num_threads: 1,
        ..Default::default()
    })
    .ok_or("无法加载本机说话人特征模型")?;
    let diarizer =
        OfflineSpeakerDiarization::create(&config).ok_or("无法加载本机说话人识别模型")?;
    Ok((diarizer, extractor))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRecommendation {
    pub model: MeetingSpeechModel,
    pub device_summary: String,
    pub reason: String,
    pub cpu_cores: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_gib: Option<u64>,
    pub accelerator: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceProfile {
    os: &'static str,
    arch: &'static str,
    cpu_cores: usize,
    memory_gib: Option<u64>,
}

fn detect_memory_gib() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut memsize: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let ok = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut memsize as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (ok == 0 && memsize > 0).then(|| (memsize + (1 << 29)) / (1 << 30))
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        }
        let mut status = MemoryStatusEx {
            dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0 && status.ull_total_phys > 0)
            .then(|| (status.ull_total_phys + (1 << 29)) / (1 << 30))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if pages > 0 && page_size > 0 {
            let total = (pages as u64).saturating_mul(page_size as u64);
            Some((total + (1 << 29)) / (1 << 30))
        } else {
            None
        }
    }
}

fn detect_device_profile() -> DeviceProfile {
    DeviceProfile {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        memory_gib: detect_memory_gib(),
    }
}

fn format_device_summary(profile: &DeviceProfile, accelerator: &str) -> String {
    let platform = match (profile.os, profile.arch) {
        ("macos", "aarch64") => "Apple 芯片 Mac",
        ("macos", _) => "Intel Mac",
        ("windows", "x86_64") => "Windows x64",
        ("windows", "aarch64") => "Windows ARM64",
        ("windows", _) => "Windows",
        ("linux", "x86_64") => "Linux x64",
        ("linux", "aarch64") => "Linux ARM64",
        _ => "本机设备",
    };
    let accel_label = if accelerator == "metal" {
        "Metal 加速"
    } else {
        "CPU 推理"
    };
    match profile.memory_gib {
        Some(mem) => format!("{platform} · {} 核 · {mem} GB 内存 · {accel_label}", profile.cpu_cores),
        None => format!("{platform} · {} 核 · {accel_label}", profile.cpu_cores),
    }
}

fn recommend_for_device(profile: &DeviceProfile) -> SpeechRecommendation {
    let is_apple_silicon = profile.os == "macos" && profile.arch == "aarch64";
    let is_windows_x64 = profile.os == "windows" && profile.arch == "x86_64";
    let accelerator = if is_apple_silicon { "metal" } else { "cpu" };
    let device_summary = format_device_summary(profile, accelerator);

    let (model, reason) = if is_apple_silicon {
        if profile.memory_gib.is_none_or(|mem| mem >= 16) {
            (
                MeetingSpeechModel::R2t2,
                "检测到 Apple 芯片（Metal 加速）与充足内存，推荐有道 Confucius4-R2T2，兼顾 320 ms 低延迟与高准确率；若需多语种或中文方言识别，也可选 Qwen3-ASR-0.6B。",
            )
        } else {
            (
                MeetingSpeechModel::Qwen3Asr06b,
                "检测到 Apple 芯片（Metal 加速）但统一内存小于 16 GB，推荐更轻量的 Qwen3-ASR-0.6B，在 8 GB 内存下更省资源且支持多语种与中文方言。",
            )
        }
    } else if is_windows_x64 {
        if profile.cpu_cores >= 8 && profile.memory_gib.unwrap_or(0) >= 16 {
            (
                MeetingSpeechModel::Qwen3Asr06b,
                "检测到 Windows x64 多核与 16 GB 以上内存；当前运行包使用 CPU 推理，0.6B 规模的 Qwen3-ASR-0.6B 较 1.7B 的有道模型更轻快，且支持多语种与中文方言。",
            )
        } else {
            (
                MeetingSpeechModel::Transcription,
                "当前设备 CPU 核心数或内存偏紧，推荐轻量的 X-ASR 160 ms（INT8 CPU，约 134 MB），可稳定保持实时转录速度。",
            )
        }
    } else {
        (
            MeetingSpeechModel::Transcription,
            "当前系统架构不支持 audio.cpp 运行包，推荐跨平台轻量的 X-ASR 160 ms（INT8 CPU，约 134 MB）。",
        )
    };

    SpeechRecommendation {
        model,
        device_summary,
        reason: reason.into(),
        cpu_cores: profile.cpu_cores,
        memory_gib: profile.memory_gib,
        accelerator,
    }
}

#[tauri::command]
pub fn meeting_speech_recommendation() -> SpeechRecommendation {
    recommend_for_device(&detect_device_profile())
}

#[tauri::command]
pub fn meeting_speech_status(
    app: AppHandle,
    model: Option<MeetingSpeechModel>,
) -> Result<ModelStatus, String> {
    let model = model.unwrap_or_default().transcription()?;
    model_status(model_path(&app, model)?, model)
}

fn model_status(path: PathBuf, model: MeetingSpeechModel) -> Result<ModelStatus, String> {
    let download = model.download().lock().map_err(|_| "模型下载状态不可用")?;
    let total = model.total_bytes();
    let unsupported_reason = model.unsupported_reason();
    let (state, downloaded_bytes, error) = match &*download {
        Download::Running(bytes) => ("downloading", *bytes, None),
        Download::Failed(error) => ("error", 0, Some(error.clone())),
        Download::Idle if unsupported_reason.is_none() && model.installed(&path) => {
            ("ready", total, None)
        }
        Download::Idle => ("missing", 0, None),
    };
    let (runtime, runtime_error) = if state == "ready" && model != MeetingSpeechModel::Speaker {
        let engine = ENGINE.lock().map_err(|_| "语音引擎状态不可用")?;
        if engine.matches(model, &path) {
            match &engine.state {
                Engine::Cold => ("cold", None),
                Engine::Loading => ("loading", None),
                Engine::Ready(_) => ("ready", None),
                Engine::Failed(error) => ("error", Some(error.clone())),
            }
        } else {
            ("cold", None)
        }
    } else {
        ("cold", None)
    };
    let can_uninstall = state != "downloading" && runtime != "loading" && has_cached_data(&path)?;
    Ok(ModelStatus {
        state,
        downloaded_bytes,
        total_bytes: total,
        error,
        runtime,
        runtime_error,
        model_path: path,
        can_uninstall,
        unsupported_reason,
    })
}

fn has_cached_data(path: &Path) -> Result<bool, String> {
    if !check_cache_directory(path)? {
        return Ok(false);
    }
    for entry in fs::read_dir(path).map_err(write_error)? {
        if entry.map_err(write_error)?.file_name() != ".install.lock" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[tauri::command]
pub async fn uninstall_meeting_speech_model(
    app: AppHandle,
    model: MeetingSpeechModel,
) -> Result<ModelStatus, String> {
    let (path, legacy) = cache_paths(&app, model)?;
    tokio::task::spawn_blocking(move || {
        uninstall_model(&path, &legacy, model)?;
        model_status(path, model)
    })
    .await
    .map_err(|_| "模型卸载任务异常退出")?
}

fn uninstall_model(path: &Path, legacy: &Path, model: MeetingSpeechModel) -> Result<(), String> {
    // 锁位于模型目录外，释放前同一模型不能被重新下载或加载。
    let _guard = model_lock(path, true)?;
    let mut download = model.download().lock().map_err(|_| "模型下载状态不可用")?;
    if matches!(*download, Download::Running(_)) {
        return Err("模型正在下载，请等待下载完成后再卸载".into());
    }
    let legacy_exists = check_cache_directory(legacy)?;
    let installed = check_cache_directory(path)?;
    let mut engine = ENGINE.lock().map_err(|_| "语音引擎状态不可用")?;
    if engine.matches(model, path) {
        if matches!(engine.state, Engine::Loading) {
            return Err("模型正在加载，请等待加载完成后再卸载".into());
        }
        engine.target = None;
        engine.state = Engine::Cold;
    }
    drop(engine);
    if legacy_exists {
        fs::remove_dir_all(legacy).map_err(uninstall_error)?;
    }
    if installed {
        fs::remove_dir_all(path).map_err(uninstall_error)?;
    }
    *download = Download::Idle;
    Ok(())
}

fn uninstall_error(error: io::Error) -> String {
    format!("无法删除会议模型文件，请检查目录权限后重试（{error}）")
}

#[tauri::command]
pub fn install_meeting_speech_model(
    app: AppHandle,
    model: Option<MeetingSpeechModel>,
) -> Result<(), String> {
    let model = model.unwrap_or_default().transcription()?;
    model.check_supported()?;
    let path = model_path(&app, model)?;
    let status = model.download();
    {
        let mut download = status.lock().map_err(|_| "模型下载状态不可用")?;
        if matches!(*download, Download::Running(_)) {
            return Ok(());
        }
        *download = Download::Running(0);
    }
    tauri::async_runtime::spawn(async move {
        let result = if let Some(archive) = model.archive() {
            install_stream_model(&path, model, archive).await
        } else if let Some(engine_model) = model.audio_cpp_model() {
            r2t2::install(engine_model, &path, status).await
        } else {
            install(&path, model.files(), status).await
        };
        let installed = result.is_ok();
        if let Ok(mut download) = status.lock() {
            *download = match result {
                Ok(()) => Download::Idle,
                Err(error) => Download::Failed(error),
            };
        }
        if installed
            && app.try_state::<AppState>().is_some_and(|state| {
                state.store.lock().is_ok_and(|store| {
                    read_preferences(&store).is_ok_and(|value| {
                        matches!(value.mode, SpeechMode::Local)
                            && value.local_model_id.unwrap_or_default() == model
                    })
                })
            })
        {
            let _ = prepare_in_background(model, path);
        }
    });
    Ok(())
}

fn write_error(error: io::Error) -> String {
    format!("无法保存语音模型，请检查磁盘空间和目录权限（{error}）")
}

fn model_lock(path: &Path, exclusive: bool) -> Result<fs::File, String> {
    let parent = path.parent().ok_or("无法定位本机模型目录")?;
    let name = path.file_name().ok_or("无法定位本机模型目录")?;
    fs::create_dir_all(parent).map_err(write_error)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join(format!(".{}.lock", name.to_string_lossy())))
        .map_err(write_error)?;
    let result = if exclusive {
        FileExt::try_lock_exclusive(&lock)
    } else {
        FileExt::try_lock_shared(&lock)
    };
    result.map_err(|_| "会议语音模型正在使用或维护，请结束录音或等待当前操作完成")?;
    Ok(lock)
}

pub(crate) fn model_read_lock(path: &Path) -> Result<fs::File, String> {
    model_lock(path, false)
}

async fn model_installer(path: &Path) -> Result<(fs::File, reqwest::Client), String> {
    let lock = model_lock(path, true)?;
    check_cache_directory(path)?;
    tokio::fs::create_dir_all(path).await.map_err(write_error)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(|_| "无法初始化模型下载")?;
    Ok((lock, client))
}

async fn install_stream_model(
    path: &Path,
    model: MeetingSpeechModel,
    package_file: &'static ModelFile,
) -> Result<(), String> {
    let (_lock, client) = model_installer(path).await?;
    let archive = path.join(package_file.name);
    let status = model.download();
    let verified = tokio::task::spawn_blocking({
        let archive = archive.clone();
        move || verify(&archive, package_file)
    })
    .await
    .map_err(|_| "无法校验语音模型包")?;
    if !verified {
        download_file(&client, path, &archive, package_file, |bytes| {
            report_progress(status, bytes);
        })
        .await?;
    }
    report_progress(status, package_file.bytes);
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        unpack_stream_model(&archive, &path, model)?;
        let _ = fs::remove_file(archive);
        Ok(())
    })
    .await
    .map_err(|_| "语音模型解压任务异常退出")?
}

fn unpack_stream_model(
    archive: &Path,
    path: &Path,
    model: MeetingSpeechModel,
) -> Result<(), String> {
    let staging = tempfile::tempdir_in(path).map_err(write_error)?;
    let input = fs::File::open(archive).map_err(write_error)?;
    let mut package = tar::Archive::new(bzip2::read::BzDecoder::new(input));
    let mut extracted = std::collections::HashSet::new();
    for entry in package.entries().map_err(|_| "语音模型包无法读取")? {
        let mut entry = entry.map_err(|_| "语音模型包内容损坏")?;
        let member = entry.path().map_err(|_| "语音模型包路径无效")?;
        let Some(file) = model
            .files()
            .iter()
            .find(|file| member == Path::new(model.directory()).join(file.name))
        else {
            continue;
        };
        if !entry.header().entry_type().is_file()
            || entry.size() != file.bytes
            || !extracted.insert(file.name)
        {
            return Err("语音模型包内的文件格式无效".into());
        }
        let target = staging.path().join(file.name);
        let mut output = fs::File::create(&target).map_err(write_error)?;
        io::copy(&mut entry, &mut output).map_err(write_error)?;
        output.sync_all().map_err(write_error)?;
        if !verify(&target, file) {
            return Err("语音模型文件未通过完整性校验".into());
        }
    }
    if extracted.len() != model.files().len() {
        return Err("语音模型包缺少必需的文件".into());
    }
    for file in model.files() {
        fs::rename(staging.path().join(file.name), path.join(file.name)).map_err(write_error)?;
    }
    Ok(())
}

async fn install(
    path: &Path,
    files: &'static [ModelFile],
    status: &Mutex<Download>,
) -> Result<(), String> {
    let (_lock, client) = model_installer(path).await?;
    let mut completed = 0;
    for file in files {
        let target = path.join(file.name);
        let verified = tokio::task::spawn_blocking({
            let target = target.clone();
            move || verify(&target, file)
        })
        .await
        .map_err(|_| "无法校验本机模型文件")?;
        if !verified {
            download_file(&client, path, &target, file, |bytes| {
                report_progress(status, completed + bytes)
            })
            .await?;
        }
        completed += file.bytes;
        report_progress(status, completed);
    }
    Ok(())
}

async fn download_file(
    client: &reqwest::Client,
    path: &Path,
    target: &Path,
    file: &'static ModelFile,
    progress: impl Fn(u64),
) -> Result<(), String> {
    let partial = path.join(format!("{}.part", file.name));
    let mut received = match tokio::fs::symlink_metadata(&partial).await {
        Ok(meta) if meta.is_file() && meta.len() <= file.bytes => meta.len(),
        Ok(meta) if meta.is_file() => {
            tokio::fs::remove_file(&partial)
                .await
                .map_err(write_error)?;
            0
        }
        Ok(_) => return Err("模型临时文件类型无效".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(write_error(error)),
    };
    for attempt in 0..3 {
        if received == file.bytes {
            break;
        }
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        progress(received);
        receive_file(client, &partial, file, &progress, &mut received).await?;
    }
    if received != file.bytes {
        return Err("模型下载已中断，检查网络后可继续下载".into());
    }
    let verified = tokio::task::spawn_blocking({
        let partial = partial.clone();
        move || verify(&partial, file)
    })
    .await
    .map_err(|_| "无法校验本机模型文件")?;
    if !verified {
        tokio::fs::remove_file(&partial)
            .await
            .map_err(write_error)?;
        return Err("模型文件未通过完整性校验，请重新下载".into());
    }
    tokio::fs::rename(&partial, target)
        .await
        .map_err(write_error)
}

/// 下载中断只保留暂存文件。续传须核对服务返回的起始位置和总大小。
async fn receive_file(
    client: &reqwest::Client,
    partial: &Path,
    file: &ModelFile,
    progress: &impl Fn(u64),
    received: &mut u64,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let source = file.source.ok_or("模型缺少下载位置")?;
    let mut request = client.get(format!("{source}/{}?download=true", file.name));
    if *received > 0 {
        request = request.header(header::RANGE, format!("bytes={received}-"));
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return Ok(()),
    };
    let resume = match response.status() {
        StatusCode::PARTIAL_CONTENT if *received > 0 => {
            let expected = format!("bytes {received}-{}/{}", file.bytes - 1, file.bytes);
            if response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                != Some(&expected)
            {
                return Err("模型下载源返回的续传位置无效".into());
            }
            true
        }
        StatusCode::OK => {
            // 服务器忽略 Range 时使用完整响应，避免将正文追加到错误位置。
            *received = 0;
            false
        }
        status => {
            return Err(format!(
                "模型下载源返回 HTTP {}，请稍后重试",
                status.as_u16()
            ))
        }
    };
    if response
        .content_length()
        .is_some_and(|length| length != file.bytes - *received)
    {
        return Err("模型文件大小与预期不符，请重新下载".into());
    }
    let mut output = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resume)
        .truncate(!resume)
        .open(partial)
        .await
        .map_err(write_error)?;
    let mut stream = response.bytes_stream();
    while let Some(Ok(bytes)) = stream.next().await {
        if *received + bytes.len() as u64 > file.bytes {
            return Err("模型文件超过预期大小，请重新下载".into());
        }
        output.write_all(&bytes).await.map_err(write_error)?;
        *received += bytes.len() as u64;
        progress(*received);
    }
    output.sync_all().await.map_err(write_error)
}

fn report_progress(status: &Mutex<Download>, bytes: u64) {
    if let Ok(mut download) = status.lock() {
        *download = Download::Running(bytes);
    }
}

fn verify(path: &Path, file: &ModelFile) -> bool {
    if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes) {
        return false;
    }
    let Ok(mut input) = fs::File::open(path) else {
        return false;
    };
    let mut digest = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 64 * 1024];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => digest.update(&buffer[..size]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    hex::encode(digest.finish()) == file.sha256
}

fn prepare_in_background(model: MeetingSpeechModel, path: PathBuf) -> Result<(), String> {
    if model.audio_cpp_model().is_some() {
        return Ok(());
    }
    if !matches!(
        *model.download().lock().map_err(|_| "模型下载状态不可用")?,
        Download::Idle
    ) || !model.installed(&path)
    {
        return Ok(());
    }
    {
        let mut engine = ENGINE.lock().map_err(|_| "语音引擎状态不可用")?;
        if matches!(engine.state, Engine::Loading)
            || (engine.matches(model, &path) && matches!(engine.state, Engine::Ready(_)))
        {
            return Ok(());
        }
        engine.loading(model, &path);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _ = finish_loading(model, &path);
    });
    Ok(())
}

/// 录音和预热共用一次加载。等待发生在解码的阻塞线程，不阻塞 UI 或 SQLite。
pub(crate) fn acquire(
    model: MeetingSpeechModel,
    path: &Path,
) -> Result<Arc<OnlineRecognizer>, String> {
    if !model.installed(path) {
        return Err("请先在「设置 → 会议语音」下载本机转录模型".into());
    }
    let mut engine = ENGINE.lock().map_err(|_| "语音引擎状态不可用")?;
    let mut waited = false;
    loop {
        let same_model = engine.matches(model, path);
        match &engine.state {
            Engine::Ready(recognizer) if same_model => return Ok(recognizer.clone()),
            Engine::Loading => {
                waited |= same_model;
                engine = ENGINE_CHANGED
                    .wait(engine)
                    .map_err(|_| "语音引擎状态不可用")?;
            }
            Engine::Failed(error) if same_model && waited => return Err(error.clone()),
            _ => {
                engine.loading(model, path);
                drop(engine);
                return finish_loading(model, path);
            }
        }
    }
}

fn finish_loading(model: MeetingSpeechModel, path: &Path) -> Result<Arc<OnlineRecognizer>, String> {
    let result = load(model, path).map(Arc::new);
    let mut engine = ENGINE.lock().map_err(|_| "语音引擎状态不可用")?;
    engine.state = match &result {
        Ok(recognizer) => Engine::Ready(recognizer.clone()),
        Err(error) => Engine::Failed(error.clone()),
    };
    ENGINE_CHANGED.notify_all();
    result
}

fn load(model: MeetingSpeechModel, path: &Path) -> Result<OnlineRecognizer, String> {
    let _guard = model_read_lock(path)?;
    {
        let download = model.download().lock().map_err(|_| "模型下载状态不可用")?;
        if matches!(*download, Download::Running(_)) {
            return Err("本机语音模型正在下载，请完成后再开始实时转录".into());
        }
    }
    let files = model.files();
    let [encoder, decoder, joiner, tokens] = files else {
        return Err("本机流式转录模型格式无效".into());
    };
    if !has_files(path, files) {
        return Err("请先在「设置 → 会议语音」下载本机转录模型".into());
    }
    if !files.iter().all(|file| verify(&path.join(file.name), file)) {
        let error = "本机语音模型文件校验失败，请在会议语音设置中重新下载".to_string();
        if let Ok(mut download) = model.download().lock() {
            if matches!(*download, Download::Idle) {
                *download = Download::Failed(error.clone());
            }
        }
        return Err(error);
    }
    let file_path = |file: &ModelFile| path.join(file.name).into_os_string().into_string().ok();
    let mut config = OnlineRecognizerConfig {
        enable_endpoint: true,
        rule1_min_trailing_silence: 2.4,
        rule2_min_trailing_silence: 1.2,
        rule3_min_utterance_length: 20.0,
        // 4 路束搜索减少背景人声下的漏字，同时保留 160 ms 模型的流式输入。
        decoding_method: Some("modified_beam_search".into()),
        max_active_paths: 4,
        ..Default::default()
    };
    config.model_config.transducer.encoder = file_path(encoder);
    config.model_config.transducer.decoder = file_path(decoder);
    config.model_config.transducer.joiner = file_path(joiner);
    config.model_config.tokens = file_path(tokens);
    if config.model_config.tokens.is_none() {
        return Err("本机模型目录不是有效的 UTF-8 路径".into());
    }
    config.model_config.provider = Some("cpu".into());
    config.model_config.num_threads = 2;
    OnlineRecognizer::create(&config).ok_or_else(|| "本机语音模型加载失败".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_legacy_cache_and_uninstalls_only_the_selected_model() {
        let temp = tempfile::tempdir().unwrap();
        let model = MeetingSpeechModel::Speaker;
        let root = temp.path().join("models/meeting-speech");
        let current = root.join(model.directory());
        let legacy = temp.path().join("speech-models").join(model.directory());
        let other = root.join(RELEASE);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(legacy.join("model.onnx.part"), b"pending download").unwrap();
        fs::write(other.join("encoder.int8.onnx"), b"keep").unwrap();

        migrate_model(&current, &legacy).unwrap();
        assert!(!legacy.exists());
        assert_eq!(
            fs::read(current.join("model.onnx.part")).unwrap(),
            b"pending download"
        );
        assert!(model_status(current.clone(), model).unwrap().can_uninstall);

        let in_use = model_read_lock(&current).unwrap();
        assert!(uninstall_model(&current, &legacy, model).is_err());
        assert!(current.exists());
        drop(in_use);

        uninstall_model(&current, &legacy, model).unwrap();
        uninstall_model(&current, &legacy, model).unwrap();
        assert!(!current.exists());
        let status = model_status(current, model).unwrap();
        assert_eq!(status.state, "missing");
        assert!(!status.can_uninstall);
        assert_eq!(fs::read(other.join("encoder.int8.onnx")).unwrap(), b"keep");
    }

    #[tokio::test]
    #[ignore = "下载公开模型和四人中文样例，验证本机多说话人分离"]
    async fn installed_speaker_models_distinguish_public_sample() {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("speakers");
        install(&model, &SPEAKER_FILES, &SPEAKER_DOWNLOAD)
            .await
            .unwrap();
        *SPEAKER_DOWNLOAD.lock().unwrap() = Download::Idle;
        let diarizer = load_speaker_model(&model).unwrap();
        let wav = tmp.path().join("sample.wav");
        if let Some(path) = std::env::var_os("PERCH_PUBLIC_DIARIZATION_SAMPLE") {
            fs::copy(path, &wav).unwrap();
        } else {
            let response = reqwest::get("https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/0-four-speakers-zh.wav")
                .await.unwrap().error_for_status().unwrap();
            fs::write(&wav, response.bytes().await.unwrap()).unwrap();
        }
        let audio = sherpa_onnx::Wave::read(wav.to_str().unwrap()).unwrap();
        assert_eq!(audio.sample_rate(), 16_000);
        let result = diarizer.process(audio.samples()).unwrap();
        eprintln!(
            "模型识别到 {} 位说话人，{} 段发言",
            result.num_speakers(),
            result.num_segments()
        );
        assert!(result.num_speakers() >= 2);
    }

    #[test]
    fn speech_preferences_read_existing_records_and_roundtrip_device() {
        let saved: SpeechPreferences = serde_json::from_value(serde_json::json!({
            "mode": "api", "modelId": "speech-model"
        }))
        .unwrap();
        assert!(matches!(saved.mode, SpeechMode::Api { .. }));
        assert!(saved.input_device_id.is_none());

        let expected = serde_json::json!({"mode": "local", "inputDeviceId": "fixture-mic"});
        let saved: SpeechPreferences = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(
            saved.local_model_id.unwrap_or_default(),
            MeetingSpeechModel::Transcription
        );
        assert_eq!(serde_json::to_value(saved).unwrap(), expected);

        for id in [
            "transcription",
            "x-asr-480",
            "zipformer-zh",
            "r2t2",
            "qwen3-asr-0.6b",
        ] {
            let expected = serde_json::json!({
                "mode": "local", "localModelId": id, "localSpeakerDiarization": true
            });
            let saved: SpeechPreferences = serde_json::from_value(expected.clone()).unwrap();
            assert!(saved.local_model_id.unwrap().transcription().is_ok());
            assert_eq!(serde_json::to_value(saved).unwrap(), expected);
        }
        assert!(MeetingSpeechModel::Speaker.transcription().is_err());
    }

    #[test]
    fn device_recommendation_selects_best_model_for_hardware() {
        let mac_16g = recommend_for_device(&DeviceProfile {
            os: "macos",
            arch: "aarch64",
            cpu_cores: 10,
            memory_gib: Some(16),
        });
        assert_eq!(mac_16g.model, MeetingSpeechModel::R2t2);
        assert_eq!(mac_16g.accelerator, "metal");
        assert!(mac_16g.device_summary.contains("16 GB"));

        let mac_8g = recommend_for_device(&DeviceProfile {
            os: "macos",
            arch: "aarch64",
            cpu_cores: 8,
            memory_gib: Some(8),
        });
        assert_eq!(mac_8g.model, MeetingSpeechModel::Qwen3Asr06b);
        assert_eq!(mac_8g.accelerator, "metal");

        let win_strong = recommend_for_device(&DeviceProfile {
            os: "windows",
            arch: "x86_64",
            cpu_cores: 12,
            memory_gib: Some(16),
        });
        assert_eq!(win_strong.model, MeetingSpeechModel::Qwen3Asr06b);
        assert_eq!(win_strong.accelerator, "cpu");

        let win_light = recommend_for_device(&DeviceProfile {
            os: "windows",
            arch: "x86_64",
            cpu_cores: 4,
            memory_gib: Some(8),
        });
        assert_eq!(win_light.model, MeetingSpeechModel::Transcription);

        let intel_mac = recommend_for_device(&DeviceProfile {
            os: "macos",
            arch: "x86_64",
            cpu_cores: 8,
            memory_gib: Some(32),
        });
        assert_eq!(intel_mac.model, MeetingSpeechModel::Transcription);
    }

    #[test]
    #[ignore = "使用三个已校验的本机模型，验证切换时重新加载并保留录音中识别"]
    fn switching_native_recognizers_uses_the_selected_model() {
        let wave = sherpa_onnx::Wave::read(&std::env::var("PERCH_TEST_PUBLIC_SPEECH_WAV").unwrap())
            .unwrap();
        assert_eq!(wave.sample_rate(), 16_000);
        let mut previous = None;
        for (model, variable) in [
            (
                MeetingSpeechModel::Transcription,
                "PERCH_TEST_SPEECH_MODEL_DIR",
            ),
            (MeetingSpeechModel::XAsr480, "PERCH_TEST_SPEECH_480_DIR"),
            (
                MeetingSpeechModel::ZipformerZh,
                "PERCH_TEST_SPEECH_ZIPFORMER_DIR",
            ),
        ] {
            let path = PathBuf::from(std::env::var_os(variable).unwrap());
            let recognizer = acquire(model, &path).unwrap();
            assert!(ENGINE.lock().unwrap().matches(model, &path));
            if let Some(previous) = previous {
                assert!(!Arc::ptr_eq(&previous, &recognizer));
            }
            let stream = recognizer.create_stream();
            let mut live = false;
            for chunk in wave.samples().chunks(2_560) {
                stream.accept_waveform(16_000, chunk);
                while recognizer.is_ready(&stream) {
                    recognizer.decode(&stream);
                }
                live |= recognizer
                    .get_result(&stream)
                    .is_some_and(|result| !result.text.is_empty());
            }
            assert!(live, "{model:?} 在录音结束前应产生文字");
            previous = Some(recognizer);
        }
    }

    #[test]
    #[ignore = "使用已安装模型验证预加载与进程内复用"]
    fn prewarm_and_reuse_installed_recognizer() {
        let path = PathBuf::from(std::env::var_os("PERCH_TEST_SPEECH_MODEL_DIR").unwrap());
        let started = std::time::Instant::now();
        let model = MeetingSpeechModel::Transcription;
        prepare_in_background(model, path.clone()).unwrap();
        let warmed = acquire(model, &path).unwrap();
        assert!(matches!(ENGINE.lock().unwrap().state, Engine::Ready(_)));
        let cold_elapsed = started.elapsed();
        let started = std::time::Instant::now();
        let next = acquire(model, &path).unwrap();
        assert!(Arc::ptr_eq(&warmed, &next));
        eprintln!(
            "首次校验和加载 {cold_elapsed:?}，复用识别器 {:?}",
            started.elapsed()
        );
        let stream = next.create_stream();
        stream.accept_waveform(16_000, &[0.0; 16_000]);
        stream.input_finished();
        while next.is_ready(&stream) {
            next.decode(&stream);
        }
        assert!(next.get_result(&stream).is_some());
    }

    #[tokio::test]
    #[ignore = "下载约 128 MiB 的公开模型包，用于验收原生推理"]
    async fn download_and_decode_public_sample() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model");
        if let Some(archive) = std::env::var_os("PERCH_TEST_XASR_ARCHIVE") {
            fs::create_dir(&path).unwrap();
            fs::copy(archive, path.join(ASR_PACKAGE.name)).unwrap();
        }
        let model = MeetingSpeechModel::Transcription;
        install_stream_model(&path, model, &ASR_PACKAGE)
            .await
            .unwrap();
        *DOWNLOAD.lock().unwrap() = Download::Idle;
        let recognizer = load(model, &path).unwrap();
        let source = "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/98590b7ed6443e77b714204da2757d75e1a642f4/test_wavs/1.wav";
        let audio = reqwest::get(source)
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let audio_path = temp.path().join("sample.wav");
        fs::write(&audio_path, audio).unwrap();
        let wave = sherpa_onnx::Wave::read(audio_path.to_str().unwrap()).unwrap();
        let stream = recognizer.create_stream();
        let frame_size = wave.sample_rate() as usize * 160 / 1000;
        let mut first_text_ms = None;
        let mut capture_ms = 0;
        let mut speech_start_ms = None;
        let mut decode_time = std::time::Duration::ZERO;
        for samples in wave.samples().chunks(frame_size) {
            if speech_start_ms.is_none()
                && samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32 > 0.0001
            {
                speech_start_ms = Some(capture_ms);
            }
            capture_ms += samples.len() * 1000 / wave.sample_rate() as usize;
            let start = std::time::Instant::now();
            stream.accept_waveform(wave.sample_rate(), samples);
            while recognizer.is_ready(&stream) {
                recognizer.decode(&stream);
            }
            if first_text_ms.is_none()
                && recognizer
                    .get_result(&stream)
                    .is_some_and(|r| !r.text.trim().is_empty())
            {
                first_text_ms = Some(capture_ms);
            }
            decode_time += start.elapsed();
        }
        let first = first_text_ms.expect("录音尚未结束时应产生识别结果");
        eprintln!(
            "首字对应音频 {first} ms，样例音量起点 {} ms，总推理 {decode_time:?}",
            speech_start_ms.unwrap_or(0)
        );
        stream.accept_waveform(wave.sample_rate(), &[0.0; 8_000]);
        stream.input_finished();
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }
        let result = recognizer.get_result(&stream).unwrap();
        assert!(
            result.text.contains("这是") && result.text.to_uppercase().contains("ALWAYS"),
            "未识别样例中的中英文片段：{}",
            result.text
        );
    }
}
