//! 用有限长度的滚动音频判断发言区间，整个录音会话内复用说话人编号。
use super::{Events, Session, SAMPLE_RATE};
use crate::meeting_speech;
use sherpa_onnx::{OfflineSpeakerDiarization, RecognizerResult, SpeakerEmbeddingExtractor};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::Ordering, mpsc, Arc},
    time::Duration,
};
use tokio::sync::watch;

const SAMPLES_PER_MS: usize = SAMPLE_RATE as usize / 1000;
const CONTEXT_MS: i64 = 6_000;
const FIRST_WINDOW_MS: i64 = 1_800;
const HOP_MS: i64 = 800;
const LOOKAHEAD_MS: i64 = 600;
const REVISION_MS: i64 = 2_400;
const MIN_VOICE_MS: i64 = 700;
const MAX_SPEAKERS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: u32,
}

pub(super) struct Word {
    pub byte: usize,
    pub ms: i64,
}

pub(super) fn word_times(text: &str, result: &RecognizerResult, elapsed_ms: i64) -> Vec<Word> {
    let Some(times) = result.timestamps.as_ref() else {
        return Vec::new();
    };
    let Some(start) = result.start_time else {
        return Vec::new();
    };
    if !start.is_finite() || times.len() != result.tokens.len() {
        return Vec::new();
    }
    let mut cursor = 0;
    let mut words: Vec<Word> = Vec::new();
    for (token, time) in result.tokens.iter().zip(times) {
        let token = token.trim_start_matches('▁').trim();
        if token.is_empty() {
            continue;
        }
        let remaining = &text[cursor..];
        let Some(relative) = remaining.find(token) else {
            // 模型的文字与 token 无法可靠对应时保留原文。
            return Vec::new();
        };
        let ms = ((start + time) * 1000.0).round() as i64;
        if remaining[..relative].chars().any(char::is_alphanumeric)
            || !time.is_finite()
            || ms < 0
            || ms > elapsed_ms
            || words.last().is_some_and(|word| word.ms > ms)
        {
            return Vec::new();
        }
        let byte = cursor + relative;
        cursor = byte + token.len();
        let is_ascii_continuation = relative == 0
            && text[..byte].ends_with(|ch: char| ch.is_ascii_alphanumeric())
            && token.starts_with(|ch: char| ch.is_ascii_alphanumeric());
        if token.chars().any(char::is_alphanumeric) && !is_ascii_continuation {
            words.push(Word { byte, ms });
        }
    }
    if text[cursor..].chars().any(char::is_alphanumeric) {
        return Vec::new();
    }
    words
}

pub(super) fn speaker_key(id: u32) -> String {
    format!("local-speaker:{id}")
}

pub(super) fn speaker_at(turns: &[Turn], ms: i64) -> Option<u32> {
    turns
        .get(turns.partition_point(|turn| turn.end_ms <= ms))
        .filter(|turn| turn.start_ms <= ms)
        .map(|turn| turn.speaker)
}

struct Window {
    start_ms: i64,
    end_ms: i64,
    samples: Vec<f32>,
    final_window: bool,
}

pub(super) struct Worker {
    tx: watch::Sender<Option<Arc<Window>>>,
    finished: mpsc::Receiver<()>,
    samples: Vec<f32>,
    last_ms: i64,
}

impl Worker {
    pub fn start(path: PathBuf, session: Arc<Session>, events: Events, last_id: u32) -> Self {
        let (tx, rx) = watch::channel(None);
        let (done, finished) = mpsc::channel();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            if let Err(error) = run(path, &session, &events, last_id, rx, runtime) {
                events.send("speakerError", &error);
            }
            let _ = done.send(());
        });
        Self {
            tx,
            finished,
            samples: Vec::new(),
            last_ms: 0,
        }
    }

    pub fn feed(&mut self, samples: &[f32], elapsed_ms: i64) {
        if self.tx.is_closed() {
            return;
        }
        self.samples.extend_from_slice(samples);
        if elapsed_ms >= FIRST_WINDOW_MS && elapsed_ms - self.last_ms >= HOP_MS {
            self.submit(elapsed_ms, false);
        }
    }

    pub fn finish(mut self, elapsed_ms: i64) {
        if elapsed_ms >= MIN_VOICE_MS && !self.tx.is_closed() {
            self.submit(elapsed_ms, true);
            // 慢设备上的说话人分析只短暂等待，录音和文字优先保存。
            let _ = self.finished.recv_timeout(Duration::from_secs(3));
        }
    }

    fn submit(&mut self, end_ms: i64, final_window: bool) {
        let keep = CONTEXT_MS as usize * SAMPLES_PER_MS;
        if self.samples.len() > keep {
            self.samples.drain(..self.samples.len() - keep);
        }
        self.last_ms = end_ms;
        // 只保留最新窗口，避免推理落后时积压录音线程。
        self.tx.send_replace(Some(Arc::new(Window {
            start_ms: end_ms - (self.samples.len() / SAMPLES_PER_MS) as i64,
            end_ms,
            samples: self.samples.clone(),
            final_window,
        })));
    }
}

