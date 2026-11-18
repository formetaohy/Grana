use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::Error;
use crate::page::{Decode, Payload, StoreId};
use crate::scratch::Scratch;

pub const DEFAULT_BUDGET: u64 = 768 << 20;

const RECLAIM_FACTOR: u64 = 4;

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

pub struct SpillStore {
    budget: u64,
    inner: Mutex<Inner>,
}

struct Inner {
    scratch: Option<Scratch>,
    length: u64,
    resident: u64,
    spilled: u64,
    stamp: u64,
    next: u32,
    entries: HashMap<u32, Slot>,
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
    fn format(&self) -> Error {
        Error::Format { path: self.path() }
    }

    fn path(&self) -> PathBuf {
        self.scratch
            .as_ref()
            .map_or_else(PathBuf::new, |scratch| scratch.path().to_path_buf())
    }

    fn scratch(&mut self) -> Result<&mut Scratch, Error> {
        if self.scratch.is_none() {
            self.scratch = Some(Scratch::acquire(0)?);
        }
        Ok(self.scratch.as_mut().expect("an inner holds its scratch"))
    }

    fn bytes(&self, offset: u64, length: u64) -> Result<&[u8], Error> {
        let start = usize::try_from(offset).map_err(|_| self.format())?;
        let end = usize::try_from(offset + length).map_err(|_| self.format())?;
        self.scratch
            .as_ref()
            .and_then(Scratch::bytes)
            .and_then(|bytes| bytes.get(start..end))
            .ok_or_else(|| self.format())
    }
}

impl SpillStore {
    pub fn temporary(budget: u64) -> Result<Arc<Self>, Error> {
        assert!(budget > 0, "a spill store holds at least one byte");
        Ok(Arc::new(Self {
            budget,
            inner: Mutex::new(Inner::empty()),
        }))
    }

    pub fn memory() -> Arc<Self> {
        Arc::new(Self {
            budget: u64::MAX,
            inner: Mutex::new(Inner::empty()),
        })
    }

    pub fn insert<T: Decode>(&self, payload: T) -> Result<StoreId<T>, Error> {
        let bytes = payload.stored_bytes();
        let mut inner = self.inner();
        let id = inner.next;
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
        drop(inner);
        self.settle()?;
        Ok(StoreId::new(id))
    }

    pub fn load<T: Decode>(&self, id: StoreId<T>) -> Result<Arc<T>, Error> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let span = match inner.entries.get_mut(&id.raw()) {
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
                    .map_err(|_| inner.format());
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
        let payload =
            Arc::new(T::decode(inner.bytes(span.0, span.1)?).ok_or_else(|| inner.format())?);
        if let Some(slot) = inner.entries.get_mut(&id.raw())
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
        drop(inner);
        self.settle()?;
        Ok(payload)
    }

    pub fn bytes(&self, id: u32) -> Option<u64> {
        let mut inner = self.inner();
        inner.stamp += 1;
        let stamp = inner.stamp;
        let slot = inner.entries.get_mut(&id)?;
        slot.touch(stamp);
        Some(slot.bytes())
    }

    pub fn release(&self, id: u32) {
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

    fn settle(&self) -> Result<(), Error> {
        let mut inner = self.inner();
        self.reclaim(&mut inner)?;
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
            let payload = match inner.entries.get(&id) {
                Some(Slot::Resident { payload, .. }) => Arc::clone(payload),
                _ => continue,
            };
            let encoded = payload.encode();
            let length = encoded.len() as u64;
            let offset = inner.length;
            let start = usize::try_from(offset).map_err(|_| inner.format())?;
            let end = usize::try_from(offset + length).map_err(|_| inner.format())?;
            let scratch = inner.scratch()?;
            scratch.reserve(offset + length)?;
            let path = scratch.path().to_path_buf();
            scratch
                .bytes_mut()?
                .get_mut(start..end)
                .ok_or_else(|| Error::Spill {
                    path,
                    detail: "the spill file outgrew its mapping".to_string(),
                })?
                .copy_from_slice(&encoded);
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
        if inner.scratch.is_none() {
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
        for (id, offset, length) in plan {
            if offset != cursor {
                let start = usize::try_from(offset).map_err(|_| inner.format())?;
                let end = usize::try_from(offset + length).map_err(|_| inner.format())?;
                let target = usize::try_from(cursor).map_err(|_| inner.format())?;
                let scratch = inner.scratch()?;
                scratch.bytes_mut()?.copy_within(start..end, target);
            }
            moved.push((id, offset, cursor));
            cursor += length;
        }
        inner.scratch()?.shrink(cursor)?;
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

impl Inner {
    fn empty() -> Self {
        Self {
            scratch: None,
            length: 0,
            resident: 0,
            spilled: 0,
            stamp: 0,
            next: 0,
            entries: HashMap::new(),
        }
    }
}
