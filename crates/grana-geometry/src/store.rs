use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::Error;
use crate::packed::PackedMesh;

pub const DEFAULT_BUDGET: u64 = 768 << 20;

const PREFIX: &str = "grana-geometry";
const SUFFIX: &str = "mesh";
const RECLAIM_FACTOR: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshStats {
    resident: u64,
    spilled: u64,
    meshes: usize,
}

impl MeshStats {
    pub fn resident(self) -> u64 {
        self.resident
    }

    pub fn spilled(self) -> u64 {
        self.spilled
    }

    pub fn meshes(self) -> usize {
        self.meshes
    }
}

pub struct MeshStore {
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
    entries: HashMap<MeshId, Slot>,
}

enum Slot {
    Resident {
        mesh: Arc<PackedMesh>,
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

impl MeshStore {
    pub fn temporary(budget: u64) -> Result<Arc<Self>, Error> {
        assert!(budget > 0, "a mesh store holds at least one byte");
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
        let mut entries = HashMap::new();
        entries.reserve(0);
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
                entries,
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

    pub fn insert(&self, mesh: PackedMesh) -> Result<MeshId, Error> {
        let bytes = mesh.byte_length() as u64;
        let mut inner = self.inner();
        let id = MeshId(inner.next);
        inner.next += 1;
        inner.stamp += 1;
        let stamp = inner.stamp;
        inner.resident += bytes;
        inner.entries.insert(
            id,
            Slot::Resident {
                mesh: Arc::new(mesh),
                bytes,
                stamp,
            },
        );
        self.settle(&mut inner)?;
        Ok(id)
    }

    pub fn load(&self, id: MeshId) -> Result<Arc<PackedMesh>, Error> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let span = match inner.entries.get_mut(&id) {
            None => return Err(Error::Unknown),
            Some(Slot::Resident {
                mesh, stamp: used, ..
            }) => {
                *used = stamp;
                return Ok(Arc::clone(mesh));
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
        let mesh = Arc::new(PackedMesh::from_bytes(&bytes).ok_or_else(|| inner.format())?);
        if let Some(slot) = inner.entries.get_mut(&id)
            && matches!(slot, Slot::Spilled { .. })
        {
            *slot = Slot::Resident {
                mesh: Arc::clone(&mesh),
                bytes: span.1,
                stamp,
            };
            inner.resident += span.1;
            inner.spilled = inner.spilled.saturating_sub(span.1);
        }
        self.settle(&mut inner)?;
        Ok(mesh)
    }

    pub fn bytes(&self, id: MeshId) -> Option<u64> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let slot = inner.entries.get_mut(&id)?;
        slot.touch(stamp);
        Some(slot.bytes())
    }

    pub fn release(&self, id: MeshId) {
        let mut inner = self.inner();
        let Some(slot) = inner.entries.remove(&id) else {
            return;
        };
        match slot {
            Slot::Resident { bytes, .. } => inner.resident = inner.resident.saturating_sub(bytes),
            Slot::Spilled { length, .. } => inner.spilled = inner.spilled.saturating_sub(length),
        }
    }

    pub fn stats(&self) -> MeshStats {
        let inner = self.inner();
        MeshStats {
            resident: inner.resident,
            spilled: inner.spilled,
            meshes: inner.entries.len(),
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
                "a mesh store without a spill file never evicts"
            );
            let mesh = match inner.entries.get(&id) {
                Some(Slot::Resident { mesh, .. }) => Arc::clone(mesh),
                _ => continue,
            };
            let encoded = mesh.to_bytes();
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

impl Drop for MeshStore {
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

#[derive(Clone)]
pub enum Geometry {
    Resident(Arc<PackedMesh>),
    Paged(Arc<Paged>),
}

impl Geometry {
    pub fn resident(mesh: PackedMesh) -> Self {
        Self::Resident(Arc::new(mesh))
    }

    pub fn paged(store: &Arc<MeshStore>, mesh: PackedMesh) -> Result<Self, Error> {
        let id = store.insert(mesh)?;
        Ok(Self::Paged(Arc::new(Paged {
            store: Arc::clone(store),
            id,
        })))
    }

    pub fn load(&self) -> Result<Arc<PackedMesh>, Error> {
        match self {
            Self::Resident(mesh) => Ok(Arc::clone(mesh)),
            Self::Paged(paged) => paged.store.load(paged.id()),
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            Self::Resident(mesh) => mesh.bytes(),
            Self::Paged(paged) => paged.store.bytes(paged.id()).unwrap_or_default(),
        }
    }

    pub fn without(&self, object: usize) -> Result<Option<Self>, Error> {
        let mesh = self.load()?;
        if object >= mesh.object_count() {
            return Ok(None);
        }
        let pruned = mesh.without(object);
        Ok(Some(match self {
            Self::Resident(_) => Self::resident(pruned),
            Self::Paged(paged) => Self::paged(paged.store(), pruned)?,
        }))
    }
}

impl PartialEq for Geometry {
    fn eq(&self, other: &Self) -> bool {
        match (self.load(), other.load()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Geometry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resident(mesh) => formatter
                .debug_struct("Resident")
                .field("mesh", mesh)
                .finish(),
            Self::Paged(paged) => formatter
                .debug_struct("Paged")
                .field("id", &paged.id())
                .field("bytes", &self.bytes())
                .finish(),
        }
    }
}

pub struct Paged {
    store: Arc<MeshStore>,
    id: MeshId,
}

impl Paged {
    fn id(&self) -> MeshId {
        self.id
    }

    fn store(&self) -> &Arc<MeshStore> {
        &self.store
    }
}

impl Drop for Paged {
    fn drop(&mut self) {
        self.store.release(self.id);
    }
}

fn temporary_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{PREFIX}-{}-{count}.{SUFFIX}", std::process::id()))
}
