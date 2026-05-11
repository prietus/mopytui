use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const DEFAULT_TTL: Duration = Duration::from_secs(30 * 24 * 3600);

fn cache_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "mopytui")?;
    let dir = dirs.cache_dir().join("metadata");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn hash_key(key: &str) -> String {
    let mut h: u64 = 5381;
    for b in key.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    format!("{:x}", h)
}

fn file_path(key: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{}.cache", hash_key(key))))
}

pub fn get(key: &str, ttl: Duration) -> Option<Vec<u8>> {
    let path = file_path(key)?;
    let meta = std::fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    if SystemTime::now().duration_since(modified).ok()? > ttl {
        return None;
    }
    std::fs::read(&path).ok()
}

pub fn set(key: &str, data: &[u8]) {
    if let Some(path) = file_path(key) {
        let _ = std::fs::write(&path, data);
    }
}

pub fn get_default(key: &str) -> Option<Vec<u8>> {
    get(key, DEFAULT_TTL)
}
