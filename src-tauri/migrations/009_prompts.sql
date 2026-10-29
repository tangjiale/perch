-- 个人提示词独立保存，内容历史存于 data，沿用统一乐观锁。
CREATE TABLE prompts(id TEXT PRIMARY KEY, title TEXT NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
