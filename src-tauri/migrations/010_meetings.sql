-- 会议原文、人工命名及 AI 产物；音频随工作空间数据库一起备份。
CREATE TABLE meetings (
 id TEXT PRIMARY KEY,
 revision INTEGER NOT NULL CHECK(revision > 0),
 data TEXT NOT NULL CHECK(json_valid(data))
);
-- 分片序号只在事务中递增，chunk_id 保证重复提交不会重复录音。
CREATE TABLE meeting_audio_chunks (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 chunk_id TEXT NOT NULL,
 sequence INTEGER NOT NULL,
 duration_ms INTEGER NOT NULL CHECK(duration_ms > 0),
 pcm BLOB NOT NULL,
 full_transcribed INTEGER NOT NULL DEFAULT 0 CHECK(full_transcribed IN (0,1)),
 PRIMARY KEY(meeting_id, chunk_id),
 UNIQUE(meeting_id, sequence)
);
-- source_chunk 记录转录来源，重转录单片只替换该片成功结果。
CREATE TABLE meeting_segments (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 id TEXT NOT NULL,
 source_chunk TEXT,
 start_ms INTEGER NOT NULL,
 end_ms INTEGER NOT NULL,
 data TEXT NOT NULL CHECK(json_valid(data)),
 PRIMARY KEY(meeting_id,id)
);
CREATE INDEX meeting_segments_timeline ON meeting_segments(meeting_id,start_ms,id);
