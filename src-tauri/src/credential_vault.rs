//! 本地凭据使用随机主密钥和 AES-256-GCM 加密，密钥独立于工作空间与备份保存。
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const VERSION: u32 = 1;
const PREFIX: &str = "perch:v1:";
const VAULT_PURPOSE: &str = "credential-vault";
const KEY_BYTES: usize = 32;
const KEY_ID_PREFIX: &str = "perch:key-id:v1:";
const KEY_ID_BYTES: usize = KEY_ID_PREFIX.len() + 64;
const NONCE_BYTES: usize = 12;
const MAX_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CIPHERTEXT_BYTES: usize = 24 * 1024 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 16 * 1024;

/// 错误不包含路径、账号、密码、令牌、密钥或解密后的原文。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    NoEntry,
    InvalidInput,
    InvalidStorage,
    MissingKey,
    StorageUnavailable,
    Busy,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoEntry => "未保存该凭据，请重新输入",
            Self::InvalidInput => "凭据参数为空、格式无效或超过存储限制",
            Self::InvalidStorage => "本地加密数据或密钥无效，未覆盖原数据",
            Self::MissingKey => "本地加密数据的密钥已丢失，请恢复原密钥后重试",
            Self::StorageUnavailable => "本地加密存储访问失败，请检查应用数据目录权限后重试",
            Self::Busy => "本地凭据库正在被另一应用进程使用，请稍后重试",
        })
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vault {
    version: u32,
    entries: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedJson {
    encrypted: String,
}

// 不缓存密钥或明文；每次操作在进程内、跨进程两层锁内读取最新数据。
static PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone)]
struct Storage {
    root: PathBuf,
    #[cfg(test)]
    fail_before_commit: bool,
    #[cfg(test)]
    fail_key_id_commit: bool,
}

struct WorkspacePaths {
    key: PathBuf,
    key_id: PathBuf,
    vault: PathBuf,
    lock: PathBuf,
}

impl Storage {
    fn default() -> Result<Self> {
        #[cfg(not(test))]
        let root = dirs::data_local_dir()
            .ok_or(Error::StorageUnavailable)?
            .join("com.self.workbench")
            .join("credentials");
        #[cfg(test)]
        let root = {
            // 单元测试绝不接触用户数据目录或原有钥匙串。
            static TEST_ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
            TEST_ROOT
                .get_or_init(|| tempfile::tempdir().expect("创建凭据测试临时目录"))
                .path()
                .join("credentials")
        };
        Ok(Self {
            root,
            #[cfg(test)]
            fail_before_commit: false,
            #[cfg(test)]
            fail_key_id_commit: false,
        })
    }

    fn paths(&self, workspace: &str) -> WorkspacePaths {
        let name = hex::encode(Sha256::digest(workspace.as_bytes()));
        WorkspacePaths {
            key: self.root.join("keys").join(format!("{name}.key")),
            key_id: self.root.join("vaults").join(format!("{name}.key-id")),
            vault: self.root.join("vaults").join(format!("{name}.enc")),
            lock: self.root.join("vaults").join(format!("{name}.lock")),
        }
    }

