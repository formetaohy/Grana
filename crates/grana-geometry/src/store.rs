use std::any::Any;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use crate::error::Error;
use crate::packed::PackedMesh;

pub const DEFAULT_BUDGET: u64 = 768 << 20;

const PREFIX: &str = "grana-geometry";
const SUFFIX: &str = "mesh";
const RECLAIM_FACTOR: u64 = 4;
const STALE_AGE: Duration = Duration::from_secs(60);

pub trait Payload: Send + Sync + 'static {
    fn encode(&self) -> Vec<u8>;

    fn stored_bytes(&self) -> u64;

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

pub trait Decode: Payload + Sized {
    fn decode(bytes: &[u8]) -> Option<Self>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeometryId(u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoreStats {
    resident: u64,
    spilled: u64,
    entries: usize,
}

impl StoreStats {
    pub fn resident(self) -> u64 {
        self.resident
    }

    pub fn spilled(self) -> u64 {
        self.spilled
    }

    pub fn entries(self) -> usize {
        self.entries
    }
}

pub struct GeometryStore {
    budget: u64,
    inner: Mutex<Inner>,
}

struct Inner {
    file: Option<File>,
    path: Option<PathBuf>,
    length: u64,
    resident: u64,
    spilled: u64,
    stamp: u64,
    next: u32,
    entries: HashMap<GeometryId, Slot>,
}

enum Slot {
    Resident {
        payload: Arc<dyn Payload>,
        bytes: u64,
        stamp: u64,
    },
    Spilled {
        offset: u64,
        length: u64,
        stamp: u64,
    },
}

impl Slot {
    fn bytes(&self) -> u64 {
        match self {
            Self::Resident { bytes, .. } => *bytes,
            Self::Spilled { length, .. } => *length,
        }
    }

    fn stamp(&self) -> u64 {
        match self {
            Self::Resident { stamp, .. } | Self::Spilled { stamp, .. } => *stamp,
        }
    }

    fn touch(&mut self, stamp: u64) {
        match self {
            Self::Resident { stamp: used, .. } | Self::Spilled { stamp: used, .. } => {
                *used = stamp;
            }
        }
    }
}

impl Inner {
    fn spill_error(&self, error: std::io::Error) -> Error {
        Error::Spill {
            path: self.path.clone().unwrap_or_default(),
            detail: error.to_string(),
        }
    }

    fn format(&self) -> Error {
        Error::Format {
            path: self.path.clone().unwrap_or_default(),
        }
    }

    fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("the spill file exists whenever it is read")
    }
}

impl GeometryStore {
    pub fn temporary(budget: u64) -> Result<Arc<Self>, Error> {
        assert!(budget > 0, "a geometry store holds at least one byte");
        sweep();
        let path = temporary_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| Error::Create {
                path: path.clone(),
                detail: error.to_string(),
            })?;
        file.lock().map_err(|error| Error::Create {
            path: path.clone(),
            detail: error.to_string(),
        })?;
        Ok(Arc::new(Self {
            budget,
            inner: Mutex::new(Inner {
                file: Some(file),
                path: Some(path),
                length: 0,
                resident: 0,
                spilled: 0,
                stamp: 0,
                next: 0,
                entries: HashMap::new(),
            }),
        }))
    }

    pub fn memory() -> Arc<Self> {
        Arc::new(Self {
            budget: u64::MAX,
            inner: Mutex::new(Inner {
                file: None,
                path: None,
                length: 0,
                resident: 0,
                spilled: 0,
                stamp: 0,
                next: 0,
                entries: HashMap::new(),
            }),
        })
    }

    pub fn insert<T: Decode>(&self, payload: T) -> Result<GeometryId, Error> {
        let bytes = payload.stored_bytes();
        let mut inner = self.inner();
        let id = GeometryId(inner.next);
        inner.next += 1;
        inner.stamp += 1;
        let stamp = inner.stamp;
        inner.resident += bytes;
        inner.entries.insert(
            id,
            Slot::Resident {
                payload: Arc::new(payload),
                bytes,
                stamp,
            },
        );
        self.settle(&mut inner)?;
        Ok(id)
    }

