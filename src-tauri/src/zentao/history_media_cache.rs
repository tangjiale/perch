//! 历史图片短期授权与有界内存缓存；不缓存凭据，不持久化图片。
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};
const IMAGE_LIMIT: usize = 32 * 1024 * 1024;
struct Entry {
    key: String,
    value: Value,
    at: Instant,
    size: usize,
}
#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    size: usize,
}
impl Cache {
    fn remove(&mut self, key: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            self.size -= self.entries.remove(i).unwrap().size;
        }
    }
    fn get(&mut self, key: &str, ttl: Duration) -> Option<Value> {
        let i = self.entries.iter().position(|e| e.key == key)?;
        let entry = self.entries.remove(i)?;
        self.size -= entry.size;
        if entry.at.elapsed() > ttl {
            return None;
        }
        let value = entry.value.clone();
        self.size += entry.size;
        self.entries.push_back(entry);
        Some(value)
    }
    fn put(&mut self, key: String, value: Value, max: usize, count: usize) {
        self.remove(&key);
        let size = key.len() + value.to_string().len();
        if size > max {
            return;
        }
        while self.size + size > max || self.entries.len() >= count {
            if let Some(e) = self.entries.pop_front() {
                self.size -= e.size;
            } else {
                break;
            }
        }
        self.size += size;
        self.entries.push_back(Entry {
            key,
            value,
            size,
            at: Instant::now(),
        });
    }
}
static HISTORY: OnceLock<Mutex<Cache>> = OnceLock::new();
static IMAGES: OnceLock<Mutex<Cache>> = OnceLock::new();
type ImageLocks = Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>;
static LOCKS: OnceLock<ImageLocks> = OnceLock::new();
fn fingerprint(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
#[cfg(test)]
pub(super) fn expire_history(key: &str) {
    if let Ok(mut cache) = HISTORY.get_or_init(Default::default).lock() {
        if let Some(entry) = cache.entries.iter_mut().find(|entry| entry.key == key) {
            entry.at = Instant::now() - Duration::from_secs(21);
        }
    }
}
pub(super) fn scope(workspace: &str, generation: &Path, connection: &Value) -> String {
    fingerprint(&serde_json::json!([workspace, generation, connection]))
}
pub(super) fn history_key(scope: &str, bug: &Value) -> String {
    fingerprint(&serde_json::json!([scope, bug]))
}
pub(super) fn invalidate(key: &str) {
    if let Ok(mut c) = HISTORY.get_or_init(Default::default).lock() {
        c.remove(key);
    }
}
pub(super) fn history(key: &str) -> Option<Value> {
    HISTORY
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .get(key, Duration::from_secs(20))
}
pub(super) fn put_history(key: String, value: Value) {
    if let Ok(mut c) = HISTORY.get_or_init(Default::default).lock() {
        c.put(key, value, 8 * 1024 * 1024, 64);
    }
}
pub(super) fn image(key: &str) -> Option<String> {
    IMAGES
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .get(key, Duration::from_secs(300))?
        .as_str()
        .map(str::to_owned)
}
pub(super) fn put_image(key: String, value: String) {
    if let Ok(mut c) = IMAGES.get_or_init(Default::default).lock() {
        c.put(key, Value::String(value), IMAGE_LIMIT, 64);
    }
}
pub(super) fn image_key(scope: &str, id: &str) -> String {
    fingerprint(&serde_json::json!([scope, id]))
}
pub(super) fn lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    locks.retain(|_, v| v.strong_count() > 0);
    if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(key.to_owned(), Arc::downgrade(&lock));
    lock
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ttl_capacity_and_scope_are_bounded() {
        let mut c = Cache::default();
        c.put("a".into(), Value::String("1234".into()), 12, 2);
        c.put("b".into(), Value::String("1234".into()), 12, 2);
        c.put("c".into(), Value::String("1234".into()), 12, 2);
        assert!(c.get("a", Duration::from_secs(1)).is_none());
        assert!(c.size <= 12);
        c.entries.back_mut().unwrap().at = Instant::now() - Duration::from_secs(21);
        assert!(c.get("c", Duration::from_secs(20)).is_none());
        let a = scope("w", Path::new("a"), &serde_json::json!({"account":"a"}));
        assert_ne!(
            a,
            scope("w", Path::new("b"), &serde_json::json!({"account":"a"}))
        );
        assert_ne!(
            a,
            scope("w", Path::new("a"), &serde_json::json!({"account":"b"}))
        );
        assert_ne!(
            history_key(&a, &serde_json::json!({"id":1})),
            history_key(&a, &serde_json::json!({"id":2}))
        );
    }
}