    fn with_workspace<T>(
        &self,
        workspace: &str,
        action: impl FnOnce(&WorkspacePaths) -> Result<T>,
    ) -> Result<T> {
        validate_identifier(workspace)?;
        let _process_lock = PROCESS_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| Error::StorageUnavailable)?;
        if let Some(parent) = self.root.parent() {
            ensure_private_directory(parent)?;
        }
        ensure_private_directory(&self.root)?;
        ensure_private_directory(&self.root.join("keys"))?;
        ensure_private_directory(&self.root.join("vaults"))?;
        let paths = self.paths(workspace);
        let _file_lock = lock_workspace(&paths.lock)?;
        action(&paths)
    }

    fn key(
        &self,
        workspace: &str,
        paths: &WorkspacePaths,
        create: bool,
    ) -> Result<Option<[u8; KEY_BYTES]>> {
        if let Some(raw) = read_optional(&paths.key, KEY_BYTES)? {
            let key = raw.try_into().map_err(|_| Error::InvalidStorage)?;
            let expected = key_identifier(workspace, &key);
            match read_optional(&paths.key_id, KEY_ID_BYTES)? {
                Some(actual) if actual == expected.as_bytes() => {}
                Some(_) => return Err(Error::InvalidStorage),
                None => {
                    // 密钥写入后中断时可补齐标记；已有凭据必须先通过 AEAD 校验。
                    if let Some(raw) = read_optional(&paths.vault, MAX_CIPHERTEXT_BYTES)? {
                        let encrypted =
                            std::str::from_utf8(&raw).map_err(|_| Error::InvalidStorage)?;
                        open(&key, workspace, VAULT_PURPOSE, encrypted)?;
                    }
                    self.atomic_write(&paths.key_id, expected.as_bytes(), false)?;
                }
            }
            return Ok(Some(key));
        }
        // 数据库可单独保存密文，因此没有凭据文件时也必须检查初始化标记。
        if regular_file_exists(&paths.key_id)? || regular_file_exists(&paths.vault)? {
            return Err(Error::MissingKey);
        }
        if !create {
            return Ok(None);
        }
        let mut key = [0; KEY_BYTES];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| Error::StorageUnavailable)?;
        // 先写密钥再写标记，两者持久化成功后才允许返回密钥、生成业务密文。
        self.atomic_write(&paths.key, &key, false)?;
        self.atomic_write(
            &paths.key_id,
            key_identifier(workspace, &key).as_bytes(),
            false,
        )?;
        Ok(Some(key))
    }

    fn load_vault(
        &self,
        workspace: &str,
        paths: &WorkspacePaths,
    ) -> Result<(Vault, Option<[u8; KEY_BYTES]>)> {
        let key = self.key(workspace, paths, false)?;
        let Some(raw) = read_optional(&paths.vault, MAX_CIPHERTEXT_BYTES)? else {
            return Ok((
                Vault {
                    version: VERSION,
                    entries: BTreeMap::new(),
                },
                key,
            ));
        };
        let key = key.ok_or(Error::MissingKey)?;
        let encrypted = std::str::from_utf8(&raw).map_err(|_| Error::InvalidStorage)?;
        let plaintext = open(&key, workspace, VAULT_PURPOSE, encrypted)?;
        let vault: Vault = serde_json::from_str(&plaintext).map_err(|_| Error::InvalidStorage)?;
        if vault.version != VERSION {
            return Err(Error::InvalidStorage);
        }
        Ok((vault, Some(key)))
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
        if bytes.len() > MAX_CIPHERTEXT_BYTES {
            return Err(Error::InvalidInput);
        }
        // 禁止临时文件或目标文件通过符号链接改写其他位置。
        regular_file_exists(path)?;
        let parent = path.parent().ok_or(Error::StorageUnavailable)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::StorageUnavailable)?;
        set_private_file_permissions(temporary.as_file())?;
        temporary
            .write_all(bytes)
            .map_err(|_| Error::StorageUnavailable)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| Error::StorageUnavailable)?;
        #[cfg(test)]
        if self.fail_before_commit
            || (self.fail_key_id_commit
                && path
                    .extension()
                    .is_some_and(|extension| extension == "key-id"))
        {
            return Err(Error::StorageUnavailable);
        }
        // tempfile 在 Windows 上同样使用原子替换；首次密钥写入禁止覆盖已有文件。
        if replace {
            temporary
                .persist(path)
                .map_err(|_| Error::StorageUnavailable)?;
        } else {
            temporary
                .persist_noclobber(path)
                .map_err(|_| Error::StorageUnavailable)?;
        }
        #[cfg(unix)]
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| Error::StorageUnavailable)?;
        Ok(())
    }
}

