//! 按本机说话人分离结果标注已有转录，保留正文和人工标记。
use super::{context, db, read, stale_analysis, workspace, write_metadata, CONFLICT, MAX_PCM};
use crate::{data::text, meeting_realtime, meeting_speech, AppState};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tauri::{AppHandle, State};
use tokio::sync::Semaphore;

static RUN: Semaphore = Semaphore::const_new(1);
const LOCAL_PREFIX: &str = "local-speaker:";

struct Turn {
    start: i64,
    end: i64,
    speaker: i32,
}

#[tauri::command]
pub async fn meeting_diarize(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    id: String,
    revision: i64,
) -> Result<Value, String> {
    let slot = RUN
        .try_acquire()
        .map_err(|_| "已有会议正在识别说话人，请稍后重试")?;
    let model_path = meeting_speech::speaker_model_path(&app)?;
    let (generation, meeting, pcm) = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        workspace(&store, &workspace_id)?;
        meeting_realtime::ensure_idle(&workspace_id, &id)?;
        let conn = store.conn.lock().map_err(|e| e.to_string())?;
        let meeting = read(&conn, &id)?;
        if meeting["revision"].as_i64() != Some(revision) {
            return Err(CONFLICT.into());
        }
        if meeting["segments"].as_array().is_none_or(Vec::is_empty) {
            return Err("请先完成会议转录".into());
        }
        let size: usize = conn
            .query_row(
                "SELECT COALESCE(SUM(length(pcm)),0) FROM meeting_audio_chunks WHERE meeting_id=?1",
                [&id],
                |row| row.get(0),
            )
            .map_err(db)?;
        if size == 0 || size > MAX_PCM {
            return Err("当前会议没有可用于说话人识别的录音".into());
        }
        let mut query = conn
            .prepare("SELECT pcm FROM meeting_audio_chunks WHERE meeting_id=?1 ORDER BY sequence")
            .map_err(db)?;
        let rows = query
            .query_map([&id], |row| row.get::<_, Vec<u8>>(0))
            .map_err(db)?;
        let mut pcm = Vec::with_capacity(size);
        for row in rows {
            pcm.extend(row.map_err(db)?);
        }
        (store.generation.clone(), meeting, pcm)
    };
    let updates = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let _model_guard = meeting_speech::model_read_lock(&model_path)?;
        let diarizer = meeting_speech::load_speaker_model(&model_path)?;
        if diarizer.sample_rate() != 16_000 || !pcm.len().is_multiple_of(2) {
            return Err("说话人识别模型与录音采样格式不匹配".to_string());
        }
        let duration = (pcm.len() / 32) as i64;
        let samples: Vec<f32> = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&sample| f32::from(i16::from_le_bytes(sample)) / 32768.0)
            .collect();
        drop(pcm);
        let result = diarizer.process(&samples).ok_or("本机说话人识别失败")?;
        let turns: Vec<_> = result
            .sort_by_start_time()
            .into_iter()
            .filter_map(|turn| {
                let start = (turn.start * 1000.0).round() as i64;
                let end = ((turn.end * 1000.0).round() as i64).min(duration);
                (turn.start.is_finite()
                    && turn.end.is_finite()
                    && start >= 0
                    && end > start
                    && turn.speaker >= 0)
                    .then_some(Turn {
                        start,
                        end,
                        speaker: turn.speaker,
                    })
            })
            .collect();
        Ok::<_, String>(speaker_updates(&meeting, &turns))
    })
    .await
    .map_err(|_| "本机说话人识别任务异常退出")??;

    let store = state.store.lock().map_err(|e| e.to_string())?;
    context(&store, &workspace_id, &generation)?;
    meeting_realtime::ensure_idle(&workspace_id, &id)?;
    let mut conn = store.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(db)?;
    let mut meeting = read(&tx, &id)?;
    if meeting["revision"].as_i64() != Some(revision) {
        return Err(CONFLICT.into());
    }
    if updates.is_empty() {
        return Ok(json!({ "meeting": meeting, "assigned": 0 }));
    }
    for segment in &updates {
        tx.execute(
            "UPDATE meeting_segments SET data=?1 WHERE meeting_id=?2 AND id=?3",
            params![segment.to_string(), id, text(segment, "id")],
        )
        .map_err(db)?;
    }
    stale_analysis(&mut meeting);
    write_metadata(&tx, &mut meeting, revision)?;
    let saved = read(&tx, &id)?;
    tx.commit().map_err(db)?;
    Ok(json!({ "meeting": saved, "assigned": updates.len() }))
}

