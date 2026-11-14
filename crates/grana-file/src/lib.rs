use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Atomic {
    target: PathBuf,
    staging: PathBuf,
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
        if self.directory && self.target.exists() {
            let backup = self.staging.with_extension("old");
            let _ = fs::remove_dir_all(&backup);
            fs::rename(&self.target, &backup)?;
            return match fs::rename(&self.staging, &self.target) {
                Ok(()) => {
                    self.committed = true;
                    let _ = fs::remove_dir_all(&backup);
                    Ok(())
                }
                Err(error) => {
                    let _ = fs::rename(&backup, &self.target);
                    Err(error)
                }
            };
        }
        fs::rename(&self.staging, &self.target)?;
        self.committed = true;
        Ok(())
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
        Ok(Self {
            target: target.to_path_buf(),
            staging,
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
            fs::remove_dir_all(&self.staging)
        } else {
            fs::remove_file(&self.staging)
        };
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let atomic = Atomic::file(path)?;
    fs::write(atomic.staging(), bytes)?;
    atomic.commit()
}
