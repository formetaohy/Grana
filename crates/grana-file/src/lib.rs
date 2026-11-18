use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rayon::prelude::*;

mod lease;

pub use lease::Lease;

const ATTEMPTS: usize = 8;
const BACKOFF: Duration = Duration::from_millis(25);
const STAGING_SUFFIX: &str = ".part";
const BACKUP_SUFFIX: &str = ".old";
const HIDDEN: char = '.';
const SEQUENCE_SEPARATOR: &str = "-";
const HEADROOM: u64 = 64 << 20;

#[cfg(windows)]
const TRANSIENT: [i32; 3] = [5, 32, 33];
#[cfg(not(windows))]
const TRANSIENT: [i32; 0] = [];

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Atomic {
    target: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    directory: bool,
    committed: bool,
    lease: Option<Lease>,
}

pub struct Replaced {
    target: PathBuf,
    backup: PathBuf,
    replaced: bool,
    _lease: Lease,
}

impl Atomic {
    pub fn file(target: &Path) -> io::Result<Self> {
        Self::new(target, false)
    }

    pub fn directory(target: &Path) -> io::Result<Self> {
        let atomic = Self::new(target, true)?;
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
        let lease = self
            .lease
            .take()
            .expect("an atomic write holds its target lease");
        sync(&self.staging)?;
        if !self.directory {
            attempt(|| fs::rename(&self.staging, &self.target))?;
            self.committed = true;
            sync_parent(&self.target)?;
            return Ok(Replaced {
                target: self.target.clone(),
                backup: self.backup.clone(),
                replaced: false,
                _lease: lease,
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
                sync_parent(&self.target)?;
                Ok(Replaced {
                    target: self.target.clone(),
                    backup: self.backup.clone(),
                    replaced,
                    _lease: lease,
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
        let lease = Lease::acquire(target)?;
        let staging = target.with_file_name(format!(
            "{HIDDEN}{name}.{}-{}{STAGING_SUFFIX}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let backup = backup_path(target);
        clear_staging(parent(target), &name)?;
        restore_backup(target)?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            backup,
            directory,
            committed: false,
            lease: Some(lease),
        })
    }
}

impl Drop for Atomic {
    fn drop(&mut self) {
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

pub fn recover(target: &Path) -> io::Result<()> {
    if !artifacts(target)? {
        return Ok(());
    }
    match Lease::acquire(target) {
        Ok(_lease) => recover_held(target),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(error) => Err(error),
    }
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

pub fn available(path: &Path) -> io::Result<u64> {
    let mut anchor = path;
    loop {
        match fs4::available_space(anchor) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(parent) = anchor
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                else {
                    return Err(error);
                };
                anchor = parent;
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn require(target: &Path, expected: u64) -> io::Result<()> {
    let held = match bytes(target) {
        Ok(held) => held,
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    let free = available(target)?;
    let needed = held.saturating_add(expected).saturating_add(HEADROOM);
    if free >= needed {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "writing {} keeps the current version of {:.2} GiB beside the {:.2} GiB it is expected to store, and needs {:.2} GiB free, but the volume holds {:.2} GiB available",
        target.display(),
        gib(held),
        gib(expected),
        gib(needed),
        gib(free),
    )))
}

pub fn sync(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return sync_file(path);
    }
    let mut files = Vec::new();
    let mut directories = vec![path.to_path_buf()];
    collect(path, &mut files, &mut directories)?;
    files.par_iter().try_for_each(|file| sync_file(file))?;
    sync_directories(&directories)
}

fn collect(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            directories.push(path.clone());
            collect(&path, files, directories)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn sync_file(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

fn sync_parent(target: &Path) -> io::Result<()> {
    sync_directories(&[parent(target).to_path_buf()])
}

#[cfg(unix)]
fn sync_directories(directories: &[PathBuf]) -> io::Result<()> {
    for directory in directories {
        fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_directories(_directories: &[PathBuf]) -> io::Result<()> {
    Ok(())
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}

fn recover_held(target: &Path) -> io::Result<()> {
    let name = segment(target)?;
    clear_staging(parent(target), &name)?;
    restore_backup(target)
}

fn restore_backup(target: &Path) -> io::Result<()> {
    let backup = backup_path(target);
    if !path_exists(&backup) {
        return Ok(());
    }
    if path_exists(target) {
        return attempt(|| remove(&backup));
    }
    attempt(|| fs::rename(&backup, target))
}

fn artifacts(target: &Path) -> io::Result<bool> {
    let name = segment(target)?;
    if path_exists(&backup_path(target)) {
        return Ok(true);
    }
    let entries = match fs::read_dir(parent(target)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let held = entry?.file_name();
        let Some(held) = held.to_str() else {
            continue;
        };
        if staging_name(held) == Some(name.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn backup_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    target.with_file_name(format!("{HIDDEN}{name}{BACKUP_SUFFIX}"))
}

fn parent(target: &Path) -> &Path {
    target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
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
        attempt(|| remove(&entry.path()))?;
    }
    Ok(())
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
