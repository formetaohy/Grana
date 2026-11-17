use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const ATTEMPTS: usize = 8;
const BACKOFF: Duration = Duration::from_millis(25);

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
}

impl Atomic {
    pub fn file(target: &Path) -> io::Result<Self> {
        let atomic = Self::new(target, false)?;
        let _ = fs::remove_file(&atomic.staging);
        Ok(atomic)
    }

    pub fn directory(target: &Path) -> io::Result<Self> {
        let atomic = Self::new(target, true)?;
        atomic.recover()?;
        if atomic.staging.exists() {
            fs::remove_dir_all(&atomic.staging)?;
        }
        fs::create_dir_all(&atomic.staging)?;
        Ok(atomic)
    }

    pub fn staging(&self) -> &Path {
        &self.staging
    }

    pub fn commit(mut self) -> io::Result<()> {
        if !self.directory {
            attempt(|| fs::rename(&self.staging, &self.target))?;
            self.committed = true;
            return Ok(());
        }
        if self.backup.exists() {
            attempt(|| fs::remove_dir_all(&self.backup))?;
        }
        let replacing = self.target.exists();
        if replacing {
            attempt(|| fs::rename(&self.target, &self.backup))?;
        }
        match attempt(|| fs::rename(&self.staging, &self.target)) {
            Ok(()) => {
                self.committed = true;
                let _ = attempt(|| fs::remove_dir_all(&self.backup));
                Ok(())
            }
            Err(error) => {
                if replacing {
                    let _ = attempt(|| fs::rename(&self.backup, &self.target));
                }
                Err(error)
            }
        }
    }

    fn recover(&self) -> io::Result<()> {
        if self.target.exists() || !self.backup.exists() {
            return Ok(());
        }
        attempt(|| fs::rename(&self.backup, &self.target))
    }

    fn new(target: &Path, directory: bool) -> io::Result<Self> {
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("`{}` names no entry to write", target.display()),
                )
            })?;
        let staging = target.with_file_name(format!(
            ".{name}.{}-{}.part",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let backup = target.with_file_name(format!(".{name}.old"));
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
        if self.committed {
            return;
        }
        let _ = if self.directory {
            attempt(|| fs::remove_dir_all(&self.staging))
        } else {
            attempt(|| fs::remove_file(&self.staging))
        };
    }
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
