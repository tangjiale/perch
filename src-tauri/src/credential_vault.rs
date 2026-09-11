//! macOS 同一工作空间共用一个钥匙串条目；Windows 保持独立凭据，避免条目容量限制。
#[cfg(not(target_os = "macos"))]
pub(crate) type Entry = keyring::Entry;

pub(crate) fn entry(workspace: &str, service: &str, id: &str) -> keyring::Result<Entry> {
    #[cfg(target_os = "macos")]
    {
        Entry::new(workspace, service, id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        keyring::Entry::new(service, &format!("{workspace}:{id}"))
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::Entry;

#[cfg(target_os = "macos")]
mod macos {
    use keyring::{Error, Result};
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        sync::{Mutex, OnceLock},
    };

    const SERVICE: &str = "com.self.workbench.vault.v1";
    // 不派生 Debug，错误和日志不得包含凭据。
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Vault {
        version: u32,
        entries: BTreeMap<String, Option<String>>,
    }
    impl Default for Vault {
        fn default() -> Self {
            Self {
                version: 1,
                entries: BTreeMap::new(),
            }
        }
    }
    trait Backend {
        fn lock(&self, workspace: &str) -> Result<Option<std::fs::File>>;
        fn read(&self, service: &str, account: &str) -> Result<String>;
        fn write(&self, service: &str, account: &str, value: &str) -> Result<()>;
    }
    struct Keychain;
    impl Backend for Keychain {
        fn lock(&self, workspace: &str) -> Result<Option<std::fs::File>> {
            use sha2::{Digest, Sha256};
            let directory = dirs::cache_dir()
                .ok_or_else(unavailable)?
                .join("com.self.workbench/credential-locks");
            std::fs::create_dir_all(&directory).map_err(|_| unavailable())?;
            let path = directory.join(format!(
                "{}.lock",
                hex::encode(Sha256::digest(workspace.as_bytes()))
            ));
            Ok(Some(lock_file(&path)?))
        }

        fn read(&self, service: &str, account: &str) -> Result<String> {
            keyring::Entry::new(service, account)?.get_password()
        }
        fn write(&self, service: &str, account: &str, value: &str) -> Result<()> {
            keyring::Entry::new(service, account)?.set_password(value)
        }
    }
    fn lock_file(path: &std::path::Path) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| unavailable())?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
            Error::NoStorageAccess(Box::new(std::io::Error::other(
                "该工作空间凭据库正在被另一应用进程使用，请先退出另一实例",
            )))
        })?;
        Ok(file)
    }
    fn unavailable() -> Error {
        Error::NoStorageAccess(Box::new(std::io::Error::other(
            "凭据库访问失败，请退出应用后重试授权",
        )))
    }
    fn invalid() -> Error {
        Error::Invalid("凭据库".into(), "格式或版本无效，未覆盖原数据".into())
    }
    #[derive(Default)]
    struct Session {
        vaults: HashMap<String, Vault>,
        locks: HashMap<String, std::fs::File>,
        blocked: HashSet<String>,
        blocked_legacy: HashSet<(String, String)>,
    }
    impl Session {
        fn load(&mut self, backend: &impl Backend, workspace: &str) -> Result<()> {
            if self.blocked.contains(workspace) {
                return Err(unavailable());
            }
            if self.vaults.contains_key(workspace) {
                return Ok(());
            }
            if !self.locks.contains_key(workspace) {
                if let Some(lock) = backend.lock(workspace)? {
                    self.locks.insert(workspace.into(), lock);
                }
            }
            let loaded = match backend.read(SERVICE, workspace) {
                Ok(raw) => serde_json::from_str::<Vault>(&raw)
                    .map_err(|_| invalid())
                    .and_then(|v| {
                        if v.version == 1 {
                            Ok(v)
                        } else {
                            Err(invalid())
                        }
                    }),
                Err(Error::NoEntry) => Ok(Vault::default()),
                Err(_) => Err(unavailable()),
            };
            match loaded {
                Ok(vault) => {
                    self.vaults.insert(workspace.into(), vault);
                    Ok(())
                }
                Err(error) => {
                    self.blocked.insert(workspace.into());
                    Err(error)
                }
            }
        }
        fn persist(
            &mut self,
            backend: &impl Backend,
            workspace: &str,
            key: &str,
            secret: Option<&str>,
        ) -> Result<()> {
            self.load(backend, workspace)?;
            let mut next = self.vaults[workspace].clone();
            next.entries.insert(key.into(), secret.map(str::to_owned));
            let raw = serde_json::to_string(&next).map_err(|_| invalid())?;
            // 系统写入成功后才更新缓存；删除使用墓碑，禁止旧条目再次复活。
            if backend.write(SERVICE, workspace, &raw).is_err() {
                return Err(unavailable());
            }
            self.vaults.insert(workspace.into(), next);
            Ok(())
        }
        fn read(&mut self, backend: &impl Backend, entry: &Entry) -> Result<String> {
            self.load(backend, &entry.workspace)?;
            if let Some(value) = self.vaults[&entry.workspace].entries.get(&entry.key) {
                return value.clone().ok_or(Error::NoEntry);
            }
            let legacy_key = (entry.workspace.clone(), entry.key.clone());
            if self.blocked_legacy.contains(&legacy_key) {
                return Err(unavailable());
            }
            let secret = match backend.read(&entry.service, &entry.account) {
                Ok(value) => Some(value),
                Err(Error::NoEntry) => None,
                Err(_) => {
                    self.blocked_legacy.insert(legacy_key);
                    return Err(unavailable());
                }
            };
            // 只迁移业务实际请求的旧凭据，不枚举钥匙串；保留旧项以避免破坏性清理。
            self.persist(backend, &entry.workspace, &entry.key, secret.as_deref())?;
            secret.ok_or(Error::NoEntry)
        }
    }
    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    pub(crate) struct Entry {
        workspace: String,
        service: String,
        account: String,
        key: String,
    }
    impl Entry {
        pub(crate) fn new(workspace: &str, service: &str, id: &str) -> Result<Self> {
            if workspace.is_empty() || id.is_empty() {
                return Err(invalid());
            }
            Ok(Self {
                workspace: workspace.into(),
                service: service.into(),
                account: format!("{workspace}:{id}"),
                key: serde_json::to_string(&(service, id)).map_err(|_| invalid())?,
            })
        }
        pub(crate) fn get_password(&self) -> Result<String> {
            SESSION
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| unavailable())?
                .read(&Keychain, self)
        }
        pub(crate) fn set_password(&self, value: &str) -> Result<()> {
            SESSION
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| unavailable())?
                .persist(&Keychain, &self.workspace, &self.key, Some(value))
        }
        pub(crate) fn delete_credential(&self) -> Result<()> {
            SESSION
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| unavailable())?
                .persist(&Keychain, &self.workspace, &self.key, None)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::{Cell, RefCell};
        #[derive(Default)]
        struct Fake {
            values: RefCell<HashMap<(String, String), String>>,
            reads: RefCell<Vec<(String, String)>>,
            fail_write: Cell<bool>,
            fail_read: Cell<bool>,
        }
        impl Backend for Fake {
            fn lock(&self, _: &str) -> Result<Option<std::fs::File>> {
                Ok(None)
            }
            fn read(&self, service: &str, account: &str) -> Result<String> {
                self.reads
                    .borrow_mut()
                    .push((service.into(), account.into()));
                if self.fail_read.get() {
                    return Err(unavailable());
                }
                self.values
                    .borrow()
                    .get(&(service.into(), account.into()))
                    .cloned()
                    .ok_or(Error::NoEntry)
            }
            fn write(&self, service: &str, account: &str, value: &str) -> Result<()> {
                if self.fail_write.get() {
                    return Err(unavailable());
                }
                self.values
                    .borrow_mut()
                    .insert((service.into(), account.into()), value.into());
                Ok(())
            }
        }
        #[test]
        fn migrates_once_then_one_read_after_restart() {
            let backend = Fake::default();
            let mut session = Session::default();
            let entries: Vec<_> = ["zentao", "mail:one:imap", "mail:two:imap"]
                .into_iter()
                .map(|id| Entry::new("w", "old", id).unwrap())
                .collect();
            for e in &entries {
                backend.write("old", &e.account, "test-secret").unwrap();
                assert!(session.read(&backend, e).is_ok());
            }
            backend.reads.borrow_mut().clear();
            let mut restarted = Session::default();
            for e in &entries {
                assert!(restarted.read(&backend, e).is_ok());
            }
            assert_eq!(backend.reads.borrow().len(), 1);
            assert_eq!(backend.reads.borrow()[0].0, SERVICE);
        }
        #[test]
        fn deletion_never_revives_legacy_and_scopes_stay_independent() {
            let backend = Fake::default();
            let mut session = Session::default();
            let a = Entry::new("w", "token", "same").unwrap();
            let b = Entry::new("w", "login", "same").unwrap();
            backend.write("token", &a.account, "old").unwrap();
            session.persist(&backend, "w", &a.key, None).unwrap();
            session
                .persist(&backend, "w", &b.key, Some("password"))
                .unwrap();
            let mut restarted = Session::default();
            assert!(matches!(restarted.read(&backend, &a), Err(Error::NoEntry)));
            assert_eq!(restarted.read(&backend, &b).unwrap(), "password");
            assert!(matches!(
                restarted.read(&backend, &Entry::new("other", "login", "same").unwrap()),
                Err(Error::NoEntry)
            ));
        }
        #[test]
        fn failed_write_keeps_previous_value_and_cancel_does_not_prompt_repeatedly() {
            let backend = Fake::default();
            let mut session = Session::default();
            let e = Entry::new("w", "old", "a").unwrap();
            session
                .persist(&backend, "w", &e.key, Some("previous"))
                .unwrap();
            backend.fail_write.set(true);
            assert!(session.persist(&backend, "w", &e.key, Some("new")).is_err());
            backend.fail_write.set(false);
            assert_eq!(Session::default().read(&backend, &e).unwrap(), "previous");
            let mut denied = Session::default();
            backend.fail_read.set(true);
            backend.reads.borrow_mut().clear();
            assert!(denied.read(&backend, &e).is_err());
            assert!(denied.read(&backend, &e).is_err());
            assert_eq!(backend.reads.borrow().len(), 1);
        }
        #[test]
        fn migration_write_failure_is_retryable_without_losing_old_credentials() {
            let backend = Fake::default();
            let mut session = Session::default();
            let entry = Entry::new("w", "old", "a").unwrap();
            backend.write("old", &entry.account, "legacy").unwrap();
            backend.fail_write.set(true);
            assert!(session.read(&backend, &entry).is_err());
            assert!(!session.vaults["w"].entries.contains_key(&entry.key));
            backend.fail_write.set(false);
            assert_eq!(session.read(&backend, &entry).unwrap(), "legacy");
        }
        #[test]
        fn exclusive_lock_prevents_two_cached_writers() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("workspace.lock");
            let first = lock_file(&path).unwrap();
            assert!(lock_file(&path).is_err());
            drop(first);
            assert!(lock_file(&path).is_ok());
        }
        #[test]
        fn malformed_vault_is_not_replaced_by_legacy() {
            let backend = Fake::default();
            backend.write(SERVICE, "w", "broken").unwrap();
            let mut session = Session::default();
            assert!(session
                .read(&backend, &Entry::new("w", "old", "a").unwrap())
                .is_err());
            assert_eq!(
                backend
                    .values
                    .borrow()
                    .get(&(SERVICE.into(), "w".into()))
                    .unwrap(),
                "broken"
            );
            assert_eq!(backend.reads.borrow().len(), 1);
        }
    }
}
