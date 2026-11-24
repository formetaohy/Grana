use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::Error;
use crate::page::{Decode, Payload, StoreId};
use crate::scratch::Scratch;

pub const SPILL_BUDGET: u64 = 768 << 20;

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
    copies: u64,
    stamp: u64,
    next: u32,
    entries: HashMap<u32, Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Encoded {
    offset: u64,
    length: u64,
}

enum Slot {
    Resident {
        payload: Arc<dyn Payload>,
        bytes: u64,
        encoded: Option<Encoded>,
        stamp: u64,
    },
    Spilled {
        encoded: Encoded,
        stamp: u64,
    },
}

impl Slot {
    fn bytes(&self) -> u64 {
        match self {
            Self::Resident { bytes, .. } => *bytes,
            Self::Spilled { encoded, .. } => encoded.length,
        }
    }

    fn encoded(&self) -> Option<Encoded> {
        match self {
            Self::Resident { encoded, .. } => *encoded,
            Self::Spilled { encoded, .. } => Some(*encoded),
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

    fn shift(&mut self, was: u64, now: u64) {
        match self {
            Self::Resident {
                encoded: Some(encoded),
                ..
            } => {
                if encoded.offset == was {
                    encoded.offset = now;
                }
            }
            Self::Spilled { encoded, .. } => {
                if encoded.offset == was {
                    encoded.offset = now;
                }
            }
            Self::Resident { encoded: None, .. } => {}
        }
    }
}

impl Inner {
    fn empty() -> Self {
        Self {
            scratch: None,
            length: 0,
            resident: 0,
            spilled: 0,
            copies: 0,
            stamp: 0,
            next: 0,
            entries: HashMap::new(),
        }
    }

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

    fn bytes(&self, encoded: Encoded) -> Result<&[u8], Error> {
        let start = usize::try_from(encoded.offset).map_err(|_| self.format())?;
        let end = usize::try_from(encoded.offset + encoded.length).map_err(|_| self.format())?;
        self.scratch
            .as_ref()
            .and_then(Scratch::bytes)
            .and_then(|bytes| bytes.get(start..end))
            .ok_or_else(|| self.format())
    }

    fn write(&mut self, encoded: Encoded, payload: &[u8]) -> Result<(), Error> {
        let start = usize::try_from(encoded.offset).map_err(|_| self.format())?;
        let end = usize::try_from(encoded.offset + encoded.length).map_err(|_| self.format())?;
        let scratch = self.scratch()?;
        scratch.reserve(encoded.offset + encoded.length)?;
        let path = scratch.path().to_path_buf();
        scratch
            .bytes_mut()?
            .get_mut(start..end)
            .ok_or_else(|| Error::Spill {
                path,
                detail: "the spill file outgrew its mapping".to_string(),
            })?
            .copy_from_slice(payload);
        Ok(())
    }

    fn reseat(&mut self, id: u32, encoded: Encoded) {
        self.stamp += 1;
        let stamp = self.stamp;
        self.entries.insert(id, Slot::Spilled { encoded, stamp });
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
                encoded: None,
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
        let encoded = match inner.entries.get_mut(&id.raw()) {
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
                encoded,
                stamp: used,
            }) => {
                *used = stamp;
                *encoded
            }
        };
        let payload = Arc::new(T::decode(inner.bytes(encoded)?).ok_or_else(|| inner.format())?);
        if let Some(slot) = inner.entries.get_mut(&id.raw())
            && matches!(slot, Slot::Spilled { .. })
        {
            *slot = Slot::Resident {
                payload: Arc::clone(&payload) as Arc<dyn Payload>,
                bytes: encoded.length,
                encoded: Some(encoded),
                stamp,
            };
            inner.resident += encoded.length;
            inner.spilled = inner.spilled.saturating_sub(encoded.length);
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
            Slot::Resident {
                bytes,
                encoded: Some(encoded),
                ..
            } => {
                inner.resident = inner.resident.saturating_sub(bytes);
                inner.copies = inner.copies.saturating_sub(encoded.length);
            }
            Slot::Resident {
                bytes,
                encoded: None,
                ..
            } => inner.resident = inner.resident.saturating_sub(bytes),
            Slot::Spilled { encoded, .. } => {
                inner.spilled = inner.spilled.saturating_sub(encoded.length);
                inner.copies = inner.copies.saturating_sub(encoded.length);
            }
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
            let (payload, bytes, held) = match inner.entries.get(&id) {
                Some(Slot::Resident {
                    payload,
                    bytes,
                    encoded,
                    ..
                }) => (Arc::clone(payload), *bytes, *encoded),
                _ => continue,
            };
            let (encoded, written) = match held {
                Some(encoded) => (encoded, false),
                None => {
                    let payload = payload.encode();
                    let encoded = Encoded {
                        offset: inner.length,
                        length: payload.len() as u64,
                    };
                    inner.write(encoded, &payload)?;
                    inner.length += encoded.length;
                    (encoded, true)
                }
            };
            inner.reseat(id, encoded);
            inner.resident = inner.resident.saturating_sub(bytes);
            inner.spilled += encoded.length;
            if written {
                inner.copies += encoded.length;
            }
        }
        Ok(())
    }

    fn reclaim(&self, inner: &mut Inner) -> Result<(), Error> {
        if inner.scratch.is_none() {
            return Ok(());
        }
        if inner.length <= RECLAIM_FACTOR * self.budget || inner.copies * 2 >= inner.length {
            return Ok(());
        }
        let mut plan = inner
            .entries
            .iter()
            .filter_map(|(id, slot)| slot.encoded().map(|encoded| (*id, encoded)))
            .collect::<Vec<_>>();
        plan.sort_unstable_by_key(|(_, encoded)| encoded.offset);
        let mut cursor = 0u64;
        let mut moved = Vec::with_capacity(plan.len());
        for (id, encoded) in plan {
            if encoded.offset != cursor {
                let start = usize::try_from(encoded.offset).map_err(|_| inner.format())?;
                let end =
                    usize::try_from(encoded.offset + encoded.length).map_err(|_| inner.format())?;
                let target = usize::try_from(cursor).map_err(|_| inner.format())?;
                let scratch = inner.scratch()?;
                scratch.bytes_mut()?.copy_within(start..end, target);
            }
            moved.push((id, encoded.offset, cursor));
            cursor += encoded.length;
        }
        inner.scratch()?.shrink(cursor)?;
        inner.length = cursor;
        for (id, was, now) in moved {
            if let Some(slot) = inner.entries.get_mut(&id) {
                slot.shift(was, now);
            }
        }
        Ok(())
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}