fn key_identifier(workspace: &str, key: &[u8; KEY_BYTES]) -> String {
    let digest = Sha256::new()
        .chain_update(b"com.self.workbench.master-key-id.v1\0")
        .chain_update(workspace.as_bytes())
        .chain_update([0])
        .chain_update(key)
        .finalize();
    format!("{KEY_ID_PREFIX}{}", hex::encode(digest))
}

fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(Error::InvalidStorage)
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| Error::StorageUnavailable)?;
            let metadata = fs::symlink_metadata(path).map_err(|_| Error::StorageUnavailable)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::InvalidStorage);
            }
        }
        Err(_) => return Err(Error::StorageUnavailable),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::StorageUnavailable)?;
    }
    Ok(())
}

fn regular_file_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(Error::InvalidStorage),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::StorageUnavailable),
    }
}

fn set_private_file_permissions(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::StorageUnavailable)?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn open_regular(path: &Path, create: bool) -> Result<File> {
    regular_file_exists(path)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|_| Error::StorageUnavailable)?;
    if !file
        .metadata()
        .map_err(|_| Error::StorageUnavailable)?
        .is_file()
    {
        return Err(Error::InvalidStorage);
    }
    set_private_file_permissions(&file)?;
    Ok(file)
}

fn read_optional(path: &Path, maximum: usize) -> Result<Option<Vec<u8>>> {
    if !regular_file_exists(path)? {
        return Ok(None);
    }
    let file = open_regular(path, false)?;
    if file
        .metadata()
        .map_err(|_| Error::StorageUnavailable)?
        .len()
        > maximum as u64
    {
        return Err(Error::InvalidStorage);
    }
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::StorageUnavailable)?;
    if bytes.len() > maximum {
        return Err(Error::InvalidStorage);
    }
    Ok(Some(bytes))
}

fn lock_workspace(path: &Path) -> Result<File> {
    let file = open_regular(path, true)?;
    let started = std::time::Instant::now();
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= std::time::Duration::from_secs(5) {
                    return Err(Error::Busy);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return Err(Error::StorageUnavailable),
        }
    }
}

fn associated_data(workspace: &str, purpose: &str) -> Result<Vec<u8>> {
    validate_identifier(workspace)?;
    validate_identifier(purpose)?;
    serde_json::to_vec(&(
        "com.self.workbench.local-credentials",
        VERSION,
        workspace,
        purpose,
    ))
    .map_err(|_| Error::InvalidInput)
}

fn seal(key: &[u8; KEY_BYTES], workspace: &str, purpose: &str, plaintext: &str) -> Result<String> {
    if plaintext.len() > MAX_PLAINTEXT_BYTES {
        return Err(Error::InvalidInput);
    }
    let aad = associated_data(workspace, purpose)?;
    let mut nonce = [0; NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| Error::StorageUnavailable)?;
    let key =
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::InvalidStorage)?);
    let mut bytes = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(aad),
        &mut bytes,
    )
    .map_err(|_| Error::InvalidStorage)?;
    Ok(format!(
        "{PREFIX}{}:{}",
        STANDARD.encode(nonce),
        STANDARD.encode(bytes)
    ))
}

fn open(key: &[u8; KEY_BYTES], workspace: &str, purpose: &str, ciphertext: &str) -> Result<String> {
    if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
        return Err(Error::InvalidStorage);
    }
    let aad = associated_data(workspace, purpose)?;
    let (nonce, encoded) = ciphertext
        .strip_prefix(PREFIX)
        .and_then(|value| value.split_once(':'))
        .ok_or(Error::InvalidStorage)?;
    if nonce.len() != 16 {
        return Err(Error::InvalidStorage);
    }
    let nonce: [u8; NONCE_BYTES] = STANDARD
        .decode(nonce)
        .map_err(|_| Error::InvalidStorage)?
        .try_into()
        .map_err(|_| Error::InvalidStorage)?;
    let mut bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error::InvalidStorage)?;
    if bytes.len() < AES_256_GCM.tag_len()
        || bytes.len() > MAX_PLAINTEXT_BYTES + AES_256_GCM.tag_len()
    {
        return Err(Error::InvalidStorage);
    }
    let key =
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::InvalidStorage)?);
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut bytes,
        )
        .map_err(|_| Error::InvalidStorage)?;
    String::from_utf8(plaintext.to_vec()).map_err(|_| Error::InvalidStorage)
}

