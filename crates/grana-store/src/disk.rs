use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

const STAGING: &str = "staging";

static NONCE: AtomicU64 = AtomicU64::new(0);
static ACCOUNTING: OnceLock<Mutex<HashMap<PathBuf, Accounting>>> = OnceLock::new();

type Accounting = Arc<Mutex<Option<u64>>>;

pub struct DiskEntry {
    directory: String,
    file: String,
}

impl DiskEntry {
    pub fn new(directory: impl Into<String>, file: impl Into<String>) -> Self {
        let directory = directory.into();
        let file = file.into();
        assert!(
            segment(&directory),
            "a disk cache directory is one path segment"
        );
        assert!(segment(&file), "a disk cache file is one path segment");
        Self { directory, file }
    }
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|character| character == '/' || character == '\\' || character.is_control())
}

pub struct DiskCache {
    root: PathBuf,
    staging: PathBuf,
    bytes: Accounting,
    recent: Mutex<HashMap<PathBuf, u64>>,
    tick: AtomicU64,
    budget: u64,
    retain: u64,
}

impl DiskCache {
    pub fn open(root: &Path, budget: u64, retain: u64) -> Option<Self> {
        assert!(budget >= retain, "a disk cache retains within its budget");
        let staging = root.join(STAGING);
        std::fs::create_dir_all(&staging).ok()?;
        Some(Self {
            root: root.to_path_buf(),
            staging,
            bytes: accounting(root),
            recent: Mutex::new(HashMap::new()),
            tick: AtomicU64::new(0),
            budget,
            retain,
        })
    }

    pub fn load(&self, entry: &DiskEntry) -> Option<Vec<u8>> {
        let path = self.path(entry);
        let bytes = std::fs::read(&path).ok()?;
        self.touch(&path);
        Some(bytes)
    }

    pub fn save(&self, entry: &DiskEntry, payload: &[u8]) {
        let path = self.path(entry);
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err()
            || std::fs::create_dir_all(&self.staging).is_err()
        {
            return;
        }
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let staging = self
            .staging
            .join(format!("{:08x}-{nonce:016x}.part", std::process::id()));
        let written = (|| {
            let mut file = File::create(&staging)?;
            file.write_all(payload)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&staging);
            return;
        }
        if path.exists() {
            if std::fs::read(&path).is_ok_and(|held| held == payload) {
                let _ = std::fs::remove_file(&staging);
                return;
            }
            if std::fs::remove_file(&path).is_err() {
                let _ = std::fs::remove_file(&staging);
                return;
            }
        }
        if std::fs::rename(&staging, &path).is_err() {
            let _ = std::fs::remove_file(&staging);
            return;
        }
        self.touch(&path);
        self.grow(payload.len() as u64);
    }

    pub fn clear(root: &Path) -> std::io::Result<u64> {
        let mut freed = 0;
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                freed += directory_bytes(&path);
                remove(&path, true)?;
            } else {
                freed += metadata.len();
                remove(&path, false)?;
            }
        }
        reset(root);
        Ok(freed)
    }

    fn path(&self, entry: &DiskEntry) -> PathBuf {
        self.root.join(&entry.directory).join(&entry.file)
    }

    fn touch(&self, path: &Path) {
        let tick = self.tick.fetch_add(1, Ordering::Relaxed) + 1;
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        recent.insert(path.to_path_buf(), tick);
    }

    fn forget(&self, path: &Path) {
        self.recent
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(path);
    }

    fn grow(&self, length: u64) {
        let mut guard = self.bytes.lock().unwrap_or_else(|error| error.into_inner());
        let total = guard.get_or_insert_with(|| self.scan());
        *total = total.saturating_add(length);
        if *total > self.budget {
            self.evict(total);
        }
    }

    fn scan(&self) -> u64 {
        self.entries().iter().map(|(_, length, _)| *length).sum()
    }

    fn evict(&self, total: &mut u64) {
        let mut entries = self.entries();
        entries.sort_by_key(|entry| entry.0);
        for (_, length, path) in entries {
            if *total <= self.retain {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                self.forget(&path);
                *total = total.saturating_sub(length);
            }
        }
    }

    fn entries(&self) -> Vec<(Recency, u64, PathBuf)> {
        let recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut entries = Vec::new();
        let Ok(directories) = std::fs::read_dir(&self.root) else {
            return entries;
        };
        for directory in directories.flatten() {
            if directory.file_name() == OsStr::new(STAGING) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(directory.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let path = file.path();
                let recency = match recent.get(&path) {
                    Some(tick) => Recency {
                        touched: 1,
                        stamp: u128::from(*tick),
                    },
                    None => Recency {
                        touched: 0,
                        stamp: modified(&metadata),
                    },
                };
                entries.push((recency, metadata.len(), path));
            }
        }
        entries
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Recency {
    touched: u8,
    stamp: u128,
}

fn accounting(root: &Path) -> Accounting {
    let mut accounts = ACCOUNTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Arc::clone(accounts.entry(root.to_path_buf()).or_default())
}

fn modified(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            std::fs::symlink_metadata(&path).map_or(0, |metadata| {
                if metadata.file_type().is_symlink() {
                    0
                } else if metadata.is_dir() {
                    directory_bytes(&path)
                } else {
                    metadata.len()
                }
            })
        })
        .sum()
}

fn remove(path: &Path, directory: bool) -> std::io::Result<()> {
    let outcome = if directory {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match outcome {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn reset(root: &Path) {
    let accounts = ACCOUNTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(account) = accounts.get(root) {
        *account.lock().unwrap_or_else(|error| error.into_inner()) = Some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("grana-disk-cache-{}-{stamp}", std::process::id()));
            std::fs::create_dir_all(&path).expect("the temporary directory is created");
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replaces_a_corrupt_entry() {
        let temp = Temp::new();
        let cache = DiskCache::open(&temp.0, 1024, 512).expect("the cache opens");
        let entry = DiskEntry::new("dataset", "frame.surface");
        cache.save(&entry, b"stale");
        cache.save(&entry, b"current");
        assert_eq!(cache.load(&entry).as_deref(), Some(&b"current"[..]));
    }

    #[test]
    fn evicts_the_oldest_entries_down_to_the_retained_budget() {
        let temp = Temp::new();
        let cache = DiskCache::open(&temp.0, 128, 64).expect("the disk cache opens");
        let first = DiskEntry::new("a", "first.entry");
        let second = DiskEntry::new("a", "second.entry");
        let third = DiskEntry::new("a", "third.entry");
        cache.save(&first, &[1; 64]);
        cache.save(&second, &[2; 64]);
        cache.save(&third, &[3; 64]);
        assert!(cache.load(&first).is_none());
        assert!(cache.load(&second).is_none());
        assert_eq!(cache.load(&third).as_deref(), Some(&[3; 64][..]));
    }
}
