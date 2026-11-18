use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, SystemTime};

use crate::budget::Budget;

const SCRATCH_STALE: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_LIMIT: u32 = 8;
const SPILL_PREFIX: &str = "grana-detection-";
const SPILL_SUFFIX: &str = ".spill";

static NEXT: AtomicU64 = AtomicU64::new(0);
static SWEEP: Once = Once::new();
static RESERVED: Mutex<u64> = Mutex::new(0);

pub(super) enum Storage {
    Memory(Vec<f32>),
    Spill(Spill),
}

pub(super) struct Spill {
    file: Option<File>,
    mapping: OnceLock<Mapping>,
    path: PathBuf,
    bytes: u64,
}

struct Mapping {
    mapped: memmap2::Mmap,
}

impl Spill {
    pub(super) fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("an open spill holds its file")
    }

    fn floats(&self, offset: usize, length: usize) -> Result<&[f32], String> {
        if self.mapping.get().is_none() {
            let file = self.file.as_ref().expect("an open spill holds its file");
            let mapped = unsafe { memmap2::Mmap::map(file) }
                .map_err(|error| format!("failed to map the detection spill: {error}"))?;
            let _ = self.mapping.set(Mapping { mapped });
        }
        let mapping = self
            .mapping
            .get()
            .ok_or_else(|| "the detection spill holds no mapping".to_string())?;
        let start = offset * size_of::<f32>();
        let end = start + length * size_of::<f32>();
        Ok(bytemuck::cast_slice(&mapping.mapped[start..end]))
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        drop(self.mapping.take());
        self.file = None;
        let _ = std::fs::remove_file(&self.path);
        release(self.bytes);
    }
}

impl Storage {
    pub(super) fn floats(&self, offset: usize, length: usize) -> Result<&[f32], String> {
        match self {
            Self::Memory(values) => Ok(&values[offset..offset + length]),
            Self::Spill(spill) => spill.floats(offset, length),
        }
    }
}

pub(super) fn spill(bytes: u64, budget: Budget) -> Result<Storage, String> {
    let directory = std::env::temp_dir();
    SWEEP.call_once(|| sweep(&directory));
    let available = fs4::available_space(&directory).map_err(|error| {
        format!(
            "failed to measure the free space of {}: {error}",
            directory.display()
        )
    })?;
    {
        let mut reserved = RESERVED
            .lock()
            .expect("the detection spill budget is never poisoned");
        if available < *reserved + bytes + budget.spill_margin_bytes() {
            return Err(format!(
                "the detection volume spills {:.2} GiB to {} and the drive holds {:.2} GiB available with {:.2} GiB already spilled",
                gib(bytes),
                directory.display(),
                gib(available),
                gib(*reserved)
            ));
        }
        *reserved += bytes;
    }
    let (path, file) = match create(&directory, bytes) {
        Ok(created) => created,
        Err(detail) => {
            release(bytes);
            return Err(detail);
        }
    };
    Ok(Storage::Spill(Spill {
        file: Some(file),
        mapping: OnceLock::new(),
        path,
        bytes,
    }))
}

pub(super) fn release(bytes: u64) {
    let mut reserved = RESERVED
        .lock()
        .expect("the detection spill budget is never poisoned");
    *reserved = reserved.saturating_sub(bytes);
}

fn create(directory: &Path, bytes: u64) -> Result<(PathBuf, File), String> {
    let mut attempt = 0;
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "{SPILL_PREFIX}{}-{id}{SPILL_SUFFIX}",
            std::process::id()
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => match file.set_len(bytes) {
                Ok(()) => return Ok((path, file)),
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("failed to size the detection spill file: {error}"));
                }
            },
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < RETRY_LIMIT =>
            {
                attempt += 1;
            }
            Err(error) => {
                return Err(format!(
                    "failed to open the detection spill file {}: {error}",
                    path.display()
                ));
            }
        }
    }
}

fn sweep(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(SPILL_PREFIX) || !name.ends_with(SPILL_SUFFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > SCRATCH_STALE);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}
