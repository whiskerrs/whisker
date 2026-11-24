//! Fetched bytes kept in memory and, off the web, on disk across launches.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const MEMORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Memory {
    entries: HashMap<String, (Rc<Vec<u8>>, u64)>,
    bytes: usize,
    tick: u64,
}

thread_local! {
    static MEMORY: RefCell<Memory> = RefCell::new(Memory::default());
}

pub(crate) fn key(url: &str, range: Option<(u64, u64)>) -> String {
    match range {
        Some((offset, length)) => format!("{url}#{offset}+{length}"),
        None => url.to_string(),
    }
}

pub(crate) fn memory_get(key: &str) -> Option<Vec<u8>> {
    MEMORY.with(|memory| {
        let mut memory = memory.borrow_mut();
        memory.tick += 1;
        let tick = memory.tick;
        let (bytes, used) = memory.entries.get_mut(key)?;
        *used = tick;
        Some(bytes.as_ref().clone())
    })
}

pub(crate) fn memory_put(key: String, bytes: &[u8]) {
    if bytes.len() > MEMORY_BYTES / 8 {
        return;
    }
    MEMORY.with(|memory| {
        let mut memory = memory.borrow_mut();
        memory.tick += 1;
        let tick = memory.tick;
        memory.bytes += bytes.len();
        if let Some((old, _)) = memory.entries.insert(key, (Rc::new(bytes.to_vec()), tick)) {
            memory.bytes -= old.len();
        }
        while memory.bytes > MEMORY_BYTES {
            let Some(oldest) = memory
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((old, _)) = memory.entries.remove(&oldest) {
                memory.bytes -= old.len();
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod disk {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::{Duration, SystemTime};

    /// How long a cached response is used without asking the server again.
    const FRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);
    const MAX_BYTES: u64 = 512 * 1024 * 1024;

    fn dir() -> Option<&'static PathBuf> {
        static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = platform_cache_dir()?.join("whisker-map");
            std::fs::create_dir_all(&dir).ok()?;
            let pruned = dir.clone();
            std::thread::spawn(move || prune(&pruned));
            Some(dir)
        })
        .as_ref()
    }

    fn platform_cache_dir() -> Option<PathBuf> {
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if cfg!(target_os = "android") {
            // The app's own cache directory, found from its package name.
            let cmdline = std::fs::read("/proc/self/cmdline").ok()?;
            let package = String::from_utf8_lossy(cmdline.split(|b| *b == 0).next()?).into_owned();
            let package = package.split(':').next()?.to_string();
            return Some(PathBuf::from(format!("/data/data/{package}/cache")));
        }
        if cfg!(any(target_os = "ios", target_os = "macos")) {
            return env("HOME").map(|home| home.join("Library/Caches"));
        }
        if cfg!(target_os = "windows") {
            return env("LOCALAPPDATA");
        }
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|home| home.join(".cache")))
    }

    fn path(dir: &Path, key: &str) -> PathBuf {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in key.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        dir.join(format!("{hash:016x}"))
    }

    /// The cached bytes and whether they are still fresh.
    pub(crate) fn read(key: &str) -> Option<(Vec<u8>, bool)> {
        read_in(dir()?, key)
    }

    pub(crate) fn write(key: &str, bytes: &[u8]) {
        if let Some(dir) = dir() {
            write_in(dir, key, bytes);
        }
    }

    /// The file starts with the key and a NUL, so a hash collision reads as a miss.
    fn read_in(dir: &Path, key: &str) -> Option<(Vec<u8>, bool)> {
        let path = path(dir, key);
        let data = std::fs::read(&path).ok()?;
        let split = data.iter().position(|b| *b == 0)?;
        if &data[..split] != key.as_bytes() {
            return None;
        }
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::MAX);
        Some((data[split + 1..].to_vec(), age < FRESH))
    }

    fn write_in(dir: &Path, key: &str, bytes: &[u8]) {
        let path = path(dir, key);
        let temp = path.with_extension("part");
        let mut data = Vec::with_capacity(key.len() + 1 + bytes.len());
        data.extend_from_slice(key.as_bytes());
        data.push(0);
        data.extend_from_slice(bytes);
        if std::fs::write(&temp, data).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }

    /// Deletes the least recently written files beyond the size limit.
    fn prune(dir: &PathBuf) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                Some((entry.path(), meta.len(), meta.modified().ok()?))
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        files.sort_by_key(|(_, _, modified)| *modified);
        for (path, len, _) in files {
            if total <= MAX_BYTES {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn disk_entries_round_trip_and_reject_other_keys() {
            let dir =
                std::env::temp_dir().join(format!("whisker-map-cache-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            write_in(&dir, "https://a/1", b"tile");
            assert_eq!(read_in(&dir, "https://a/1"), Some((b"tile".to_vec(), true)));
            assert_eq!(read_in(&dir, "https://a/2"), None);
            std::fs::write(path(&dir, "https://a/3"), b"https://a/4\0x").unwrap();
            assert_eq!(read_in(&dir, "https://a/3"), None);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keeps_recent_entries() {
        memory_put("a".into(), b"one");
        assert_eq!(memory_get("a").as_deref(), Some(&b"one"[..]));
        assert_eq!(memory_get("b"), None);
        assert_ne!(key("u", Some((0, 10))), key("u", None));
    }
}