    pub fn load<T: Decode>(&self, id: GeometryId) -> Result<Arc<T>, Error> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let path = inner.path.clone().unwrap_or_default();
        let span = match inner.entries.get_mut(&id) {
            None => return Err(Error::Unknown),
            Some(Slot::Resident {
                payload,
                stamp: used,
                ..
            }) => {
                *used = stamp;
                let payload = Arc::clone(payload);
                return payload
                    .into_any()
                    .downcast::<T>()
                    .map_err(|_| Error::Format { path });
            }
            Some(Slot::Spilled {
                offset,
                length,
                stamp: used,
            }) => {
                *used = stamp;
                (*offset, *length)
            }
        };
        let length = usize::try_from(span.1).map_err(|_| inner.format())?;
        let mut bytes = vec![0u8; length];
        inner
            .file()
            .seek(SeekFrom::Start(span.0))
            .and_then(|_| inner.file().read_exact(&mut bytes))
            .map_err(|error| inner.spill_error(error))?;
        let payload = Arc::new(T::decode(&bytes).ok_or_else(|| inner.format())?);
        if let Some(slot) = inner.entries.get_mut(&id)
            && matches!(slot, Slot::Spilled { .. })
        {
            *slot = Slot::Resident {
                payload: Arc::clone(&payload) as Arc<dyn Payload>,
                bytes: span.1,
                stamp,
            };
            inner.resident += span.1;
            inner.spilled = inner.spilled.saturating_sub(span.1);
        }
        self.settle(&mut inner)?;
        Ok(payload)
    }

    pub fn bytes(&self, id: GeometryId) -> Option<u64> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let slot = inner.entries.get_mut(&id)?;
        slot.touch(stamp);
        Some(slot.bytes())
    }

    pub fn release(&self, id: GeometryId) {
        let mut inner = self.inner();
        let Some(slot) = inner.entries.remove(&id) else {
            return;
        };
        match slot {
            Slot::Resident { bytes, .. } => inner.resident = inner.resident.saturating_sub(bytes),
            Slot::Spilled { length, .. } => inner.spilled = inner.spilled.saturating_sub(length),
        }
    }

    pub fn stats(&self) -> StoreStats {
        let inner = self.inner();
        StoreStats {
            resident: inner.resident,
            spilled: inner.spilled,
            entries: inner.entries.len(),
        }
    }

    fn settle(&self, inner: &mut Inner) -> Result<(), Error> {
        self.reclaim(inner)?;
        while inner.resident > self.budget {
            let victim = inner
                .entries
                .iter()
                .filter(|(_, slot)| matches!(slot, Slot::Resident { .. }))
                .min_by_key(|(_, slot)| slot.stamp())
                .map(|(id, _)| *id);
            let Some(id) = victim else {
                return Ok(());
            };
            assert!(
                inner.file.is_some(),
                "a geometry store without a spill file never evicts"
            );
            let payload = match inner.entries.get(&id) {
                Some(Slot::Resident { payload, .. }) => Arc::clone(payload),
                _ => continue,
            };
            let encoded = payload.encode();
            let length = encoded.len() as u64;
            let offset = inner.length;
            inner
                .file()
                .seek(SeekFrom::Start(offset))
                .and_then(|_| inner.file().write_all(&encoded))
                .map_err(|error| inner.spill_error(error))?;
            inner.length += length;
            if let Some(slot) = inner.entries.get_mut(&id)
                && matches!(slot, Slot::Resident { .. })
            {
                let stored = slot.bytes();
                inner.stamp += 1;
                let stamp = inner.stamp;
                inner.entries.insert(
                    id,
                    Slot::Spilled {
                        offset,
                        length,
                        stamp,
                    },
                );
                inner.resident = inner.resident.saturating_sub(stored);
                inner.spilled += length;
            }
        }
        Ok(())
    }

    fn reclaim(&self, inner: &mut Inner) -> Result<(), Error> {
        if inner.file.is_none() {
            return Ok(());
        }
        let occupied = inner.spilled;
        if inner.length <= RECLAIM_FACTOR * self.budget || occupied * 2 >= inner.length {
            return Ok(());
        }
        let mut plan = inner
            .entries
            .iter()
            .filter_map(|(id, slot)| match slot {
                Slot::Resident { .. } => None,
                Slot::Spilled { offset, length, .. } => Some((*id, *offset, *length)),
            })
            .collect::<Vec<_>>();
        plan.sort_unstable_by_key(|(_, offset, _)| *offset);
        let mut cursor = 0u64;
        let mut moved = Vec::with_capacity(plan.len());
        let mut buffer = Vec::new();
        for (id, offset, length) in plan {
            if offset != cursor {
                buffer.resize(length as usize, 0);
                inner
                    .file()
                    .seek(SeekFrom::Start(offset))
                    .and_then(|_| inner.file().read_exact(&mut buffer))
                    .and_then(|_| inner.file().seek(SeekFrom::Start(cursor)))
                    .and_then(|_| inner.file().write_all(&buffer))
                    .map_err(|error| inner.spill_error(error))?;
            }
            moved.push((id, offset, cursor));
            cursor += length;
        }
        inner
            .file()
            .set_len(cursor)
            .map_err(|error| inner.spill_error(error))?;
        inner.length = cursor;
        for (id, was, now) in moved {
            if let Some(Slot::Spilled { offset, .. }) = inner.entries.get_mut(&id)
                && *offset == was
            {
                *offset = now;
            }
        }
        Ok(())
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl Drop for GeometryStore {
    fn drop(&mut self) {
        let inner = self
            .inner
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(path) = &inner.path {
            remove(path);
        }
    }
}

fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub struct Geometry<T> {
    stored: Stored<T>,
}

enum Stored<T> {
    Resident(Arc<T>),
    Paged(Arc<Paged<T>>),
}

impl<T: Decode> Geometry<T> {
    pub fn resident(payload: T) -> Self {
        Self {
            stored: Stored::Resident(Arc::new(payload)),
        }
    }

    pub fn paged(store: &Arc<GeometryStore>, payload: T) -> Result<Self, Error> {
        Ok(Self {
            stored: Stored::Paged(Arc::new(Paged::new(store, payload)?)),
        })
    }

    pub fn load(&self) -> Result<Arc<T>, Error> {
        match &self.stored {
            Stored::Resident(payload) => Ok(Arc::clone(payload)),
            Stored::Paged(paged) => paged.load(),
        }
    }

    pub fn bytes(&self) -> u64 {
        match &self.stored {
            Stored::Resident(payload) => payload.stored_bytes(),
            Stored::Paged(paged) => paged.bytes(),
        }
    }

    pub fn replace(&self, payload: T) -> Result<Self, Error> {
        Ok(match &self.stored {
            Stored::Resident(_) => Self::resident(payload),
            Stored::Paged(paged) => Self::paged(&paged.store, payload)?,
        })
    }
}

impl Geometry<PackedMesh> {
    pub fn without(&self, object: usize) -> Result<Option<Self>, Error> {
        let mesh = self.load()?;
        if object >= mesh.object_count() {
            return Ok(None);
        }
        self.replace(mesh.without(object)).map(Some)
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Self, Error> {
        let mesh = self.load()?;
        assert_eq!(
            keep.len(),
            mesh.object_count(),
            "the kept objects cover every packed object"
        );
        self.replace(mesh.retain(keep))
    }

    pub fn merge(&self, other: &Self) -> Result<Self, Error> {
        let mesh = self.load()?;
        let absorbed = other.load()?;
        self.replace(mesh.merge(&absorbed))
    }
}

impl<T> Clone for Geometry<T> {
    fn clone(&self) -> Self {
        Self {
            stored: match &self.stored {
                Stored::Resident(payload) => Stored::Resident(Arc::clone(payload)),
                Stored::Paged(paged) => Stored::Paged(Arc::clone(paged)),
            },
        }
    }
}

impl<T: Decode + std::fmt::Debug> std::fmt::Debug for Geometry<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.stored {
            Stored::Resident(payload) => formatter.debug_tuple("Resident").field(payload).finish(),
            Stored::Paged(paged) => formatter
                .debug_struct("Paged")
                .field("id", &paged.id)
                .field("bytes", &paged.bytes())
                .finish(),
        }
    }
}