fn run(
    path: PathBuf,
    session: &Session,
    events: &Events,
    last_id: u32,
    mut rx: watch::Receiver<Option<Arc<Window>>>,
    runtime: tokio::runtime::Handle,
) -> Result<(), String> {
    let _model_guard = meeting_speech::model_read_lock(&path)?;
    let (diarizer, extractor) = meeting_speech::load_live_speaker_models(&path)?;
    let mut roster = Roster::new(last_id);
    let mut confirmed_ms = 0;
    while !session.cancelled.load(Ordering::Acquire) && runtime.block_on(rx.changed()).is_ok() {
        let window = rx.borrow_and_update().clone();
        let Some(window) = window else {
            continue;
        };
        let turns = analyze_window(&diarizer, &extractor, &window, &mut roster);
        let mut current = session.result.lock().map_err(|_| "实时转录状态不可用")?;
        if current.ended || session.cancelled.load(Ordering::Acquire) {
            break;
        }
        let changed = update_turns(&mut current.turns, turns, &window, &mut confirmed_ms);
        if changed && (!current.segments.is_empty() || current.partial.is_some()) {
            events.preview(&current);
        }
        if window.final_window {
            break;
        }
    }
    Ok(())
}

fn update_turns(
    history: &mut Vec<Turn>,
    turns: Vec<Turn>,
    window: &Window,
    confirmed_ms: &mut i64,
) -> bool {
    // 最近的标签允许随上下文修正，较早的已确认标签保持稳定。
    let keep_through = (*confirmed_ms).max(window.start_ms);
    let keep = history.partition_point(|turn| turn.end_ms <= keep_through);
    let previous = history.split_off(keep);
    let mut replacement = Vec::<Turn>::new();
    if let Some(overlap) = previous.first().filter(|turn| turn.start_ms < keep_through) {
        replacement.push(Turn {
            end_ms: keep_through,
            ..*overlap
        });
    }
    let through_ms = window.end_ms - if window.final_window { 0 } else { LOOKAHEAD_MS };
    for turn in turns {
        let start_ms = turn.start_ms.max(keep_through);
        let end_ms = turn.end_ms.min(through_ms);
        if start_ms >= end_ms {
            continue;
        }
        if let Some(last) = replacement
            .last_mut()
            .filter(|last| last.end_ms == start_ms && last.speaker == turn.speaker)
        {
            last.end_ms = end_ms;
        } else {
            replacement.push(Turn {
                start_ms,
                end_ms,
                ..turn
            });
        }
    }
    let changed = previous != replacement;
    for turn in replacement {
        if let Some(last) = history
            .last_mut()
            .filter(|last| last.end_ms == turn.start_ms && last.speaker == turn.speaker)
        {
            last.end_ms = turn.end_ms;
        } else {
            history.push(turn);
        }
    }
    *confirmed_ms = keep_through.max(window.end_ms - REVISION_MS);
    changed
}

