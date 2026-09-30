use crate::data::{optional, text, validate};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

const DEFAULT_DATA_DIR: &str = ".perch";
const LEGACY_DATA_DIR: &str = ".self-workbanch";
const ENCRYPTION_CHECK_KEY: &str = "credential-encryption-check";
const ENCRYPTION_CHECK_PURPOSE: &str = "settings:credential-encryption-check";
const ENCRYPTION_CLEANUP_KEY: &str = "credential-encryption-cleanup-pending";

pub struct Store {
    pub root: PathBuf,
    pub generation: PathBuf,
    pub workspace_id: String,
    pub conn: Mutex<Connection>,
    _lock: File,
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
    let mut file = File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(
        serde_json::to_string_pretty(value)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}

fn verify_encryption_key(conn: &Connection, workspace: &str) -> Result<(), String> {
    let raw: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [ENCRYPTION_CHECK_KEY],
            |row| row.get(0),
        )
        .map_err(|_| "工作空间缺少加密校验信息，未修改数据")?;
    let check = crate::credential_vault::decrypt_json(workspace, ENCRYPTION_CHECK_PURPOSE, &raw)
        .map_err(|_| "本机加密密钥缺失、损坏或与工作空间不匹配，无法打开或恢复该工作空间")?;
    if check != json!({"version":1,"workspaceId":workspace}) {
        return Err("工作空间加密校验失败，未修改数据".into());
    }
    Ok(())
}

