use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const ATTEMPTS: usize = 8;
const BACKOFF: Duration = Duration::from_millis(25);
const STAGING_SUFFIX: &str = ".part";
const BACKUP_SUFFIX: &str = ".old";
const HIDDEN: char = '.';
const SEQUENCE_SEPARATOR: &str = "-";

#[cfg(windows)]
const TRANSIENT: [i32; 3] = [5, 32, 33];
#[cfg(not(windows))]
const TRANSIENT: [i32; 0] = [];

static NEXT: AtomicUsize = AtomicUsize::new(0);
static LIVE: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub struct Atomic {
    target: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    directory: bool,
    committed: bool,
}

pub struct Replaced {
    target: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    replaced: bool,
}

impl Atomic {
    pub fn file(target: &Path) -> io::Result<Self> {
        let atomic = Self::new(target, false)?;
        let _ = remove_if_present(&atomic.staging);
        claim(&atomic.staging);
        Ok(atomic)
    }

    pub fn directory(target: &Path) -> io::Result<Self> {
        let atomic = Self::new(target, true)?;
        recover(target)?;
        claim(&atomic.staging);
        fs::create_dir_all(&atomic.staging)?;
        Ok(atomic)
    }

    pub fn staging(&self) -> &Path {
        &self.staging
    }

    pub fn commit(self) -> io::Result<()> {
        self.commit_held().map(Replaced::publish)
    }

    pub fn commit_held(mut self) -> io::Result<Replaced> {
        if !self.directory {
            attempt(|| fs::rename(&self.staging, &self.target))?;
            self.committed = true;
            return Ok(Replaced {
                target: self.target.clone(),
                staging: self.staging.clone(),
                backup: self.backup.clone(),
                replaced: false,
            });
        }
        if path_exists(&self.backup) {
            attempt(|| remove(&self.backup))?;
        }
        let replaced = path_exists(&self.target);
        if replaced {
            attempt(|| fs::rename(&self.target, &self.backup))?;
        }
        match attempt(|| fs::rename(&self.staging, &self.target)) {
            Ok(()) => {
                self.committed = true;
                Ok(Replaced {
                    target: self.target.clone(),
                    staging: self.staging.clone(),
                    backup: self.backup.clone(),
                    replaced,
                })
            }
            Err(error) => {
                if replaced {
                    let _ = attempt(|| fs::rename(&self.backup, &self.target));
                }
                Err(error)
            }
        }
    }

    fn new(target: &Path, directory: bool) -> io::Result<Self> {
        let name = segment(target)?;
        let staging = target.with_file_name(format!(
            "{HIDDEN}{name}.{}-{}{STAGING_SUFFIX}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let backup = target.with_file_name(format!("{HIDDEN}{name}{BACKUP_SUFFIX}"));
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            backup,
            directory,
            committed: false,
        })
    }
}

impl Drop for Atomic {
    fn drop(&mut self) {
        release(&self.staging);
        if self.committed {
            return;
        }
        let _ = remove_if_present(&self.staging);
    }
}

impl Replaced {
    pub fn publish(self) {
        let _ = remove_if_present(&self.backup);
    }

    pub fn rollback(self) -> io::Result<()> {
        remove_if_present(&self.target)?;
        if self.replaced {
            attempt(|| fs::rename(&self.backup, &self.target))?;
        }
        Ok(())
    }
}

impl Drop for Replaced {
    fn drop(&mut self) {
        release(&self.staging);
    }
}

pub fn recover(target: &Path) -> io::Result<()> {
    let Some(name) = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return Ok(());
    };
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    clear_staging(parent, &name)?;
    let backup = target.with_file_name(format!("{HIDDEN}{name}{BACKUP_SUFFIX}"));
    if !path_exists(&backup) {
        return Ok(());
    }
    if path_exists(target) {
        return attempt(|| remove(&backup));
    }
    attempt(|| fs::rename(&backup, target))
}

pub fn recover_inside(directory: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut targets = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(target) = artifact_target(name) {
            targets.push(target.to_string());
        }
    }
    targets.sort_unstable();
    targets.dedup();
    for target in targets {
        recover(&directory.join(target))?;
    }
    Ok(())
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let atomic = Atomic::file(path)?;
    fs::write(atomic.staging(), bytes)?;
    atomic.commit()
}

pub fn bytes(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        total += bytes(&entry?.path())?;
    }
    Ok(total)
}

fn segment(target: &Path) -> io::Result<String> {
    target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{}` names no entry to write", target.display()),
            )
        })
}

fn artifact_target(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix(HIDDEN)?;
    if let Some(backup) = name.strip_suffix(BACKUP_SUFFIX)
        && !backup.is_empty()
    {
        return Some(backup);
    }
    staging_name(entry)
}

fn staging_name(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix(HIDDEN)?.strip_suffix(STAGING_SUFFIX)?;
    let (name, sequence) = name.rsplit_once(HIDDEN)?;
    let (process, slot) = sequence.split_once(SEQUENCE_SEPARATOR)?;
    if name.is_empty() || process.parse::<u32>().is_err() || slot.parse::<u32>().is_err() {
        return None;
    }
    Some(name)
}

fn clear_staging(parent: &Path, name: &str) -> io::Result<()> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let held = entry.file_name();
        let Some(held) = held.to_str() else {
            continue;
        };
        if staging_name(held) != Some(name) {
            continue;
        }
        let path = entry.path();
        if live(&path) {
            continue;
        }
        attempt(|| remove(&path))?;
    }
    Ok(())
}

fn claim(staging: &Path) {
    LIVE.lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(staging.to_path_buf());
}

fn release(staging: &Path) {
    LIVE.lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(staging);
}

fn live(staging: &Path) -> bool {
    LIVE.lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(staging)
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn remove(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match remove(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        outcome => outcome,
    }
}

fn attempt(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut taken = 0;
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) if transient(&error) && taken + 1 < ATTEMPTS => {
                taken += 1;
                std::thread::sleep(BACKOFF * taken as u32);
            }
            Err(error) => return Err(error),
        }
    }
}

fn transient(error: &io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|code| TRANSIENT.contains(&code))
}