fn analyze_window(
    diarizer: &OfflineSpeakerDiarization,
    extractor: &SpeakerEmbeddingExtractor,
    window: &Window,
    roster: &mut Roster,
) -> Vec<Turn> {
    let Some(result) = diarizer.process(&window.samples) else {
        return Vec::new();
    };
    let regions = result
        .sort_by_start_time()
        .into_iter()
        .filter(|part| part.start.is_finite() && part.end.is_finite())
        .filter_map(|part| {
            let duration_ms = window.end_ms - window.start_ms;
            let start = ((part.start * 1000.0).round() as i64).clamp(0, duration_ms);
            let end = ((part.end * 1000.0).round() as i64).clamp(0, duration_ms);
            (end > start).then_some((start, end, part.speaker))
        })
        .collect::<Vec<_>>();
    let clean = exclusive_regions(&regions);
    let mut identities = HashMap::new();
    for &(_, _, cluster) in &clean {
        identities.entry(cluster).or_insert_with(|| {
            let (from, to, _) = *clean
                .iter()
                .filter(|&&(from, to, id)| id == cluster && to - from >= MIN_VOICE_MS)
                .max_by_key(|&&(from, to, _)| to - from)?;
            let stream = extractor.create_stream()?;
            let (from, to) = (from as usize, to as usize);
            stream.accept_waveform(
                SAMPLE_RATE,
                &window.samples[from * SAMPLES_PER_MS..to * SAMPLES_PER_MS],
            );
            stream.input_finished();
            if !extractor.is_ready(&stream) {
                return None;
            }
            roster.identify(extractor.compute(&stream)?)
        });
    }
    clean
        .into_iter()
        .filter_map(|(start, end, cluster)| {
            Some(Turn {
                start_ms: window.start_ms + start,
                end_ms: window.start_ms + end,
                speaker: identities[&cluster]?,
            })
        })
        .collect()
}

// 两个及以上发言重叠的区域不分配给任一方。
fn exclusive_regions(regions: &[(i64, i64, i32)]) -> Vec<(i64, i64, i32)> {
    let mut edges: Vec<_> = regions
        .iter()
        .flat_map(|&(start, end, _)| [start, end])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut clean = Vec::new();
    for edge in edges.windows(2) {
        let mut speakers = regions
            .iter()
            .filter(|&&(start, end, _)| start < edge[1] && end > edge[0]);
        if let Some(&(_, _, speaker)) = speakers.next() {
            if speakers.all(|&(_, _, other)| other == speaker) {
                if let Some((_, end, _)) = clean
                    .last_mut()
                    .filter(|(_, end, previous)| *end == edge[0] && *previous == speaker)
                {
                    *end = edge[1];
                } else {
                    clean.push((edge[0], edge[1], speaker));
                }
            }
        }
    }
    clean
}

struct Roster {
    last_id: u32,
    voices: Vec<(u32, Vec<f32>)>,
}

impl Roster {
    fn new(last_id: u32) -> Self {
        Self {
            last_id,
            voices: Vec::new(),
        }
    }

    fn identify(&mut self, mut voice: Vec<f32>) -> Option<u32> {
        normalize(&mut voice)?;
        let mut best = (0, -1.0_f32);
        let mut next = -1.0_f32;
        for (index, (_, profile)) in self.voices.iter().enumerate() {
            let score: f32 = profile.iter().zip(&voice).map(|(a, b)| a * b).sum();
            if score > best.1 {
                next = best.1;
                best = (index, score);
            } else {
                next = next.max(score);
            }
        }
        if best.1 >= 0.56 && best.1 - next >= 0.08 {
            let (id, profile) = &mut self.voices[best.0];
            for (mean, value) in profile.iter_mut().zip(voice) {
                *mean = 0.85 * *mean + 0.15 * value;
            }
            normalize(profile)?;
            return Some(*id);
        }
        if best.1 > 0.40 || self.voices.len() >= MAX_SPEAKERS {
            return None;
        }
        self.last_id = self.last_id.checked_add(1)?;
        self.voices.push((self.last_id, voice));
        Some(self.last_id)
    }
}