fn migrate_encrypted_credentials(conn: &mut Connection, workspace: &str) -> Result<(), String> {
    let sql = include_str!("../migrations/006_encrypted_credentials.sql");
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(sql).map_err(|e| e.to_string())?;
    let providers = {
        let mut statement = tx
            .prepare("SELECT provider_id,secret FROM provider_credentials")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for (id, secret) in providers {
        let encrypted =
            crate::credential_vault::encrypt(workspace, &format!("provider:{id}"), &secret)?;
        tx.execute(
            "UPDATE provider_credentials SET secret=?1 WHERE provider_id=?2",
            params![encrypted, id],
        )
        .map_err(|e| e.to_string())?;
    }
    for (table, id_column, purpose, filter) in [
        ("connections", "id", "connections", ""),
        ("mail_accounts", "id", "mail_accounts", ""),
        (
            "entity_history",
            "entity_id",
            "connections",
            " WHERE entity_type='connections'",
        ),
    ] {
        let rows = {
            let mut statement = tx
                .prepare(&format!(
                    "SELECT rowid,{id_column},data FROM {table}{filter}"
                ))
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (rowid, id, raw) in rows {
            let value =
                serde_json::from_str(&raw).map_err(|_| "旧版凭据配置格式无效，未执行迁移")?;
            let encrypted = crate::credential_vault::encrypt_json(
                workspace,
                &format!("{purpose}:{id}"),
                &value,
            )?;
            tx.execute(
                &format!("UPDATE {table} SET data=?1 WHERE rowid=?2"),
                params![encrypted, rowid],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let calendar: Option<String> = tx
        .query_row(
            "SELECT value FROM settings WHERE key='dingtalk-caldav'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(raw) = calendar {
        let value = serde_json::from_str(&raw).map_err(|_| "旧版日历配置格式无效，未执行迁移")?;
        let encrypted =
            crate::credential_vault::encrypt_json(workspace, "settings:dingtalk-caldav", &value)?;
        tx.execute(
            "UPDATE settings SET value=?1 WHERE key='dingtalk-caldav'",
            [encrypted],
        )
        .map_err(|e| e.to_string())?;
    }
    let check = crate::credential_vault::encrypt_json(
        workspace,
        ENCRYPTION_CHECK_PURPOSE,
        &json!({"version":1,"workspaceId":workspace}),
    )?;
    tx.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)",
        params![ENCRYPTION_CHECK_KEY, check],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO schema_migrations VALUES(6,?1,?2)",
        params![
            format!("{:x}", Sha256::digest(sql.as_bytes())),
            chrono::Utc::now().timestamp_millis()
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute_batch("PRAGMA user_version=6")
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn checkpoint(conn: &Connection) -> Result<(), String> {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|_| "无法清理旧版凭据日志，请关闭其他数据库连接后重试")?;
    if busy != 0 {
        return Err("旧版凭据日志仍被占用，请关闭其他数据库连接后重试".into());
    }
    Ok(())
}

fn finish_encryption_cleanup(conn: &Connection) -> Result<(), String> {
    let pending: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key=?1)",
            [ENCRYPTION_CLEANUP_KEY],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if pending {
        // 事务提交之后清理旧页及 WAL；保留标记，以便中断后继续完成物理清理。
        checkpoint(conn)?;
        conn.execute_batch("VACUUM")
            .map_err(|_| "无法清理旧版凭据页面，请重启应用后重试")?;
        checkpoint(conn)?;
        conn.execute(
            "DELETE FROM settings WHERE key=?1",
            [ENCRYPTION_CLEANUP_KEY],
        )
        .map_err(|e| e.to_string())?;
        checkpoint(conn)?;
    }
    Ok(())
}

fn decode_entity(workspace: &str, table: &str, id: &str, raw: &str) -> Result<Value, String> {
    if table == "connections" {
        crate::credential_vault::decrypt_json(workspace, &format!("connections:{id}"), raw)
    } else {
        serde_json::from_str(raw).map_err(|e| e.to_string())
    }
}

impl Store {
    pub fn restore_to(archive: &Path, target: &Path) -> Result<Self, String> {
        if target.exists()
            && fs::read_dir(target)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err("恢复目标必须是空目录".into());
        }
        let file = File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let manifest: Value = {
            let mut entry = zip.by_name("manifest.json").map_err(|e| e.to_string())?;
            serde_json::from_reader(&mut entry).map_err(|e| e.to_string())?
        };
        if manifest["formatVersion"] != json!(1)
            || !matches!(manifest["schemaVersion"].as_i64(), Some(1..=6))
        {
            return Err("不支持的备份版本".into());
        }
        let workspace = manifest["workspaceId"]
            .as_str()
            .ok_or("备份缺少工作空间 ID")?;
        uuid::Uuid::parse_str(workspace).map_err(|_| "工作空间 ID 无效")?;
        let generation_id = uuid::Uuid::now_v7().to_string();
        let generation = target
            .join("workspaces")
            .join(workspace)
            .join("generations")
            .join(&generation_id);
        let files = manifest["files"].as_array().ok_or("备份清单无效")?;
        let mut verified = Vec::new();
        let mut total = 0u64;
        let mut names = std::collections::HashSet::new();
        for item in files {
            let name = item["path"].as_str().ok_or("备份路径无效")?;
            let path = Path::new(name);
            if !names.insert(name.to_string())
                || path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                || (name != "workbench.sqlite3" && !name.starts_with("objects/"))
            {
                return Err("备份包含不允许的路径".into());
            }
            let mut entry = zip.by_name(name).map_err(|e| e.to_string())?;
            total = total.checked_add(entry.size()).ok_or("备份过大")?;
            if total > 2 * 1024 * 1024 * 1024 || entry.size() > 512 * 1024 * 1024 {
                return Err("备份超过恢复大小限制".into());
            }
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
            if item["size"].as_u64() != Some(bytes.len() as u64)
                || item["sha256"].as_str() != Some(&format!("{:x}", Sha256::digest(&bytes)))
            {
                return Err("备份文件校验失败".into());
            }
            verified.push((name.to_string(), bytes));
        }
        if !names.contains("workbench.sqlite3") {
            return Err("备份缺少数据库".into());
        }
        for (name, bytes) in verified {
            let output = generation.join(name);
            fs::create_dir_all(output.parent().ok_or("无效恢复路径")?)
                .map_err(|e| e.to_string())?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        let db =
            Connection::open(generation.join("workbench.sqlite3")).map_err(|e| e.to_string())?;
        let integrity: String = db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if integrity != "ok" {
            return Err("备份数据库完整性校验失败".into());
        }
        if db
            .prepare("PRAGMA foreign_key_check")
            .map_err(|e| e.to_string())?
            .query([])
            .map_err(|e| e.to_string())?
            .next()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("备份数据库关联校验失败".into());
        }
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if version > 6 {
            return Err("该数据库由更新版本创建，请升级应用".into());
        }
        if version >= 6 {
            // 先确认本机仍有匹配的密钥，失败时不发布新的工作空间定位。
            verify_encryption_key(&db, workspace)?;
        }
        drop(db);
        atomic_json(
            &target.join("current.json"),
            &json!({"workspaceId":workspace,"generationId":generation_id}),
        )?;
        Self::open_at(target.to_path_buf())
    }

    pub fn migrate_copy(&self, target: &Path) -> Result<Self, String> {
        fs::create_dir_all(target).map_err(|e| e.to_string())?;
        let target = target.canonicalize().map_err(|e| e.to_string())?;
        if target.starts_with(&self.root) || self.root.starts_with(&target) {
            return Err("目标与当前目录不能相同或互相包含".into());
        }
        if fs::read_dir(&target)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
        {
            return Err("迁移目标必须为空目录".into());
        }
        let archive = self
            .generation
            .join("staging")
            .join(format!("migration-{}.zip", uuid::Uuid::now_v7()));
        self.backup(&archive)?;
        let restored = Self::restore_to(&archive, &target)?;
        let mut backups = Vec::new();
        collect_files(&self.root.join("backups"), &self.root, &mut backups)?;
        for (source, relative) in backups {
            let destination = target.join(relative);
            fs::create_dir_all(destination.parent().ok_or("备份路径无效")?)
                .map_err(|e| e.to_string())?;
            fs::copy(&source, &destination).map_err(|e| e.to_string())?;
            if Sha256::digest(fs::read(source).map_err(|e| e.to_string())?)
                != Sha256::digest(fs::read(destination).map_err(|e| e.to_string())?)
            {
                return Err("备份目录复制校验失败".into());
            }
        }
        fs::remove_file(archive).map_err(|e| e.to_string())?;
        Ok(restored)
    }

    pub fn activate_default_bootstrap(&self) -> Result<(), String> {
        let default = dirs::home_dir()
            .ok_or("无法确定用户目录")?
            .join(DEFAULT_DATA_DIR);
        fs::create_dir_all(&default).map_err(|e| e.to_string())?;
        atomic_json(
            &default.join("bootstrap.json"),
            &json!({"configVersion":1,"dataRoot":self.root,"workspaceId":self.workspace_id}),
        )
    }
    pub fn info(&self) -> Value {
        json!({"dataRoot":self.root,"defaultRoot":dirs::home_dir().map(|p|p.join(DEFAULT_DATA_DIR)),"workspaceId":self.workspace_id,"version":env!("CARGO_PKG_VERSION")})
    }

    pub fn save(&self, kind: &str, value: Value) -> Result<Value, String> {
        self.save_table(entity_table(kind)?, value)
    }

    /// 模型供应商密钥经认证加密后保存，不混入前端快照。
    pub fn provider_secret(&self, provider_id: &str) -> Result<String, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let encrypted: Option<String> = conn
            .query_row(
                "SELECT secret FROM provider_credentials WHERE provider_id=?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        encrypted
            .map(|secret| {
                crate::credential_vault::decrypt(
                    &self.workspace_id,
                    &format!("provider:{provider_id}"),
                    &secret,
                )
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    /// 供应商配置与新密钥在同一 SQLite 事务内提交；None 表示保留已有密钥。
    pub fn save_provider_with_secret(
        &self,
        value: Value,
        secret: Option<&str>,
    ) -> Result<Value, String> {
        if secret.is_some_and(|value| value.len() > 64 * 1024) {
            return Err("API Key 过长".into());
        }
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let output = Self::save_in_transaction(&tx, &self.workspace_id, "providers", value, false)?;
        if let Some(secret) = secret {
            let provider_id = text(&output, "id");
            if secret.is_empty() {
                tx.execute(
                    "DELETE FROM provider_credentials WHERE provider_id=?1",
                    [provider_id],
                )
                .map_err(|e| e.to_string())?;
            } else {
                tx.execute(
                    "INSERT INTO provider_credentials(provider_id,secret,updated_at_ms) VALUES(?1,?2,?3) \
                     ON CONFLICT(provider_id) DO UPDATE SET secret=excluded.secret,updated_at_ms=excluded.updated_at_ms",
                    params![provider_id, crate::credential_vault::encrypt(
                        &self.workspace_id,
                        &format!("provider:{provider_id}"),
                        secret,
                    )?, chrono::Utc::now().timestamp_millis()],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }

    /// 远端写入前使用同一落库规则校验，事务回滚，不发布草稿。
    pub fn validate_save(&self, kind: &str, value: Value) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        Self::save_in_transaction(&tx, &self.workspace_id, entity_table(kind)?, value, false)?;
        tx.rollback().map_err(|e| e.to_string())
    }

    /// 在同一事务中校验输入快照并发布结果，避免同步或索引仅保存部分数据。
    pub fn save_batch(
        &self,
        changes: Vec<(&str, Value)>,
        expected: Vec<(&str, String, i64)>,
        trusted_remote: bool,
    ) -> Result<Vec<Value>, String> {
        self.save_batch_with_scope(changes, expected, trusted_remote, None)
    }

    pub fn save_zentao_snapshot(
        &self,
        changes: Vec<(&str, Value)>,
        expected: Vec<(&str, String, i64)>,
        connection: &str,
    ) -> Result<Vec<Value>, String> {
        self.save_batch_with_scope(changes, expected, true, Some(connection))
    }

    /// 完整读取后一次发布当前连接 BUG；同步失败不清除既有缓存。
    pub fn save_bug_snapshot(
        &self,
        changes: Vec<Value>,
        connection: &Value,
    ) -> Result<Vec<Value>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let id = text(connection, "id");
        let revision: i64 = tx
            .query_row("SELECT revision FROM connections WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(|_| "禅道连接已删除")?;
        if connection["revision"].as_i64() != Some(revision) {
            return Err("禅道连接已变化，请重新同步".into());
        }
        let keep: std::collections::HashSet<_> = changes.iter().map(|v| text(v, "id")).collect();
        let ids = {
            let mut stmt = tx
                .prepare("SELECT id FROM bugs WHERE connection_id=?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for removed in ids.iter().filter(|v| !keep.contains(v.as_str())) {
            tx.execute("DELETE FROM bugs WHERE id=?1", [removed])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM entity_history WHERE entity_type='bugs' AND entity_id=?1",
                [removed],
            )
            .map_err(|e| e.to_string())?;
        }
        let mut output = vec![];
        for value in changes {
            if text(&value, "connectionId") != id {
                return Err("BUG 连接范围不一致".into());
            }
            output.push(Self::save_in_transaction(
                &tx,
                &self.workspace_id,
                "bugs",
                value,
                true,
            )?);
        }
        let mut latest = connection.clone();
        latest["lastBugSync"] = json!(chrono::Utc::now().timestamp_millis());
        Self::save_in_transaction(&tx, &self.workspace_id, "connections", latest, true)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }

    fn save_batch_with_scope(
        &self,
        changes: Vec<(&str, Value)>,
        expected: Vec<(&str, String, i64)>,
        trusted_remote: bool,
        connection: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (kind, id, revision) in expected {
            let table = entity_table(kind)?;
            let actual: Option<i64> = tx
                .query_row(
                    &format!("SELECT revision FROM {table} WHERE id=?1"),
                    [&id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if actual != Some(revision) {
                return Err("输入数据已修改或删除，请重新执行操作".into());
            }
        }
        if let Some(connection) = connection {
            // 仅在整轮远端读取成功后，对当前连接发布的工作项快照做差集清理。
            let keep: std::collections::HashSet<&str> = changes
                .iter()
                .filter(|(kind, value)| {
                    *kind == "task"
                        && text(value, "source") == "zentao"
                        && text(value, "connectionId") == connection
                })
                .map(|(_, value)| text(value, "id"))
                .collect();
            let ids = {
                let mut statement = tx.prepare("SELECT id FROM tasks WHERE source='zentao' AND json_extract(data,'$.connectionId')=?1").map_err(|e| e.to_string())?;
                let rows = statement
                    .query_map([connection], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?
            };
            for id in ids.into_iter().filter(|id| !keep.contains(id.as_str())) {
                tx.execute("DELETE FROM tasks WHERE id=?1", [&id])
                    .map_err(|e| e.to_string())?;
                tx.execute(
                    "DELETE FROM entity_history WHERE entity_type='tasks' AND entity_id=?1",
                    [&id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        let mut output = Vec::with_capacity(changes.len());
        for (kind, value) in changes {
            output.push(Self::save_in_transaction(
                &tx,
                &self.workspace_id,
                entity_table(kind)?,
                value,
                trusted_remote,
            )?);
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }

    pub fn delete(&self, kind: &str, id: &str, revision: Option<i64>) -> Result<(), String> {
        let table = entity_table(kind)?;
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let revision = revision.ok_or("删除需要记录版本，请刷新重试")?;
        if table == "conversations" {
            let current: Option<i64> = tx
                .query_row(
                    "SELECT revision FROM conversations WHERE id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if current != Some(revision) {
                return Err("记录已修改或删除，请刷新重试".into());
            }
            let active: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id=?1 AND status='streaming'",
                    [id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if active > 0 {
                return Err("请先停止生成，再删除会话".into());
            }
            tx.execute("DELETE FROM messages WHERE conversation_id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        let changed = tx
            .execute(
                &format!("DELETE FROM {table} WHERE id=?1 AND revision=?2"),
                params![id, revision],
            )
            .map_err(|_| "该记录仍被引用，请先解除关联或停用".to_string())?;
        if changed == 0 {
            return Err("记录已修改或删除，请刷新重试".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn install_agent_presets_once(&self) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let installed: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE key='agent-presets-v1')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if installed {
            return Ok(());
        }
        let model: Option<String> = tx.query_row("SELECT m.id FROM models m JOIN providers p ON m.provider_id=p.id WHERE json_extract(m.data,'$.enabled')=1 AND json_extract(p.data,'$.enabled')=1 AND json_extract(m.data,'$.capability') IN ('chat','vision') ORDER BY CASE json_extract(m.data,'$.capability') WHEN 'chat' THEN 0 ELSE 1 END,m.id LIMIT 1", [], |row| row.get(0)).optional().map_err(|e|e.to_string())?;
        for agent in crate::agent_presets::agents(model.as_deref().unwrap_or("")) {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1)",
                    [text(&agent, "id")],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                Self::save_in_transaction(&tx, &self.workspace_id, "agents", agent, false)?;
            }
        }
        tx.execute(
            "INSERT INTO settings(key,value) VALUES('agent-presets-v1','true')",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn open_default() -> Result<Self, String> {
        let home = dirs::home_dir().ok_or("无法确定用户目录")?;
        let default = home.join(DEFAULT_DATA_DIR);
        let legacy = home.join(LEGACY_DATA_DIR);
        migrate_legacy_default(&legacy, &default)?;
        let bootstrap = default.join("bootstrap.json");
        if bootstrap.exists() {
            let config: Value =
                serde_json::from_slice(&fs::read(&bootstrap).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("启动配置损坏：{e}"))?;
            let root = PathBuf::from(config["dataRoot"].as_str().ok_or("启动配置缺少路径")?);
            if !root.join("current.json").exists() {
                return Err("配置的数据目录不可用，请重新选择原数据目录".into());
            }
            return Self::open_at(root);
        }
        if default.join("current.json").exists() {
            return Err("发现已有数据但启动配置缺失，请恢复启动配置".into());
        }
        let store = Self::open_at(default.clone())?;
        atomic_json(
            &bootstrap,
            &json!({"configVersion":1,"dataRoot":store.root,"workspaceId":store.workspace_id}),
        )?;
        Ok(store)
    }

    pub fn open_at(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("workspace.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "该工作空间已由另一个应用进程打开".to_string())?;
        let locator = root.join("current.json");
        let existing = locator.exists();
        let current: Value = if existing {
            serde_json::from_slice(&fs::read(&locator).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        } else {
            if root.join("workspaces").exists() {
                return Err("工作空间定位文件丢失，禁止自动创建空库".into());
            }
            json!({"workspaceId":uuid::Uuid::now_v7().to_string(),"generationId":uuid::Uuid::now_v7().to_string()})
        };
        let workspace_id = current["workspaceId"]
            .as_str()
            .ok_or("无效工作空间定位")?
            .to_owned();
        let generation_id = current["generationId"].as_str().ok_or("无效数据代定位")?;
        uuid::Uuid::parse_str(&workspace_id).map_err(|_| "无效工作空间 ID")?;
        uuid::Uuid::parse_str(generation_id).map_err(|_| "无效数据代 ID")?;
        let generation = root
            .join("workspaces")
            .join(&workspace_id)
            .join("generations")
            .join(generation_id);
        if existing && !generation.join("workbench.sqlite3").is_file() {
            return Err("数据库文件缺失，禁止创建空库".into());
        }
        for directory in ["objects/sha256", "derived", "staging"] {
            fs::create_dir_all(generation.join(directory)).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(root.join("backups")).map_err(|e| e.to_string())?;
        let mut conn =
            Connection::open(generation.join("workbench.sqlite3")).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=5000; PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY;").map_err(|e|e.to_string())?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 6 {
            return Err("该数据库由更新版本创建，请升级应用".into());
        }
        if version == 0 {
            let sql = include_str!("../migrations/001_initial.sql");
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO schema_migrations VALUES(1,?1,?2)",
                params![
                    format!("{:x}", Sha256::digest(sql.as_bytes())),
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute_batch("PRAGMA user_version=1")
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 2 {
            let sql = include_str!("../migrations/002_mail.sql");
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO schema_migrations VALUES(2,?1,?2)",
                params![
                    format!("{:x}", Sha256::digest(sql.as_bytes())),
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute_batch("PRAGMA user_version=2")
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 3 {
            let sql = include_str!("../migrations/003_task_end.sql");
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql).map_err(|e| e.to_string())?;
            let rows = {
                let mut stmt = tx
                    .prepare("SELECT data FROM tasks")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?
            };
            for row in rows {
                let mut value: Value = serde_json::from_str(&row).map_err(|e| e.to_string())?;
                if value.get("dueDate").is_some() {
                    crate::data::migrate_task_end(&mut value);
                    Self::save_in_transaction(&tx, &workspace_id, "tasks", value, true)?;
                }
            }
            tx.execute(
                "INSERT INTO schema_migrations VALUES(3,?1,?2)",
                params![
                    format!("{:x}", Sha256::digest(sql.as_bytes())),
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute_batch("PRAGMA user_version=3")
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 4 {
            let sql = include_str!("../migrations/004_bugs.sql");
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO schema_migrations VALUES(4,?1,?2)",
                params![
                    format!("{:x}", Sha256::digest(sql.as_bytes())),
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute_batch("PRAGMA user_version=4")
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 5 {
            let sql = include_str!("../migrations/005_provider_credentials.sql");
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO schema_migrations VALUES(5,?1,?2)",
                params![
                    format!("{:x}", Sha256::digest(sql.as_bytes())),
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.execute_batch("PRAGMA user_version=5")
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 6 {
            migrate_encrypted_credentials(&mut conn, &workspace_id)?;
        }
        verify_encryption_key(&conn, &workspace_id)?;
        finish_encryption_cleanup(&conn)?;
        let integrity: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if integrity != "ok" {
            return Err(format!("数据库完整性检查失败：{integrity}"));
        }
        if !existing {
            atomic_json(&locator, &current)?;
        }
        let store = Self {
            root,
            generation,
            workspace_id,
            conn: Mutex::new(conn),
            _lock: lock,
        };
        store.install_agent_presets_once()?;
        let snapshot = store.snapshot()?;
        let interrupted = snapshot["messages"]
            .as_array()
            .ok_or("消息数据无效")?
            .iter()
            .filter(|m| ["streaming", "pending"].contains(&text(m, "status")))
            .map(|m| {
                let mut value = m.clone();
                value["status"] = json!("interrupted");
                ("message", value)
            })
            .collect::<Vec<_>>();
        if !interrupted.is_empty() {
            store.save_batch(interrupted, vec![], false)?;
        }
        Ok(store)
    }

    pub fn snapshot(&self) -> Result<Value, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut output = json!({"storagePath":self.root,"workspaceId":self.workspace_id});
        for (table, key) in [
            ("tasks", "tasks"),
            ("bugs", "bugs"),
            ("projects", "projects"),
            ("apps", "apps"),
            ("categories", "categories"),
            ("providers", "providers"),
            ("models", "models"),
            ("agents", "agents"),
            ("skills", "skills"),
            ("knowledge_bases", "knowledge"),
            ("conversations", "conversations"),
            ("messages", "messages"),
            ("documents", "documents"),
            ("connections", "connections"),
        ] {
            let mut stmt = conn
                .prepare(&format!("SELECT id,data FROM {table} ORDER BY rowid"))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            let mut values = vec![];
            for row in rows {
                let (id, raw) = row.map_err(|e| e.to_string())?;
                values.push(decode_entity(&self.workspace_id, table, &id, &raw)?);
            }
            output[key] = json!(values);
        }
        Ok(output)
    }

    fn save_table(&self, table: &str, value: Value) -> Result<Value, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let output = Self::save_in_transaction(&tx, &self.workspace_id, table, value, false)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(output)
    }

    fn save_in_transaction(
        tx: &rusqlite::Transaction<'_>,
        workspace: &str,
        table: &str,
        mut value: Value,
        trusted_remote: bool,
    ) -> Result<Value, String> {
        if !value.is_object() {
            return Err("数据必须是对象".into());
        }
        if table == "tasks" {
            value.as_object_mut().unwrap().remove("dueDate");
            for key in ["startAt", "endAt", "scheduleDate"] {
                value[key] = Value::Null;
            }
            if let Some(schedule) = value.get("schedule").filter(|s| !s.is_null()).cloned() {
                value["timezone"] = schedule["timezone"].clone();
                if text(&schedule, "kind") == "timed" {
                    let start = chrono::DateTime::parse_from_rfc3339(text(&schedule, "start"))
                        .map_err(|_| "开始时间必须包含时区偏移")?;
                    value["startAt"] = json!(start.timestamp_millis());
                    if let Some(end) = optional(&schedule, "end") {
                        value["endAt"] = json!(chrono::DateTime::parse_from_rfc3339(&end)
                            .map_err(|_| "结束时间无效")?
                            .timestamp_millis());
                    }
                } else if text(&schedule, "kind") == "all_day" {
                    let start =
                        chrono::NaiveDate::parse_from_str(text(&schedule, "start"), "%Y-%m-%d")
                            .map_err(|_| "全天开始日期无效")?;
                    if let Some(end) = optional(&schedule, "end") {
                        let end = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d")
                            .map_err(|_| "全天结束日期无效")?;
                        if end <= start {
                            return Err("全天结束日期必须晚于开始日期".into());
                        }
                    }
                    value["scheduleDate"] = schedule["start"].clone();
                } else {
                    return Err("无效排期类型".into());
                }
            }
        }
        validate(&value, table)?;
        let id = optional(&value, "id").unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let now = chrono::Utc::now().timestamp_millis();
        let previous: Option<(i64, String)> = tx
            .query_row(
                &format!("SELECT revision,data FROM {table} WHERE id=?1"),
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let revision = if let Some((revision, old)) = &previous {
            if value.get("revision").and_then(Value::as_i64) != Some(*revision) {
                return Err("数据已被修改，请刷新后重试".into());
            }
            let old = decode_entity(workspace, table, &id, old)?;
            if !trusted_remote
                && text(&old, "source") == "zentao"
                && (text(&value, "source") != "zentao"
                    || ["projectId", "remoteId", "remoteType", "connectionId"]
                        .iter()
                        .any(|key| old.get(*key) != value.get(*key)))
            {
                return Err("禅道来源及项目关联只能通过同步更新".into());
            }
            value["createdAt"] = old["createdAt"].clone();
            revision + 1
        } else {
            if value
                .get("revision")
                .and_then(Value::as_i64)
                .is_some_and(|r| r > 0)
            {
                return Err("记录已被删除，请刷新后重试".into());
            }
            value["createdAt"] = json!(now);
            1
        };
        value["id"] = json!(id);
        value["revision"] = json!(revision);
        value["updatedAt"] = json!(now);
        if table == "messages" && previous.is_none() {
            let sequence: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(sequence),0)+1 FROM messages WHERE conversation_id=?1",
                    [text(&value, "conversationId")],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            value["sequence"] = json!(sequence);
        } else if table == "messages" {
            let old: Value =
                serde_json::from_str(&previous.as_ref().unwrap().1).map_err(|e| e.to_string())?;
            value["sequence"] = old["sequence"].clone();
        }
        if ["tasks", "projects"].contains(&table) {
            if optional(&value, "source").is_none() {
                value["source"] = json!("local");
            }
            if optional(&value, "status").is_none() {
                value["status"] = json!("todo");
            }
        }
        if table == "tasks" {
            if optional(&value, "priority").is_none() {
                value["priority"] = json!("normal");
            }
            if optional(&value, "timezone").is_none() {
                value["timezone"] = json!("Asia/Shanghai");
            }
            value["completedAt"] = if text(&value, "status") == "done" {
                value
                    .get("completedAt")
                    .filter(|v| v.is_i64())
                    .cloned()
                    .unwrap_or(json!(now))
            } else {
                Value::Null
            };
            value["closedAt"] = if text(&value, "status") == "closed" {
                value
                    .get("closedAt")
                    .filter(|v| v.is_i64())
                    .cloned()
                    .unwrap_or(json!(now))
            } else {
                Value::Null
            };
        }
        let stored_data = if table == "connections" {
            crate::credential_vault::encrypt_json(workspace, &format!("connections:{id}"), &value)?
        } else {
            value.to_string()
        };
        let mut columns = vec!["id".to_string(), "revision".to_string(), "data".to_string()];
        let mut vals = vec![
            rusqlite::types::Value::Text(id.clone()),
            rusqlite::types::Value::Integer(revision),
            rusqlite::types::Value::Text(stored_data.clone()),
        ];
        let fields: &[(&str, &str)] = match table {
            "bugs" => &[
                ("title", "title"),
                ("connection_id", "connectionId"),
                ("remote_id", "remoteId"),
                ("status", "status"),
            ],
            "tasks" => &[
                ("title", "title"),
                ("project_id", "projectId"),
                ("source", "source"),
                ("status", "status"),
                ("priority", "priority"),
                ("start_at_ms", "startAt"),
                ("end_at_ms", "endAt"),
                ("schedule_date", "scheduleDate"),
                ("timezone", "timezone"),
            ],
            "projects" => &[("name", "name"), ("source", "source"), ("status", "status")],
            "apps" => &[
                ("name", "name"),
                ("category_id", "categoryId"),
                ("url", "url"),
            ],
            "models" => &[("name", "name"), ("provider_id", "providerId")],
            "agents" => &[("name", "name"), ("model_id", "modelId")],
            "knowledge_bases" => &[("name", "name"), ("embedding_model_id", "modelId")],
            "conversations" => &[
                ("title", "title"),
                ("agent_id", "agentId"),
                ("knowledge_base_id", "knowledgeId"),
            ],
            "messages" => &[
                ("conversation_id", "conversationId"),
                ("role", "role"),
                ("content", "content"),
                ("status", "status"),
                ("sequence", "sequence"),
            ],
            "documents" => &[
                ("knowledge_base_id", "knowledgeId"),
                ("name", "name"),
                ("status", "status"),
            ],
            _ => &[("name", "name")],
        };
        for (column, key) in fields {
            columns.push((*column).into());
            vals.push(match value.get(*key) {
                Some(Value::String(s)) if !s.is_empty() || *key == "content" => {
                    rusqlite::types::Value::Text(s.clone())
                }
                Some(Value::Number(n)) => {
                    rusqlite::types::Value::Integer(n.as_i64().ok_or("无效数值")?)
                }
                _ => rusqlite::types::Value::Null,
            });
        }
        if table == "categories" {
            columns.push("normalized_name".into());
            vals.push(rusqlite::types::Value::Text(
                text(&value, "name").trim().to_lowercase(),
            ));
        }
        if table == "knowledge_bases" {
            let model: String = tx
                .query_row(
                    "SELECT data FROM models WHERE id=?1",
                    [text(&value, "modelId")],
                    |r| r.get(0),
                )
                .map_err(|_| "请选择有效的向量模型")?;
            let model: Value = serde_json::from_str(&model).map_err(|e| e.to_string())?;
            if text(&model, "capability") != "embedding" {
                return Err("知识库必须选择向量模型".into());
            }
        }
        let updates = columns
            .iter()
            .skip(1)
            .map(|c| format!("{c}=excluded.{c}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {updates}",
            columns.join(","),
            vec!["?"; vals.len()].join(",")
        );
        tx.execute(&sql, rusqlite::params_from_iter(vals))
            .map_err(|e| format!("保存失败，请检查关联与字段：{e}"))?;
        if table == "agents" {
            tx.execute("DELETE FROM agent_skills WHERE agent_id=?1", [&id])
                .map_err(|e| e.to_string())?;
            if let Some(skills) = value["skillIds"].as_array() {
                for skill in skills {
                    tx.execute(
                        "INSERT INTO agent_skills VALUES(?1,?2)",
                        params![id, skill.as_str().ok_or("技能 ID 无效")?],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        tx.execute(
            "INSERT INTO entity_history VALUES(?1,?2,?3,?4,?5)",
            params![table, id, revision, stored_data, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(value)
    }

    fn delete_table(&self, table: &str, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let affected = conn
            .execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])
            .map_err(|_| "该记录仍被其他数据引用，请先解除关联或停用".to_string())?;
        if affected == 0 {
            return Err("记录不存在".into());
        }
        Ok(())
    }

    pub fn backup(&self, destination: &Path) -> Result<(), String> {
        if destination.exists() {
            return Err("备份目标文件已存在，请选择其他文件名".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let staging = self
            .generation
            .join("staging")
            .join(format!("backup-{}.sqlite3", uuid::Uuid::now_v7()));
        let _staging_cleanup = TemporaryFile(staging.clone());
        let mut target = Connection::open(&staging).map_err(|e| e.to_string())?;
        let backup =
            rusqlite::backup::Backup::new(&conn, &mut target).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(100, std::time::Duration::from_millis(5), None)
            .map_err(|e| e.to_string())?;
        drop(backup);
        drop(target);
        let archive_staging = destination.with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
        let _archive_cleanup = TemporaryFile(archive_staging.clone());
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&archive_staging)
            .map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        let mut manifest = vec![];
        let mut files = vec![(staging.clone(), "workbench.sqlite3".to_string())];
        collect_files(
            &self.generation.join("objects"),
            &self.generation,
            &mut files,
        )?;
        for (path, name) in files {
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            zip.start_file(&name, options).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
            manifest.push(json!({"path":name,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        zip.start_file("manifest.json", options)
            .map_err(|e| e.to_string())?;
        zip.write_all(json!({"formatVersion":1,"schemaVersion":6,"workspaceId":self.workspace_id,"files":manifest}).to_string().as_bytes()).map_err(|e|e.to_string())?;
        zip.finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        // 同目录硬链接只发布完整备份，且不会覆盖并发创建的目标。
        fs::hard_link(&archive_staging, destination).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 将旧版本默认工作空间一次性迁移到 `.perch`。使用同卷原子改名，避免
/// 两个目录同时被应用写入；旧目录仅在改名成功后消失。
fn migrate_legacy_default(legacy: &Path, default: &Path) -> Result<(), String> {
    if default.exists() || !legacy.exists() {
        return Ok(());
    }
    if !legacy.join("current.json").exists() && !legacy.join("bootstrap.json").exists() {
        return Ok(());
    }
    fs::rename(legacy, default).map_err(|error| format!("旧工作空间迁移失败：{error}"))?;
    let bootstrap = default.join("bootstrap.json");
    if bootstrap.exists() {
        let mut config: Value = serde_json::from_slice(
            &fs::read(&bootstrap).map_err(|error| format!("读取迁移配置失败：{error}"))?,
        )
        .map_err(|error| format!("迁移配置损坏：{error}"))?;
        if config["dataRoot"].as_str() == Some(legacy.to_string_lossy().as_ref()) {
            config["dataRoot"] = json!(default);
            atomic_json(&bootstrap, &config)?;
        }
    } else if default.join("current.json").exists() {
        let current: Value = serde_json::from_slice(
            &fs::read(default.join("current.json")).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("工作空间定位损坏：{error}"))?;
        atomic_json(
            &bootstrap,
            &json!({"configVersion":1,"dataRoot":default,"workspaceId":current["workspaceId"]}),
        )?;
    }
    Ok(())
}

fn collect_files(
    directory: &Path,
    root: &Path,
    output: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("管理目录不允许符号链接".into());
        }
        if kind.is_dir() {
            collect_files(&entry.path(), root, output)?;
        } else {
            output.push((
                entry.path(),
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            ));
        }
    }
    Ok(())
}

macro_rules! entity_api {
    ($save:ident,$delete:ident,$table:literal) => {
        impl Store {
            pub fn $save(&self, value: Value) -> Result<Value, String> {
                self.save_table($table, value)
            }
            pub fn $delete(&self, id: &str) -> Result<(), String> {
                self.delete_table($table, id)
            }
        }
    };
}
entity_api!(save_task, delete_task, "tasks");
entity_api!(save_project, delete_project, "projects");
entity_api!(save_app, delete_app, "apps");
entity_api!(save_category, delete_category, "categories");
entity_api!(save_provider, delete_provider, "providers");
entity_api!(save_model, delete_model, "models");
entity_api!(save_agent, delete_agent, "agents");
entity_api!(save_skill, delete_skill, "skills");
entity_api!(
    save_knowledge_base,
    delete_knowledge_base,
    "knowledge_bases"
);
entity_api!(save_conversation, delete_conversation, "conversations");
entity_api!(save_message, delete_message, "messages");
entity_api!(save_document, delete_document, "documents");

fn entity_table(kind: &str) -> Result<&'static str, String> {
    match kind {
        "task" => Ok("tasks"),
        "bug" => Ok("bugs"),
        "project" => Ok("projects"),
        "app" => Ok("apps"),
        "category" => Ok("categories"),
        "provider" => Ok("providers"),
        "model" => Ok("models"),
        "agent" => Ok("agents"),
        "skill" => Ok("skills"),
        "knowledge" => Ok("knowledge_bases"),
        "conversation" => Ok("conversations"),
        "message" => Ok("messages"),
        "document" => Ok("documents"),
        "connection" => Ok("connections"),
        _ => Err("未知实体类型".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_credentials_workspace(root: &Path) -> (String, PathBuf) {
        let workspace = uuid::Uuid::now_v7().to_string();
        let generation_id = uuid::Uuid::now_v7().to_string();
        let generation = root
            .join("workspaces")
            .join(&workspace)
            .join("generations")
            .join(&generation_id);
        fs::create_dir_all(&generation).unwrap();
        atomic_json(
            &root.join("current.json"),
            &json!({"workspaceId":workspace,"generationId":generation_id}),
        )
        .unwrap();
        let database = generation.join("workbench.sqlite3");
        let conn = Connection::open(&database).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_mail.sql"),
            include_str!("../migrations/003_task_end.sql"),
            include_str!("../migrations/004_bugs.sql"),
            include_str!("../migrations/005_provider_credentials.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("PRAGMA user_version=5").unwrap();
        conn.execute(
            "INSERT INTO providers VALUES('model','模型',1,?1)",
            [json!({"id":"model","name":"模型","revision":1}).to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO provider_credentials VALUES('model','fixture-model-api-key',1)",
            [],
        )
        .unwrap();
        let connection = json!({"id":"zentao","name":"禅道","revision":2,"loginAccount":"fixture-zentao-account","authMode":"account","rememberCredentials":true,"enabled":true,"hasCredential":true});
        conn.execute(
            "INSERT INTO connections VALUES('zentao','禅道',2,?1)",
            [connection.to_string()],
        )
        .unwrap();
        let mut history = connection;
        history["loginAccount"] = json!("fixture-historical-account");
        history["revision"] = json!(1);
        conn.execute(
            "INSERT INTO entity_history VALUES('connections','zentao',1,?1,1)",
            [history.to_string()],
        )
        .unwrap();
        conn.execute("INSERT INTO mail_accounts VALUES('mail',1,?1)", [json!({"id":"mail","revision":1,"username":"fixture-imap-account","smtpUsername":"fixture-smtp-account","hasImapCredential":true,"hasSmtpCredential":true}).to_string()]).unwrap();
        conn.execute("INSERT INTO settings VALUES('dingtalk-caldav',?1)", [json!({"config":{"username":"fixture-caldav-account","enabled":true,"hasCredential":true},"events":[]}).to_string()]).unwrap();
        conn.execute(
            "INSERT INTO settings VALUES('mcp.connection.test',?1)",
            [json!({"id":"test","hasSecrets":true,"enabled":true}).to_string()],
        )
        .unwrap();
        drop(conn);
        (workspace, database)
    }

    fn assert_fixture_credentials_absent(bytes: &[u8]) {
        for marker in [
            "fixture-model-api-key",
            "fixture-zentao-account",
            "fixture-historical-account",
            "fixture-imap-account",
            "fixture-smtp-account",
            "fixture-caldav-account",
        ] {
            assert!(
                !bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "未清理凭据明文：{marker}"
            );
        }
    }

    #[test]
    fn schema_six_encrypts_existing_credentials_and_cleans_database_and_wal() {
        let root = tempfile::tempdir().unwrap();
        let (workspace, database) = legacy_credentials_workspace(root.path());
        assert!(fs::read(&database)
            .unwrap()
            .windows(21)
            .any(|window| window == b"fixture-model-api-key"));
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let connection = store.snapshot().unwrap()["connections"][0].clone();
        assert_eq!(connection["loginAccount"], "fixture-zentao-account");
        assert_eq!(connection["revision"], 2);
        assert_eq!(connection["authMode"], "account");
        assert_eq!(connection["rememberCredentials"], true);
        assert_eq!(connection["enabled"], true);
        assert_eq!(connection["hasCredential"], false);
        assert_eq!(
            store.provider_secret("model").unwrap(),
            "fixture-model-api-key"
        );
        {
            let conn = store.conn.lock().unwrap();
            let history: String = conn.query_row("SELECT data FROM entity_history WHERE entity_type='connections' AND entity_id='zentao'", [], |row| row.get(0)).unwrap();
            let history =
                crate::credential_vault::decrypt_json(&workspace, "connections:zentao", &history)
                    .unwrap();
            assert_eq!(history["loginAccount"], "fixture-historical-account");
            assert_eq!(history["revision"], 1);
            let mail: String = conn
                .query_row(
                    "SELECT data FROM mail_accounts WHERE id='mail'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let mail =
                crate::credential_vault::decrypt_json(&workspace, "mail_accounts:mail", &mail)
                    .unwrap();
            assert_eq!(mail["username"], "fixture-imap-account");
            assert_eq!(mail["smtpUsername"], "fixture-smtp-account");
            assert_eq!(mail["hasImapCredential"], false);
            assert_eq!(mail["hasSmtpCredential"], false);
            let calendar: String = conn
                .query_row(
                    "SELECT value FROM settings WHERE key='dingtalk-caldav'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let calendar = crate::credential_vault::decrypt_json(
                &workspace,
                "settings:dingtalk-caldav",
                &calendar,
            )
            .unwrap();
            assert_eq!(calendar["config"]["username"], "fixture-caldav-account");
            assert_eq!(calendar["config"]["enabled"], true);
            assert_eq!(calendar["config"]["hasCredential"], false);
            let mcp: String = conn
                .query_row(
                    "SELECT value FROM settings WHERE key='mcp.connection.test'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&mcp).unwrap()["hasSecrets"],
                false
            );
            assert_eq!(
                conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                6
            );
        }
        assert_fixture_credentials_absent(&fs::read(&database).unwrap());
        let wal = database.with_file_name("workbench.sqlite3-wal");
        if wal.exists() {
            assert_fixture_credentials_absent(&fs::read(&wal).unwrap());
        }
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(
            store.provider_secret("model").unwrap(),
            "fixture-model-api-key"
        );
        let mut updated = connection;
        updated["loginAccount"] = json!("fixture-new-account");
        let updated = store.save("connection", updated).unwrap();
        assert_eq!(updated["revision"], 3);
        assert_eq!(store.snapshot().unwrap()["connections"][0], updated);
        let history: String = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT data FROM entity_history WHERE entity_type='connections' AND revision=3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!history.contains("fixture-new-account"));
        assert_eq!(
            crate::credential_vault::decrypt_json(&workspace, "connections:zentao", &history)
                .unwrap(),
            updated
        );
    }

    #[test]
    fn encrypted_records_reject_ciphertext_replacement_and_plaintext_fallback() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        for id in ["one", "two"] {
            store
                .save(
                    "connection",
                    json!({"id":id,"name":id,"loginAccount":format!("account-{id}")}),
                )
                .unwrap();
            store
                .save_provider_with_secret(
                    json!({"id":id,"name":id}),
                    Some(&format!("api-key-{id}")),
                )
                .unwrap();
        }
        {
            let conn = store.conn.lock().unwrap();
            conn.execute("UPDATE connections SET data=(SELECT data FROM connections WHERE id='one') WHERE id='two'", []).unwrap();
            conn.execute("UPDATE provider_credentials SET secret=(SELECT secret FROM provider_credentials WHERE provider_id='one') WHERE provider_id='two'", []).unwrap();
        }
        assert!(store.snapshot().is_err());
        assert!(store.provider_secret("two").is_err());
        assert_eq!(store.provider_secret("one").unwrap(), "api-key-one");
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE connections SET data=?1 WHERE id='two'",
                [json!({"id":"two","name":"two","revision":1}).to_string()],
            )
            .unwrap();
        assert!(store.snapshot().is_err());
    }

    #[test]
    fn encrypted_backup_contains_no_secrets_or_key_and_requires_matching_local_key() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let (workspace, _) = legacy_credentials_workspace(&source);
        let store = Store::open_at(source.clone()).unwrap();
        let archive = root.path().join("backup.zip");
        store.backup(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).unwrap();
            assert!(["workbench.sqlite3", "manifest.json"].contains(&entry.name()));
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            assert_fixture_credentials_absent(&bytes);
        }
        drop(zip);
        let restored = Store::restore_to(&archive, &root.path().join("restored")).unwrap();
        assert_eq!(
            restored.provider_secret("model").unwrap(),
            "fixture-model-api-key"
        );
        drop(restored);
        drop(store);
        let key = crate::credential_vault::test_key_path(&workspace);
        let held = key.with_extension("held");
        fs::rename(&key, &held).unwrap();
        assert!(Store::open_at(source.clone()).is_err());
        assert!(!key.exists(), "缺失密钥时禁止创建替代密钥");
        let target = root.path().join("missing-key-restore");
        assert!(Store::restore_to(&archive, &target).is_err());
        assert!(!target.join("current.json").exists());
        assert!(!key.exists());
        fs::rename(held, key).unwrap();
        assert_eq!(
            Store::open_at(source)
                .unwrap()
                .provider_secret("model")
                .unwrap(),
            "fixture-model-api-key"
        );
    }

    #[test]
    fn provider_metadata_and_encrypted_secret_rollback_together() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let provider = store
            .save_provider_with_secret(json!({"name":"原配置"}), Some("original-key"))
            .unwrap();
        store.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_credential_change BEFORE UPDATE ON provider_credentials BEGIN SELECT RAISE(ABORT, 'fixture refusal'); END;").unwrap();
        let mut changed = provider.clone();
        changed["name"] = json!("未提交配置");
        assert!(store
            .save_provider_with_secret(changed, Some("replacement-key"))
            .is_err());
        assert_eq!(
            store.provider_secret(text(&provider, "id")).unwrap(),
            "original-key"
        );
        assert_eq!(store.snapshot().unwrap()["providers"][0], provider);
        assert_eq!(
            store
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM entity_history WHERE entity_type='providers'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert!(!store
            .snapshot()
            .unwrap()
            .to_string()
            .contains("original-key"));
    }

    #[test]
    fn remote_preflight_validates_revision_without_publishing_changes() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let original = store
            .save(
                "task",
                json!({"title":"原任务","status":"todo","source":"local"}),
            )
            .unwrap();
        let mut draft = original.clone();
        draft["title"] = json!("待回写标题");
        store.validate_save("task", draft.clone()).unwrap();
        assert_eq!(store.snapshot().unwrap()["tasks"][0], original);
        draft["revision"] = json!(0);
        assert!(store.validate_save("task", draft).is_err());
        assert_eq!(store.snapshot().unwrap()["tasks"][0], original);
    }
    #[test]
    fn agent_presets_are_once_only_and_preserve_edits_and_deletions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let snapshot = store.snapshot().unwrap();
        let agents = snapshot["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 5);
        assert!(agents
            .iter()
            .all(|a| a["enabled"] == false && a["modelId"] == ""));
        let mut edited = agents[0].clone();
        edited["systemPrompt"] = json!("用户自己的提示词");
        store.save("agent", edited.clone()).unwrap();
        store
            .delete("agent", text(&agents[1], "id"), Some(1))
            .unwrap();
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["agents"].as_array().unwrap().len(), 4);
        assert!(snapshot["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["systemPrompt"] == "用户自己的提示词"));
    }
    #[test]
    fn agent_presets_select_an_enabled_chat_model_on_upgrade() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let provider = store
            .save("provider", json!({"name":"Provider","enabled":true}))
            .unwrap();
        let model = store.save("model",json!({"name":"Chat","providerId":provider["id"],"capability":"chat","enabled":true})).unwrap();
        // 模拟尚未安装预置的旧工作空间。
        store.conn.lock().unwrap().execute_batch("DELETE FROM agents; DELETE FROM entity_history WHERE entity_type='agents'; DELETE FROM settings WHERE key='agent-presets-v1';").unwrap();
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let snapshot = store.snapshot().unwrap();
        let agents = snapshot["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 5);
        assert!(agents
            .iter()
            .all(|a| a["modelId"] == model["id"] && a["enabled"] == true));
    }
    #[test]
    fn zentao_scope_pruning_is_atomic_and_connection_isolated() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let connection = store.save("connection", json!({"name":"Zentao"})).unwrap();
        let connection_id = text(&connection, "id");
        let values = store.save_batch(vec![
            ("task",json!({"title":"保留","source":"zentao","connectionId":connection_id})),
            ("task",json!({"title":"关闭执行","source":"zentao","connectionId":connection_id})),
            ("task",json!({"title":"关闭执行的任务","source":"zentao","connectionId":connection_id})),
            ("task",json!({"title":"其他连接","source":"zentao","connectionId":"other"})),
            ("task",json!({"title":"本地任务","source":"local"})),
        ],vec![],true).unwrap();
        let expected = vec![("connection", connection_id.to_owned(), 1)];
        assert!(store
            .save_zentao_snapshot(
                vec![(
                    "task",
                    json!({"id":values[0]["id"],"revision":999,"title":"错误"})
                )],
                expected.clone(),
                connection_id
            )
            .is_err());
        assert_eq!(
            store.snapshot().unwrap()["tasks"].as_array().unwrap().len(),
            5
        );
        store
            .save_zentao_snapshot(vec![("task", values[0].clone())], expected, connection_id)
            .unwrap();
        let remaining = store.snapshot().unwrap();
        assert_eq!(remaining["tasks"].as_array().unwrap().len(), 3);
        for title in ["保留", "其他连接", "本地任务"] {
            assert!(remaining["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| text(v, "title") == title));
        }
    }
    #[test]
    fn task_end_migration_preserves_schedule_and_revisions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let values = [
            json!({"title":"仅截止", "dueDate":"2026-09-08"}),
            json!({"title":"已有结束", "dueDate":"2026-09-01", "schedule":{"kind":"all_day","start":"2026-09-15","end":"2026-09-18","timezone":"Asia/Shanghai"}}),
            json!({"title":"无结束", "schedule":{"kind":"all_day","start":"2026-09-15","timezone":"Asia/Shanghai"}}),
        ];
        let mut saved = Vec::new();
        for value in &values {
            saved.push(store.save("task", value.clone()).unwrap());
        }
        {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN due_date TEXT; DROP TABLE bugs; DROP TABLE provider_credentials; DELETE FROM schema_migrations WHERE version>=3; DELETE FROM settings WHERE key IN ('credential-encryption-check','credential-encryption-cleanup-pending'); PRAGMA user_version=2;").unwrap();
            for (mut value, old) in saved.into_iter().zip(values) {
                if let Some(due) = old.get("dueDate") {
                    value["dueDate"] = due.clone();
                }
                conn.execute(
                    "UPDATE tasks SET data=?1,due_date=?2 WHERE id=?3",
                    params![
                        value.to_string(),
                        optional(&value, "dueDate"),
                        text(&value, "id")
                    ],
                )
                .unwrap();
            }
        }
        drop(store);
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let snapshot = store.snapshot().unwrap();
        let tasks = snapshot["tasks"].as_array().unwrap();
        let task = tasks.iter().find(|v| v["title"] == "仅截止").unwrap();
        assert_eq!(task["schedule"]["end"], "2026-09-09");
        assert_eq!(task["revision"], 2);
        assert!(tasks.iter().all(|v| v.get("dueDate").is_none()));
        assert_eq!(
            tasks.iter().find(|v| v["title"] == "已有结束").unwrap()["schedule"]["end"],
            "2026-09-18"
        );
        assert!(
            tasks.iter().find(|v| v["title"] == "无结束").unwrap()["schedule"]
                .get("end")
                .is_none()
        );
        let conn = store.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT MAX(revision) FROM entity_history WHERE entity_id=?1",
                [text(task, "id")],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert!(conn.prepare("SELECT due_date FROM tasks").is_err());
    }

    #[test]
    fn persistence_revisions_and_relations() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open_at(root.path().to_path_buf()).unwrap();
        let project = store.save("project", json!({"name":"Project"})).unwrap();
        let task=store.save("task",json!({"title":"Task","projectId":project["id"],"schedule":{"kind":"timed","timezone":"Asia/Shanghai","start":"2026-09-07T23:00:00+08:00","end":"2026-09-08T01:00:00+08:00"}})).unwrap();
        assert!(store
            .delete("project", project["id"].as_str().unwrap(), Some(1))
            .is_err());
        let mut changed = task.clone();
        changed["status"] = json!("done");
        let saved = store.save("task", changed).unwrap();
        assert_eq!(saved["revision"], 2);
        assert!(store.save("task", task).is_err());
        assert!(saved["completedAt"].is_i64());
        drop(store);
        let reopened = Store::open_at(root.path().to_path_buf()).unwrap();
        assert_eq!(reopened.snapshot().unwrap()["tasks"][0]["status"], "done");
    }

    #[test]
    fn provider_secret_persists_without_snapshot_leak_and_follows_backup() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("data");
        let archive = sandbox.path().join("provider-backup.zip");
        let store = Store::open_at(root.clone()).unwrap();
        let provider = store
            .save_provider_with_secret(
                json!({
                    "id":"provider-local-secret",
                    "name":"测试供应商",
                    "baseUrl":"https://example.com/v1",
                    "protocol":"openai-responses",
                    "enabled":true
                }),
                Some("fixture-api-key"),
            )
            .unwrap();
        assert_eq!(
            store.provider_secret(text(&provider, "id")).unwrap(),
            "fixture-api-key"
        );
        assert!(!store
            .snapshot()
            .unwrap()
            .to_string()
            .contains("fixture-api-key"));
        let history: String = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT data FROM entity_history WHERE entity_type='providers' AND entity_id=?1",
                [text(&provider, "id")],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!history.contains("fixture-api-key"));
        let mut changed = provider.clone();
        changed["name"] = json!("修改后仍保留密钥");
        let changed = store.save_provider_with_secret(changed, None).unwrap();
        assert_eq!(changed["revision"], 2);
        assert_eq!(
            store.provider_secret(text(&provider, "id")).unwrap(),
            "fixture-api-key"
        );
        store.backup(&archive).unwrap();
        drop(store);

        let reopened = Store::open_at(root).unwrap();
        assert_eq!(
            reopened.provider_secret(text(&provider, "id")).unwrap(),
            "fixture-api-key"
        );
        let restored = Store::restore_to(&archive, &sandbox.path().join("restored")).unwrap();
        assert_eq!(
            restored.provider_secret(text(&provider, "id")).unwrap(),
            "fixture-api-key"
        );
    }

    #[test]
    fn restores_legacy_backup_versions_and_migrates_deadline() {
        for version in [1, 2] {
            let sandbox = tempfile::tempdir().unwrap();
            let db = sandbox.path().join("old.sqlite3");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(include_str!("../migrations/001_initial.sql"))
                .unwrap();
            if version == 2 {
                conn.execute_batch(include_str!("../migrations/002_mail.sql"))
                    .unwrap();
            }
            conn.execute_batch(&format!("PRAGMA user_version={version}"))
                .unwrap();
            let task = json!({"id":"old","title":"旧任务","source":"local","status":"todo","priority":"normal","timezone":"Asia/Shanghai","revision":1,"dueDate":"2026-09-08"});
            conn.execute("INSERT INTO tasks(id,title,source,status,priority,timezone,revision,due_date,data) VALUES('old','旧任务','local','todo','normal','Asia/Shanghai',1,'2026-09-08',?1)",[task.to_string()]).unwrap();
            drop(conn);
            let bytes = fs::read(db).unwrap();
            let archive = sandbox.path().join("old.zip");
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("workbench.sqlite3", options).unwrap();
            zip.write_all(&bytes).unwrap();
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(json!({"formatVersion":1,"schemaVersion":version,"workspaceId":uuid::Uuid::now_v7().to_string(),"files":[{"path":"workbench.sqlite3","size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}]}).to_string().as_bytes()).unwrap();
            zip.finish().unwrap();
            let restored = Store::restore_to(&archive, &sandbox.path().join("restored")).unwrap();
            let snapshot = restored.snapshot().unwrap();
            assert_eq!(snapshot["tasks"][0]["schedule"]["end"], "2026-09-09");
            assert!(snapshot["tasks"][0].get("dueDate").is_none());
            assert_eq!(
                restored
                    .conn
                    .lock()
                    .unwrap()
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                6
            );
        }
    }

    #[test]
    fn backup_restore_and_copy_preserve_data() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().join("source")).unwrap();
        store
            .save("skill", json!({"name":"Skill","content":"hello"}))
            .unwrap();
        let object = store.generation.join("objects/sha256/ab/test");
        fs::create_dir_all(object.parent().unwrap()).unwrap();
        fs::write(&object, b"sample").unwrap();
        let archive = sandbox.path().join("backup.zip");
        store.backup(&archive).unwrap();
        let restored = Store::restore_to(&archive, &sandbox.path().join("restore")).unwrap();
        assert_eq!(
            restored.snapshot().unwrap()["skills"][0]["content"],
            "hello"
        );
        assert_eq!(
            fs::read(restored.generation.join("objects/sha256/ab/test")).unwrap(),
            b"sample"
        );
        let copied = store
            .migrate_copy(&sandbox.path().join("migrated"))
            .unwrap();
        assert_eq!(copied.workspace_id, store.workspace_id);
        assert!(store.migrate_copy(&store.root.join("nested")).is_err());
    }
    #[test]
    fn chat_empty_stream_and_knowledge_references() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        let provider = store.save("provider", json!({"name":"Provider"})).unwrap();
        let model = store
            .save(
                "model",
                json!({"name":"Embedding","providerId":provider["id"],"capability":"embedding"}),
            )
            .unwrap();
        let knowledge = store
            .save(
                "knowledge",
                json!({"name":"Knowledge","modelId":model["id"]}),
            )
            .unwrap();
        let conversation = store
            .save(
                "conversation",
                json!({"title":"Chat","knowledgeId":knowledge["id"]}),
            )
            .unwrap();
        let message=store.save("message",json!({"conversationId":conversation["id"],"role":"assistant","content":"","status":"streaming"})).unwrap();
        assert_eq!(message["sequence"], 1);
        assert!(store
            .delete("model", model["id"].as_str().unwrap(), Some(1))
            .is_err());
        assert!(store
            .delete("knowledge", knowledge["id"].as_str().unwrap(), Some(1))
            .is_err());
    }
    #[test]
    fn interrupted_generation_and_conversation_delete() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        let conversation = store
            .save("conversation", json!({"title":"Restart"}))
            .unwrap();
        store.save("message",json!({"conversationId":conversation["id"],"role":"assistant","content":"partial","status":"streaming"})).unwrap();
        assert!(store
            .delete("conversation", text(&conversation, "id"), Some(1))
            .is_err());
        drop(store);
        let reopened = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened.snapshot().unwrap()["messages"][0]["status"],
            "interrupted"
        );
        assert!(reopened
            .delete("conversation", text(&conversation, "id"), Some(2))
            .is_err());
        assert_eq!(
            reopened.snapshot().unwrap()["messages"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        reopened
            .delete("conversation", text(&conversation, "id"), Some(1))
            .unwrap();
        assert!(reopened.snapshot().unwrap()["messages"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn validation_and_workspace_lock() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        assert!(Store::open_at(sandbox.path().to_path_buf()).is_err());
        assert!(store.save("task",json!({"title":"bad","schedule":{"kind":"timed","start":"2026-09-07T12:00:00+08:00","end":"2026-09-07T11:00:00+08:00"}})).is_err());
        assert!(store
            .save("provider", json!({"name":"bad","apiKey":"secret"}))
            .is_err());
        assert!(store
            .save("app", json!({"name":"bad","url":"javascript:alert(1)"}))
            .is_err());
    }

    #[test]
    fn batch_conflict_and_failure_roll_back_all_rows_and_history() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        let project = store
            .save("project", json!({"id":"p","name":"Before"}))
            .unwrap();
        let mut changed = project.clone();
        changed["name"] = json!("After");
        assert!(store
            .save_batch(
                vec![
                    ("project", changed.clone()),
                    ("task", json!({"title":"Invalid","projectId":"missing"})),
                ],
                vec![],
                false
            )
            .is_err());
        assert_eq!(store.snapshot().unwrap()["projects"][0], project);
        assert!(store
            .save_batch(
                vec![("project", changed.clone())],
                vec![("project", "p".into(), 2)],
                false
            )
            .is_err());
        assert_eq!(store.snapshot().unwrap()["projects"][0], project);
        let saved = store
            .save_batch(
                vec![
                    ("project", changed),
                    ("task", json!({"title":"Valid","projectId":"p"})),
                ],
                vec![("project", "p".into(), 1)],
                false,
            )
            .unwrap();
        assert_eq!(saved[0]["revision"], 2);
        assert_eq!(saved[1]["projectId"], "p");
        let count: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM entity_history WHERE entity_id='p'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn only_trusted_sync_can_change_remote_association() {
        let sandbox = tempfile::tempdir().unwrap();
        let store = Store::open_at(sandbox.path().to_path_buf()).unwrap();
        let task = store
            .save(
                "task",
                json!({"title":"Remote","source":"zentao","remoteId":"1"}),
            )
            .unwrap();
        let mut changed = task.clone();
        changed["remoteId"] = json!("2");
        assert!(store.save("task", changed).is_err());
        let mut changed = task.clone();
        changed["projectId"] = json!("remote-project");
        assert!(store.save("task", changed.clone()).is_err());
        let saved = store
            .save_batch(
                vec![
                    (
                        "project",
                        json!({"id":"remote-project","name":"Remote","source":"zentao"}),
                    ),
                    ("task", changed),
                ],
                vec![],
                true,
            )
            .unwrap();
        assert_eq!(saved[1]["projectId"], "remote-project");
        let mut personal = saved[1].clone();
        personal["status"] = json!("doing");
        assert!(store.save("task", personal).is_ok());
        assert!(store.save("task", json!([])).is_err());
    }

    #[test]
    fn legacy_default_directory_is_moved_and_bootstrap_is_rewritten() {
        let sandbox = tempfile::tempdir().unwrap();
        let legacy = sandbox.path().join(".self-workbanch");
        let default = sandbox.path().join(".perch");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(
            legacy.join("current.json"),
            r#"{"workspaceId":"01a07c49-45ee-7810-9256-f0f6148f33cf","generationId":"01a07c49-45ee-7810-9256-f104dd4c47ad"}"#,
        )
        .unwrap();
        let bootstrap = serde_json::json!({
            "configVersion": 1,
            "dataRoot": legacy,
            "workspaceId": "01a07c49-45ee-7810-9256-f0f6148f33cf"
        });
        fs::write(
            legacy.join("bootstrap.json"),
            serde_json::to_vec(&bootstrap).unwrap(),
        )
        .unwrap();
        migrate_legacy_default(&legacy, &default).unwrap();
        assert!(!legacy.exists());
        let config: Value =
            serde_json::from_slice(&fs::read(default.join("bootstrap.json")).unwrap()).unwrap();
        assert_eq!(config["dataRoot"], json!(default));
        assert!(default.join("current.json").exists());
    }
}