pub(crate) struct Entry {
    storage: Storage,
    workspace: String,
    key: String,
}

pub(crate) fn entry(workspace: &str, service: &str, id: &str) -> Result<Entry> {
    validate_identifier(workspace)?;
    validate_identifier(service)?;
    validate_identifier(id)?;
    Ok(Entry {
        storage: Storage::default()?,
        workspace: workspace.into(),
        key: serde_json::to_string(&(service, id)).map_err(|_| Error::InvalidInput)?,
    })
}

impl Entry {
    pub(crate) fn get_password(&self) -> Result<String> {
        self.storage.with_workspace(&self.workspace, |paths| {
            let (vault, _) = self.storage.load_vault(&self.workspace, paths)?;
            vault.entries.get(&self.key).cloned().ok_or(Error::NoEntry)
        })
    }

    pub(crate) fn set_password(&self, password: &str) -> Result<()> {
        if password.len() > MAX_PLAINTEXT_BYTES {
            return Err(Error::InvalidInput);
        }
        self.storage.with_workspace(&self.workspace, |paths| {
            let (mut vault, key) = self.storage.load_vault(&self.workspace, paths)?;
            let key = match key {
                Some(key) => key,
                None => self
                    .storage
                    .key(&self.workspace, paths, true)?
                    .ok_or(Error::MissingKey)?,
            };
            vault.entries.insert(self.key.clone(), password.into());
            let plaintext = serde_json::to_string(&vault).map_err(|_| Error::InvalidStorage)?;
            let encrypted = seal(&key, &self.workspace, VAULT_PURPOSE, &plaintext)?;
            self.storage
                .atomic_write(&paths.vault, encrypted.as_bytes(), true)
        })
    }

    pub(crate) fn delete_credential(&self) -> Result<()> {
        self.storage.with_workspace(&self.workspace, |paths| {
            let (mut vault, key) = self.storage.load_vault(&self.workspace, paths)?;
            if vault.entries.remove(&self.key).is_none() {
                return Err(Error::NoEntry);
            }
            let key = key.ok_or(Error::MissingKey)?;
            let plaintext = serde_json::to_string(&vault).map_err(|_| Error::InvalidStorage)?;
            let encrypted = seal(&key, &self.workspace, VAULT_PURPOSE, &plaintext)?;
            self.storage
                .atomic_write(&paths.vault, encrypted.as_bytes(), true)
        })
    }
}

pub(crate) fn encrypt(
    workspace: &str,
    purpose: &str,
    plaintext: &str,
) -> std::result::Result<String, String> {
    (|| {
        validate_identifier(purpose)?;
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(Error::InvalidInput);
        }
        let storage = Storage::default()?;
        storage.with_workspace(workspace, |paths| {
            let (_, key) = storage.load_vault(workspace, paths)?;
            let key = match key {
                Some(key) => key,
                None => storage
                    .key(workspace, paths, true)?
                    .ok_or(Error::MissingKey)?,
            };
            seal(&key, workspace, purpose, plaintext)
        })
    })()
    .map_err(|error: Error| error.to_string())
}

pub(crate) fn decrypt(
    workspace: &str,
    purpose: &str,
    ciphertext: &str,
) -> std::result::Result<String, String> {
    (|| {
        let storage = Storage::default()?;
        storage.with_workspace(workspace, |paths| {
            let key = storage
                .key(workspace, paths, false)?
                .ok_or(Error::MissingKey)?;
            open(&key, workspace, purpose, ciphertext)
        })
    })()
    .map_err(|error: Error| error.to_string())
}

