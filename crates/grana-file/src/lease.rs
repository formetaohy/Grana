use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DIRECTORY: &str = "grana-locks";
const EXTENSION: &str = ".lock";
const ATTEMPTS: usize = 10;
const BACKOFF: Duration = Duration::from_millis(25);
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

pub struct Lease {
    _file: File,
}

impl Lease {
    pub fn acquire(target: &Path) -> io::Result<Self> {
        for attempt in 0..ATTEMPTS {
            if let Some(lease) = Self::try_acquire(target)? {
                return Ok(lease);
            }
            std::thread::sleep(BACKOFF * (attempt as u32 + 1).min(4));
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("another process is writing {}", target.display()),
        ))
    }

    pub fn try_acquire(target: &Path) -> io::Result<Option<Self>> {
        let path = path(target)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

fn path(target: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(target)?;
    let mut hash = OFFSET;
    for unit in units(&absolute) {
        hash ^= u64::from(unit);
        hash = hash.wrapping_mul(PRIME);
    }
    Ok(std::env::temp_dir()
        .join(DIRECTORY)
        .join(format!("{hash:016x}{EXTENSION}")))
}

#[cfg(windows)]
fn units(path: &Path) -> impl Iterator<Item = u16> + '_ {
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    for unit in wide.iter_mut() {
        if (0x41..=0x5a).contains(unit) {
            *unit += 0x20;
        }
    }
    wide.into_iter()
}

#[cfg(unix)]
fn units(path: &Path) -> impl Iterator<Item = u16> + '_ {
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| u16::from(*byte))
}

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