// 用发言时间占比匹配文字。两人均占显著时长或发言太短时不猜测。
fn dominant_speaker(segment: &Value, turns: &[Turn]) -> Option<i32> {
    let start = segment["startMs"].as_i64()?;
    let end = segment["endMs"].as_i64()?;
    let mut overlap = HashMap::new();
    for turn in turns.iter().take_while(|turn| turn.start < end) {
        let shared = turn.end.min(end) - turn.start.max(start);
        if shared > 0 {
            *overlap.entry(turn.speaker).or_insert(0i64) += shared;
        }
    }
    let (speaker, time) = overlap.iter().max_by_key(|(_, time)| *time)?;
    if *time < 300
        || *time * 3 < end - start
        || overlap
            .iter()
            .any(|(other, other_time)| other != speaker && other_time * 3 >= *time)
    {
        return None;
    }
    Some(*speaker)
}

fn speaker_updates(meeting: &Value, turns: &[Turn]) -> Vec<Value> {
    let Some(segments) = meeting["segments"].as_array() else {
        return vec![];
    };
    let matches: Vec<_> = segments
        .iter()
        .map(|segment| dominant_speaker(segment, turns))
        .collect();
    let mut used: HashSet<String> = meeting["speakerNames"]
        .as_object()
        .into_iter()
        .flat_map(|names| names.keys().cloned())
        .collect();
    let mut anchors: HashMap<i32, HashSet<String>> = HashMap::new();
    let mut reverse: HashMap<String, HashSet<i32>> = HashMap::new();
    for (segment, speaker) in segments.iter().zip(&matches) {
        let id = text(segment, "speaker");
        if !id.is_empty() {
            used.insert(id.into());
        }
        if let Some(speaker) = speaker.filter(|_| id.starts_with(LOCAL_PREFIX)) {
            anchors.entry(speaker).or_default().insert(id.into());
            reverse.entry(id.into()).or_default().insert(speaker);
        }
    }
    // 继续录音时只复用单一且双向匹配的标签，保留用户填写的名称。
    let mut labels: HashMap<i32, String> = anchors
        .into_iter()
        .filter_map(|(speaker, ids)| {
            if ids.len() != 1 {
                return None;
            }
            let id = ids.into_iter().next()?;
            (reverse.get(&id)?.len() == 1).then_some((speaker, id))
        })
        .collect();
    let mut next = 1;
    segments
        .iter()
        .zip(matches)
        .filter_map(|(segment, speaker)| {
            if !text(segment, "speaker").is_empty() {
                return None;
            }
            let speaker = speaker?;
            let label = labels.entry(speaker).or_insert_with(|| loop {
                let id = format!("{LOCAL_PREFIX}{next}");
                next += 1;
                if used.insert(id.clone()) {
                    break id;
                }
            });
            let mut updated = segment.clone();
            updated["speaker"] = json!(label);
            Some(updated)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_distinct_speakers_preserves_names_and_skips_mixed_turns() {
        let meeting = json!({
            "speakerNames": {"local-speaker:1":"同事甲"},
            "segments":[
                {"id":"a","text":"已校对","speaker":"local-speaker:1","startMs":0,"endMs":1000},
                {"id":"b","text":"同一个人","startMs":1000,"endMs":2000},
                {"id":"c","text":"另一个人","startMs":2000,"endMs":3000},
                {"id":"d","text":"重叠说话","startMs":3000,"endMs":4000},
            ]
        });
        let turns = [
            Turn {
                start: 0,
                end: 2000,
                speaker: 3,
            },
            Turn {
                start: 2000,
                end: 4000,
                speaker: 7,
            },
            Turn {
                start: 3000,
                end: 4000,
                speaker: 3,
            },
        ];
        let updates = speaker_updates(&meeting, &turns);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0]["speaker"], "local-speaker:1");
        assert_eq!(updates[1]["speaker"], "local-speaker:2");
        assert_eq!(updates[1]["text"], "另一个人");
    }
}