pub(crate) fn encrypt_json(
    workspace: &str,
    purpose: &str,
    value: &serde_json::Value,
) -> std::result::Result<String, String> {
    let plaintext = serde_json::to_string(value).map_err(|_| Error::InvalidInput.to_string())?;
    let encrypted = encrypt(workspace, purpose, &plaintext)?;
    serde_json::to_string(&EncryptedJson { encrypted }).map_err(|_| Error::InvalidInput.to_string())
}

pub(crate) fn decrypt_json(
    workspace: &str,
    purpose: &str,
    raw: &str,
) -> std::result::Result<serde_json::Value, String> {
    if raw.len() > MAX_CIPHERTEXT_BYTES + 64 {
        return Err(Error::InvalidStorage.to_string());
    }
    let wrapped: EncryptedJson =
        serde_json::from_str(raw).map_err(|_| Error::InvalidStorage.to_string())?;
    let plaintext = decrypt(workspace, purpose, &wrapped.encrypted)?;
    serde_json::from_str(&plaintext).map_err(|_| Error::InvalidStorage.to_string())
}

#[cfg(test)]
pub(crate) fn test_key_path(workspace: &str) -> PathBuf {
    Storage::default().unwrap().paths(workspace).key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Entry) {
        let root = tempfile::tempdir().unwrap();
        let entry = Entry {
            storage: Storage {
                root: root.path().join("credentials"),
                fail_before_commit: false,
                fail_key_id_commit: false,
            },
            workspace: uuid::Uuid::now_v7().to_string(),
            key: serde_json::to_string(&("test-service", "sensitive-account")).unwrap(),
        };
        (root, entry)
    }

    #[test]
    fn stores_only_ciphertext_and_reopens_with_same_key() {
        let (_root, entry) = fixture();
        entry.set_password("sensitive-password").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        let first = fs::read(&paths.vault).unwrap();
        assert!(!String::from_utf8_lossy(&first).contains("sensitive-account"));
        assert!(!String::from_utf8_lossy(&first).contains("sensitive-password"));
        assert_eq!(fs::read(&paths.key).unwrap().len(), KEY_BYTES);
        let reopened = Entry {
            storage: entry.storage.clone(),
            workspace: entry.workspace.clone(),
            key: entry.key.clone(),
        };
        assert_eq!(reopened.get_password().unwrap(), "sensitive-password");
        reopened.set_password("sensitive-password").unwrap();
        assert_ne!(first, fs::read(&paths.vault).unwrap());
    }

    #[test]
    fn namespaces_are_isolated_and_delete_does_not_restore() {
        let (_root, entry) = fixture();
        let other = Entry {
            storage: entry.storage.clone(),
            workspace: entry.workspace.clone(),
            key: serde_json::to_string(&("other-service", "sensitive-account")).unwrap(),
        };
        assert_eq!(entry.get_password(), Err(Error::NoEntry));
        entry.set_password("first").unwrap();
        other.set_password("second").unwrap();
        assert_eq!(entry.get_password().unwrap(), "first");
        entry.delete_credential().unwrap();
        assert_eq!(entry.get_password(), Err(Error::NoEntry));
        assert_eq!(entry.delete_credential(), Err(Error::NoEntry));
        assert_eq!(other.get_password().unwrap(), "second");
    }

    #[test]
    fn altered_ciphertext_is_rejected_without_overwrite_or_cached_failure() {
        let (_root, entry) = fixture();
        entry.set_password("secret").unwrap();
        let path = entry.storage.paths(&entry.workspace).vault;
        let valid = fs::read(&path).unwrap();
        let mut tampered = valid.clone();
        let index = tampered.len() - 5;
        tampered[index] = if tampered[index] == b'A' { b'B' } else { b'A' };
        fs::write(&path, &tampered).unwrap();
        assert_eq!(entry.get_password(), Err(Error::InvalidStorage));
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
        assert_eq!(fs::read(&path).unwrap(), tampered);
        fs::write(&path, &valid).unwrap();
        assert_eq!(entry.get_password().unwrap(), "secret");
    }

    #[test]
    fn missing_corrupt_or_wrong_key_never_replaces_ciphertext() {
        let (_root, entry) = fixture();
        entry.set_password("secret").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        let ciphertext = fs::read(&paths.vault).unwrap();
        let correct_key = fs::read(&paths.key).unwrap();
        fs::remove_file(&paths.key).unwrap();
        assert_eq!(entry.get_password(), Err(Error::MissingKey));
        assert_eq!(entry.set_password("replacement"), Err(Error::MissingKey));
        assert!(!paths.key.exists());
        fs::write(&paths.key, b"broken").unwrap();
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
        fs::write(&paths.key, [0_u8; KEY_BYTES]).unwrap();
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
        assert_eq!(fs::read(&paths.vault).unwrap(), ciphertext);
        fs::write(&paths.key, &correct_key).unwrap();
        assert_eq!(entry.get_password().unwrap(), "secret");
    }

    #[test]
    fn failed_commit_preserves_existing_ciphertext_and_key() {
        let (_root, mut entry) = fixture();
        entry.set_password("original").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        let original = fs::read(&paths.vault).unwrap();
        let key = fs::read(&paths.key).unwrap();
        entry.storage.fail_before_commit = true;
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::StorageUnavailable)
        );
        assert_eq!(entry.delete_credential(), Err(Error::StorageUnavailable));
        assert_eq!(fs::read(&paths.vault).unwrap(), original);
        assert_eq!(fs::read(&paths.key).unwrap(), key);
        assert_eq!(entry.get_password().unwrap(), "original");
        assert_eq!(
            fs::read_dir(paths.vault.parent().unwrap()).unwrap().count(),
            3
        );
    }

    #[test]
    fn parallel_entries_are_not_lost() {
        let (_root, entry) = fixture();
        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for index in 0..16 {
                let storage = entry.storage.clone();
                let workspace = entry.workspace.clone();
                workers.push(scope.spawn(move || {
                    Entry {
                        storage,
                        workspace,
                        key: format!("entry-{index}"),
                    }
                    .set_password(&format!("secret-{index}"))
                }));
            }
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        for index in 0..16 {
            let saved = Entry {
                storage: entry.storage.clone(),
                workspace: entry.workspace.clone(),
                key: format!("entry-{index}"),
            };
            assert_eq!(saved.get_password().unwrap(), format!("secret-{index}"));
        }
    }

    #[test]
    fn public_json_api_requires_envelope_and_binds_workspace_and_purpose() {
        let workspace = uuid::Uuid::now_v7().to_string();
        let value =
            serde_json::json!({"account": "private-account", "password": "private-password"});
        let first = encrypt_json(&workspace, "settings:account", &value).unwrap();
        let second = encrypt_json(&workspace, "settings:account", &value).unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("private-account"));
        assert!(!first.contains("private-password"));
        assert_eq!(
            decrypt_json(&workspace, "settings:account", &first).unwrap(),
            value
        );
        assert!(decrypt_json(&workspace, "settings:other", &first).is_err());
        let other_workspace = uuid::Uuid::now_v7().to_string();
        encrypt(&other_workspace, "settings:account", "unrelated").unwrap();
        assert!(decrypt_json(&other_workspace, "settings:account", &first).is_err());
        assert!(decrypt_json(&workspace, "settings:account", &value.to_string()).is_err());
        let mut wrapped: serde_json::Value = serde_json::from_str(&first).unwrap();
        wrapped["extra"] = true.into();
        assert!(decrypt_json(&workspace, "settings:account", &wrapped.to_string()).is_err());
    }

    #[test]
    fn public_api_does_not_recreate_key_when_vault_ciphertext_exists() {
        let workspace = uuid::Uuid::now_v7().to_string();
        let credential = entry(&workspace, "test", "id").unwrap();
        credential.set_password("secret").unwrap();
        let paths = credential.storage.paths(&workspace);
        fs::remove_file(test_key_path(&workspace)).unwrap();
        assert!(encrypt(&workspace, "settings", "text").is_err());
        assert!(!paths.key.exists());
    }

    #[test]
    fn database_only_workspace_refuses_missing_key_without_creating_replacement() {
        let workspace = uuid::Uuid::now_v7().to_string();
        let ciphertext = encrypt(&workspace, "provider-config", "provider-secret").unwrap();
        let storage = Storage::default().unwrap();
        let paths = storage.paths(&workspace);
        let key = fs::read(&paths.key).unwrap();
        let marker = fs::read(&paths.key_id).unwrap();
        assert!(!paths.vault.exists());
        assert_eq!(marker.len(), KEY_ID_BYTES);
        assert!(!String::from_utf8_lossy(&marker).contains(&hex::encode(&key)));

        let moved_key = paths.key.with_extension("saved-key");
        fs::rename(&paths.key, &moved_key).unwrap();
        assert!(encrypt(&workspace, "provider-config", "replacement").is_err());
        assert!(decrypt(&workspace, "provider-config", &ciphertext).is_err());
        assert!(!paths.key.exists());
        assert_eq!(fs::read(&paths.key_id).unwrap(), marker);
        assert!(!paths.vault.exists());

        fs::rename(&moved_key, &paths.key).unwrap();
        assert_eq!(
            decrypt(&workspace, "provider-config", &ciphertext).unwrap(),
            "provider-secret"
        );
        assert!(encrypt(&workspace, "provider-config", "new secret").is_ok());
        assert_eq!(fs::read(&paths.key).unwrap(), key);
    }

    #[test]
    fn database_only_workspace_rejects_wrong_key_and_corrupt_marker() {
        let workspace = uuid::Uuid::now_v7().to_string();
        encrypt(&workspace, "provider-config", "provider-secret").unwrap();
        let paths = Storage::default().unwrap().paths(&workspace);
        let key = fs::read(&paths.key).unwrap();
        let marker = fs::read(&paths.key_id).unwrap();
        fs::write(&paths.key, [0_u8; KEY_BYTES]).unwrap();
        assert!(encrypt(&workspace, "provider-config", "replacement").is_err());
        assert_eq!(fs::read(&paths.key_id).unwrap(), marker);
        fs::write(&paths.key, &key).unwrap();
        fs::write(&paths.key_id, b"broken").unwrap();
        assert!(encrypt(&workspace, "provider-config", "replacement").is_err());
        assert_eq!(fs::read(&paths.key_id).unwrap(), b"broken");
        assert_eq!(fs::read(&paths.key).unwrap(), key);
        assert!(!paths.vault.exists());
    }

    #[test]
    fn interrupted_key_initialization_reuses_key_before_creating_ciphertext() {
        let (_root, mut entry) = fixture();
        entry.storage.fail_key_id_commit = true;
        assert_eq!(entry.set_password("secret"), Err(Error::StorageUnavailable));
        let paths = entry.storage.paths(&entry.workspace);
        let original_key = fs::read(&paths.key).unwrap();
        assert_eq!(original_key.len(), KEY_BYTES);
        assert!(!paths.key_id.exists());
        assert!(!paths.vault.exists());

        entry.storage.fail_key_id_commit = false;
        entry.set_password("secret").unwrap();
        assert_eq!(fs::read(&paths.key).unwrap(), original_key);
        assert!(paths.key_id.exists());
        assert_eq!(entry.get_password().unwrap(), "secret");
    }

    #[test]
    fn missing_marker_checks_existing_vault_before_remembering_key() {
        let (_root, entry) = fixture();
        entry.set_password("secret").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        let original_key = fs::read(&paths.key).unwrap();
        let original_marker = fs::read(&paths.key_id).unwrap();
        fs::remove_file(&paths.key_id).unwrap();
        fs::write(&paths.key, [0_u8; KEY_BYTES]).unwrap();
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
        assert!(!paths.key_id.exists());

        fs::write(&paths.key, original_key).unwrap();
        assert_eq!(entry.get_password().unwrap(), "secret");
        assert_eq!(fs::read(&paths.key_id).unwrap(), original_marker);
    }

    #[test]
    fn public_encrypt_rejects_corrupt_existing_vault() {
        let workspace = uuid::Uuid::now_v7().to_string();
        let credential = entry(&workspace, "test", "id").unwrap();
        credential.set_password("secret").unwrap();
        let paths = credential.storage.paths(&workspace);
        fs::write(&paths.vault, "damaged").unwrap();
        assert!(encrypt(&workspace, "settings", "new text").is_err());
        assert_eq!(fs::read(&paths.vault).unwrap(), b"damaged");
    }

    #[test]
    fn same_key_cannot_decrypt_different_workspace_or_purpose() {
        let key = [9; KEY_BYTES];
        let ciphertext = seal(&key, "workspace-a", "purpose-a", "secret").unwrap();
        assert_eq!(
            open(&key, "workspace-a", "purpose-a", &ciphertext).unwrap(),
            "secret"
        );
        assert_eq!(
            open(&key, "workspace-b", "purpose-a", &ciphertext),
            Err(Error::InvalidStorage)
        );
        assert_eq!(
            open(&key, "workspace-a", "purpose-b", &ciphertext),
            Err(Error::InvalidStorage)
        );
    }

    #[test]
    fn oversized_file_and_unknown_version_are_rejected() {
        let (_root, entry) = fixture();
        entry.set_password("original").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        let file = OpenOptions::new().write(true).open(&paths.vault).unwrap();
        file.set_len((MAX_CIPHERTEXT_BYTES + 1) as u64).unwrap();
        assert_eq!(entry.get_password(), Err(Error::InvalidStorage));
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
        fs::write(&paths.vault, b"perch:v2:unsupported").unwrap();
        assert_eq!(entry.get_password(), Err(Error::InvalidStorage));
        assert_eq!(
            entry.set_password("replacement"),
            Err(Error::InvalidStorage)
        );
    }

    #[test]
    fn lock_is_held_until_handle_is_dropped() {
        let (_root, entry) = fixture();
        entry.set_password("secret").unwrap();
        let path = entry.storage.paths(&entry.workspace).lock;
        let first = lock_workspace(&path).unwrap();
        let second = open_regular(&path, true).unwrap();
        assert!(fs2::FileExt::try_lock_exclusive(&second).is_err());
        drop(first);
        assert!(fs2::FileExt::try_lock_exclusive(&second).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn unix_files_and_directories_have_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, entry) = fixture();
        entry.set_password("secret").unwrap();
        let paths = entry.storage.paths(&entry.workspace);
        for path in [&paths.key, &paths.key_id, &paths.vault, &paths.lock] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        for path in [
            &entry.storage.root,
            &entry.storage.root.join("keys"),
            &entry.storage.root.join("vaults"),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_key_key_id_vault_or_lock_is_rejected() {
        use std::os::unix::fs::symlink;
        for target in ["key", "key-id", "vault", "lock"] {
            let (root, entry) = fixture();
            entry.set_password("secret").unwrap();
            let paths = entry.storage.paths(&entry.workspace);
            let selected = match target {
                "key" => paths.key,
                "key-id" => paths.key_id,
                "vault" => paths.vault,
                _ => paths.lock,
            };
            let outside = root.path().join("outside");
            fs::rename(&selected, &outside).unwrap();
            let original = fs::read(&outside).unwrap();
            symlink(&outside, &selected).unwrap();
            assert_eq!(entry.get_password(), Err(Error::InvalidStorage));
            assert_eq!(
                entry.set_password("replacement"),
                Err(Error::InvalidStorage)
            );
            assert_eq!(fs::read(&outside).unwrap(), original);
        }
    }
}
