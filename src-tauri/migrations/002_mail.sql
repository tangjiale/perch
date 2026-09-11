CREATE TABLE mail_accounts(id TEXT PRIMARY KEY, revision INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
CREATE TABLE mail_folders(account_id TEXT NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE, path TEXT NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)), PRIMARY KEY(account_id,path));
CREATE TABLE mail_messages(id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL, uid INTEGER NOT NULL, date_ms INTEGER NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)), raw BLOB NOT NULL, UNIQUE(account_id,folder,uid_validity,uid));
CREATE INDEX mail_messages_folder ON mail_messages(account_id,folder,date_ms DESC);
CREATE TABLE mail_drafts(id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE, state TEXT NOT NULL DEFAULT 'draft', data TEXT NOT NULL CHECK(json_valid(data)));