impl<T: Decode + PartialEq> PartialEq for Geometry<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self.load(), other.load()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}

struct Paged<T> {
    store: Arc<GeometryStore>,
    id: GeometryId,
    payload: PhantomData<fn() -> T>,
}

impl<T: Decode> Paged<T> {
    fn new(store: &Arc<GeometryStore>, payload: T) -> Result<Self, Error> {
        Ok(Self {
            id: store.insert(payload)?,
            store: Arc::clone(store),
            payload: PhantomData,
        })
    }

    fn load(&self) -> Result<Arc<T>, Error> {
        self.store.load(self.id)
    }

    fn bytes(&self) -> u64 {
        self.store.bytes(self.id).unwrap_or_default()
    }
}

impl<T> Drop for Paged<T> {
    fn drop(&mut self) {
        self.store.release(self.id);
    }
}

fn temporary_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{PREFIX}-{}-{count}.{SUFFIX}", std::process::id()))
}

fn sweep() {
    static SWEPT: OnceLock<()> = OnceLock::new();
    SWEPT.get_or_init(|| {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let suffix = format!(".{SUFFIX}");
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with(PREFIX) || !name.ends_with(&suffix) {
                continue;
            }
            let fresh = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age < STALE_AGE);
            if fresh {
                continue;
            }
            let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            remove(&path);
        }
    });
}
