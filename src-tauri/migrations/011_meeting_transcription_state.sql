-- 补齐早期 schema 10 已建库缺少的整场转录覆盖标记，不重建或清空录音表。
ALTER TABLE meeting_audio_chunks ADD COLUMN full_transcribed INTEGER NOT NULL DEFAULT 0 CHECK(full_transcribed IN (0,1));

-- 旧版已有整场转录时，保守保护此前音频块，避免局部转录覆盖人工校对。
-- 后续追加的新块仍使用默认值 0，可继续边录边转录。
UPDATE meeting_audio_chunks
SET full_transcribed=1
WHERE EXISTS (
 SELECT 1 FROM meeting_segments
 WHERE meeting_segments.meeting_id=meeting_audio_chunks.meeting_id
   AND meeting_segments.source_chunk IS NULL
);
