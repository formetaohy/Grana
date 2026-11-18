use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::time::Duration;

use memmap2::MmapMut;

use crate::error::Error;

const STEP: u64 = 64 << 20;
const MARGIN: u64 = 64 << 20;
const RETRIES: u32 = 8;
const STALE: Duration = Duration::from_secs(60);
const PREFIX: &str = "grana-spill";
const SUFFIX: &str = ".spill";

static NEXT: AtomicU64 = AtomicU64::new(0);
static RESERVED: Mutex<u64> = Mutex::new(0);
static SWEEP: Once = Once::new();

pub struct Scratch {
    path: PathBuf,
    file: File,
    mapping: Option<MmapMut>,
    size: u64,
    claim: u64,
}

impl Scratch {
    pub fn acquire(bytes: u64) -> Result<Self, Error> {
        SWEEP.call_once(sweep);
        let size = extent(bytes);
        let (path, file) = create()?;
        let claim = claim(&path, size)?;
        file.set_len(size).map_err(|error| Error::Create {
            path: path.clone(),
            detail: error.to_string(),
        })?;
        let mapping = map(&path, &file)?;
        Ok(Self {
            path,
            file,
            mapping: Some(mapping),
            size,
            claim,
        })
    }

    pub fn reserve(&mut self, bytes: u64) -> Result<(), Error> {
        let size = extent(bytes);
        if size <= self.size {
            return Ok(());
        }
        claim(&self.path, size - self.size)?;
        self.file.set_len(size).map_err(|error| Error::Create {
            path: self.path.clone(),
            detail: error.to_string(),
        })?;
        let mapping = map(&self.path, &self.file)?;
        self.mapping = Some(mapping);
        self.size = size;
        self.claim = size;
        Ok(())
    }

    pub fn shrink(&mut self, bytes: u64) -> Result<(), Error> {
        let size = extent(bytes);
        if size >= self.size {
            return Ok(());
        }
        self.mapping = None;
        self.file.set_len(size).map_err(|error| Error::Create {
            path: self.path.clone(),
            detail: error.to_string(),
        })?;
        release(self.size - size);
        self.size = size;
        self.claim = size;
        self.mapping = Some(map(&self.path, &self.file)?);
        Ok(())
    }

    pub fn capacity(&self) -> u64 {
        self.size
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.mapping.as_ref().map(|mapping| &mapping[..])
    }

    pub fn bytes_mut(&mut self) -> Result<&mut [u8], Error> {
        if self.mapping.is_none() {
            self.mapping = Some(map(&self.path, &self.file)?);
        }
        Ok(self
            .mapping
            .as_mut()
            .expect("a reserved scratch holds its mapping"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(self.mapping.take());
        release(self.claim);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn extent(bytes: u64) -> u64 {
    bytes.div_ceil(STEP).max(1) * STEP
}

fn claim(path: &Path, bytes: u64) -> Result<u64, Error> {
    let directory = directory();
    let available = fs4::available_space(&directory).map_err(|error| Error::Create {
        path: path.to_path_buf(),
        detail: format!(
            "failed to measure the free space of {}: {error}",
            directory.display()
        ),
    })?;
    let mut reserved = RESERVED.lock().unwrap_or_else(|error| error.into_inner());
    if available < *reserved + bytes + MARGIN {
        return Err(Error::Space {
            path: path.to_path_buf(),
            detail: format!(
                "spilling {:.2} GiB keeps {:.2} GiB aside, but {} holds {:.2} GiB available",
                gib(bytes),
                gib(*reserved),
                directory.display(),
                gib(available)
            ),
        });
    }
    *reserved += bytes;
    Ok(bytes)
}

fn release(bytes: u64) {
    let mut reserved = RESERVED.lock().unwrap_or_else(|error| error.into_inner());
    *reserved = reserved.saturating_sub(bytes);
}

fn map(path: &Path, file: &File) -> Result<MmapMut, Error> {
    unsafe { MmapMut::map_mut(file) }.map_err(|error| Error::Create {
        path: path.to_path_buf(),
        detail: format!("failed to map the spill file: {error}"),
    })
}

fn create() -> Result<(PathBuf, File), Error> {
    let directory = directory();
    let mut attempt = 0;
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("{PREFIX}-{}-{id}{SUFFIX}", std::process::id()));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => match file.lock() {
                Ok(()) => return Ok((path, file)),
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(Error::Create {
                        path,
                        detail: error.to_string(),
                    });
                }
            },
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < RETRIES =>
            {
                attempt += 1;
            }
            Err(error) => {
                return Err(Error::Create {
                    path,
                    detail: error.to_string(),
                });
            }
        }
    }
}

fn sweep() {
    let Ok(entries) = std::fs::read_dir(directory()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE);
        if !stale {
            continue;
        }
        let Ok(file) = OpenOptions::new().read(true).write(true).open(entry.path()) else {
            continue;
        };
        if file.try_lock().is_err() {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

fn directory() -> PathBuf {
    std::env::temp_dir()
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}