fn normalize(vector: &mut [f32]) -> Option<()> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return None;
    }
    vector.iter_mut().for_each(|value| *value /= norm);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_reuses_speaker_on_return_and_leaves_uncertain_voices_unlabelled() {
        let mut roster = Roster::new(2);
        assert_eq!(roster.identify(vec![1.0, 0.0, 0.0]), Some(3));
        assert_eq!(roster.identify(vec![0.0, 1.0, 0.0]), Some(4));
        assert_eq!(roster.identify(vec![0.92, 0.05, 0.05]), Some(3));
        assert_eq!(roster.identify(vec![0.7, 0.7, 0.0]), None);
        assert_eq!(roster.identify(vec![0.0, 0.0, 0.0]), None);
        assert_eq!(
            exclusive_regions(&[(0, 1400, 0), (900, 1700, 1), (2000, 2700, 0)]),
            vec![(0, 900, 0), (1400, 1700, 1), (2000, 2700, 0)]
        );
    }

    #[test]
    #[ignore = "需要已安装的会议模型和 sherpa-onnx 公开四人中文 WAV 样例"]
    fn live_models_reuse_a_speaker_in_streaming_windows() {
        let path = PathBuf::from(std::env::var_os("PERCH_TEST_SPEAKER_MODEL_DIR").unwrap());
        let file = std::env::var("PERCH_PUBLIC_DIARIZATION_SAMPLE").unwrap();
        let wave = sherpa_onnx::Wave::read(&file).unwrap();
        assert_eq!(wave.sample_rate(), SAMPLE_RATE);
        let (diarizer, extractor) = meeting_speech::load_live_speaker_models(&path).unwrap();
        let full = diarizer
            .process(wave.samples())
            .unwrap()
            .sort_by_start_time();
        let region = |part: &sherpa_onnx::OfflineSpeakerDiarizationSegment| {
            let start = (part.start * SAMPLE_RATE as f32) as usize;
            let end = (part.end * SAMPLE_RATE as f32) as usize;
            &wave.samples()[start..end]
        };
        let a = full
            .iter()
            .find(|part| part.end - part.start > 2.5)
            .unwrap();
        let b = full
            .iter()
            .find(|part| part.speaker != a.speaker && part.end - part.start > 2.5)
            .unwrap();
        let a_again = full
            .iter()
            .find(|part| {
                part.speaker == a.speaker && part.start > b.end && part.end - part.start > 2.5
            })
            .unwrap();
        let mut samples = Vec::new();
        let span = 2_400 * SAMPLES_PER_MS;
        for voice in [region(a), region(b), region(a_again)] {
            samples.extend_from_slice(&voice[..span]);
            samples.extend_from_slice(&[0.0; 3_200]);
        }
        let mut roster = Roster::new(0);
        let start = std::time::Instant::now();
        let mut max_runtime = Duration::ZERO;
        let mut turns = Vec::new();
        let mut confirmed = 0;
        for end_ms in (FIRST_WINDOW_MS..=7800).step_by(HOP_MS as usize) {
            let start_ms = (end_ms - CONTEXT_MS).max(0);
            let window = Window {
                start_ms,
                end_ms,
                final_window: false,
                samples: samples
                    [(start_ms as usize) * SAMPLES_PER_MS..(end_ms as usize) * SAMPLES_PER_MS]
                    .to_vec(),
            };
            let before = std::time::Instant::now();
            let speech = analyze_window(&diarizer, &extractor, &window, &mut roster);
            update_turns(&mut turns, speech, &window, &mut confirmed);
            max_runtime = max_runtime.max(before.elapsed());
        }
        eprintln!(
            "滚动分析 {turns:?}，总耗时 {:?}，单次最长 {max_runtime:?}",
            start.elapsed()
        );
        let first = speaker_at(&turns, 1500).expect("录音期间应识别到第一位说话人");
        let second = speaker_at(&turns, 3900).expect("录音期间应识别到第二位说话人");
        assert_ne!(first, second);
        assert_eq!(speaker_at(&turns, 6500), Some(first));

        let speech_path = PathBuf::from(std::env::var_os("PERCH_TEST_SPEECH_MODEL_DIR").unwrap());
        let recognizer = meeting_speech::acquire(&speech_path).unwrap();
        let stream = recognizer.create_stream();
        for frame in samples.chunks(160 * SAMPLES_PER_MS) {
            stream.accept_waveform(SAMPLE_RATE, frame);
            while recognizer.is_ready(&stream) {
                recognizer.decode(&stream);
            }
        }
        let result = recognizer.get_result(&stream).unwrap();
        let text = result.text.trim();
        let words = word_times(text, &result, 7800);
        let segment = super::super::Segment {
            start_ms: 0,
            end_ms: 7800,
            text: text.into(),
            words,
        };
        let lines = segment.split(&turns);
        let mut ids = lines
            .iter()
            .filter_map(|line| line.speaker.as_deref())
            .collect::<Vec<_>>();
        ids.dedup();
        assert_eq!(
            ids,
            [speaker_key(first), speaker_key(second), speaker_key(first)]
        );
        assert_eq!(lines.iter().map(|line| line.text).collect::<String>(), text);
    }
}
