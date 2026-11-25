use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(u32);

impl ObjectId {
    pub const FIRST: Self = Self(1);

    pub fn new(raw: u32) -> Self {
        assert!(raw != 0, "an object id is positive");
        Self(raw)
    }

    pub fn from_raw(raw: u32) -> Option<Self> {
        (raw != 0).then_some(Self(raw))
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn after(self, count: usize) -> Option<Self> {
        let count = u32::try_from(count).ok()?;
        self.0.checked_add(count).map(Self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectIds {
    ids: Arc<[ObjectId]>,
    order: Option<Arc<[u32]>>,
}

impl ObjectIds {
    pub fn new(ids: impl Into<Arc<[ObjectId]>>) -> Self {
        let ids: Arc<[ObjectId]> = ids.into();
        let order = ascending(&ids).expect("the object ids of a frame differ pairwise");
        Self { ids, order }
    }

    pub fn checked(ids: Vec<ObjectId>) -> Option<Self> {
        let ids: Arc<[ObjectId]> = ids.into();
        let order = ascending(&ids)?;
        Some(Self { ids, order })
    }

    pub fn empty() -> Self {
        Self {
            ids: Arc::new([]),
            order: None,
        }
    }

    pub fn block(first: ObjectId, count: usize) -> Self {
        let ids = (0..count)
            .map(|slot| {
                first
                    .after(slot)
                    .expect("an object id block lies inside the id space")
            })
            .collect::<Vec<_>>();
        Self::new(ids)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<ObjectId> {
        self.ids.get(index).copied()
    }

    pub fn as_slice(&self) -> &[ObjectId] {
        &self.ids
    }

    pub fn index_of(&self, id: ObjectId) -> Option<usize> {
        match &self.order {
            Some(order) => order
                .binary_search_by(|slot| self.ids[*slot as usize].cmp(&id))
                .ok()
                .map(|at| order[at] as usize),
            None => self.ids.binary_search(&id).ok(),
        }
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.index_of(id).is_some()
    }

    pub fn retained(&self, keep: &[bool]) -> Self {
        assert_eq!(
            keep.len(),
            self.ids.len(),
            "the kept objects cover every object id"
        );
        Self::new(
            self.ids
                .iter()
                .zip(keep)
                .filter(|(_, kept)| **kept)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
        )
    }

    pub fn without(&self, index: usize) -> Option<Self> {
        if index >= self.ids.len() {
            return None;
        }
        let mut ids = Vec::with_capacity(self.ids.len() - 1);
        ids.extend_from_slice(&self.ids[..index]);
        ids.extend_from_slice(&self.ids[index + 1..]);
        Some(Self::new(ids))
    }

    pub fn appended(&self, other: &Self) -> Option<Self> {
        let mut ids = Vec::with_capacity(self.ids.len() + other.ids.len());
        ids.extend_from_slice(&self.ids);
        ids.extend_from_slice(&other.ids);
        Self::checked(ids)
    }

    pub fn next(&self) -> ObjectId {
        self.ids
            .iter()
            .copied()
            .max()
            .and_then(|last| last.after(1))
            .unwrap_or(ObjectId::FIRST)
    }

    pub fn to_vec(&self) -> Vec<ObjectId> {
        self.ids.to_vec()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(u32);

impl TrackId {
    pub const FIRST: Self = Self(1);

    pub fn new(raw: u32) -> Self {
        assert!(raw != 0, "a track id is positive");
        Self(raw)
    }

    pub fn from_raw(raw: u32) -> Option<Self> {
        (raw != 0).then_some(Self(raw))
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn after(self, count: usize) -> Option<Self> {
        let count = u32::try_from(count).ok()?;
        self.0.checked_add(count).map(Self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackIds {
    ids: Arc<[TrackId]>,
    order: Option<Arc<[u32]>>,
}

impl TrackIds {
    pub fn new(ids: impl Into<Arc<[TrackId]>>) -> Self {
        let ids: Arc<[TrackId]> = ids.into();
        let order = ascending_tracks(&ids).expect("the track ids of a set differ pairwise");
        Self { ids, order }
    }

    pub fn checked(ids: Vec<TrackId>) -> Option<Self> {
        let ids: Arc<[TrackId]> = ids.into();
        let order = ascending_tracks(&ids)?;
        Some(Self { ids, order })
    }

    pub fn empty() -> Self {
        Self {
            ids: Arc::new([]),
            order: None,
        }
    }

    pub fn block(first: TrackId, count: usize) -> Self {
        let ids = (0..count)
            .map(|slot| {
                first
                    .after(slot)
                    .expect("a track id block lies inside the id space")
            })
            .collect::<Vec<_>>();
        Self::new(ids)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn as_slice(&self) -> &[TrackId] {
        &self.ids
    }

    pub fn index_of(&self, id: TrackId) -> Option<usize> {
        match &self.order {
            Some(order) => order
                .binary_search_by(|slot| self.ids[*slot as usize].cmp(&id))
                .ok()
                .map(|at| order[at] as usize),
            None => self.ids.binary_search(&id).ok(),
        }
    }

    pub fn contains(&self, id: TrackId) -> bool {
        self.index_of(id).is_some()
    }
}

#[derive(Clone, Debug)]
pub struct TrackIdSource {
    next: Arc<AtomicU32>,
}

impl TrackIdSource {
    pub fn new() -> Self {
        Self {
            next: Arc::new(AtomicU32::new(TrackId::FIRST.raw())),
        }
    }

    pub fn allocate(&self, count: usize) -> TrackIds {
        let count = u32::try_from(count).expect("a track id block fits the id space");
        let first = self.next.fetch_add(count, Ordering::Relaxed);
        assert!(
            first != 0 && first.checked_add(count).is_some(),
            "a track id block lies inside the id space"
        );
        TrackIds::block(TrackId::new(first), count as usize)
    }

    pub fn observe(&self, ids: &TrackIds) {
        if let Some(last) = ids.as_slice().iter().copied().max() {
            self.next.fetch_max(last.raw() + 1, Ordering::Relaxed);
        }
    }
}

impl Default for TrackIdSource {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct ObjectIdSource {
    next: Arc<AtomicU32>,
}

impl ObjectIdSource {
    pub fn new() -> Self {
        Self {
            next: Arc::new(AtomicU32::new(ObjectId::FIRST.raw())),
        }
    }

    pub fn allocate(&self, count: usize) -> ObjectIds {
        let count = u32::try_from(count).expect("an object id block fits the id space");
        let first = self.next.fetch_add(count, Ordering::Relaxed);
        assert!(
            first != 0 && first.checked_add(count).is_some(),
            "an object id block lies inside the id space"
        );
        ObjectIds::block(ObjectId::new(first), count as usize)
    }

    pub fn observe(&self, ids: &ObjectIds) {
        if let Some(last) = ids.as_slice().iter().copied().max() {
            self.next.fetch_max(last.raw() + 1, Ordering::Relaxed);
        }
    }
}

impl Default for ObjectIdSource {
    fn default() -> Self {
        Self::new()
    }
}

fn ascending_tracks(ids: &[TrackId]) -> Option<Option<Arc<[u32]>>> {
    if ids.windows(2).all(|pair| pair[0] < pair[1]) {
        return Some(None);
    }
    let mut order = (0..ids.len() as u32).collect::<Vec<_>>();
    order.sort_unstable_by_key(|slot| ids[*slot as usize]);
    if order
        .windows(2)
        .any(|pair| ids[pair[0] as usize] == ids[pair[1] as usize])
    {
        return None;
    }
    Some(Some(order.into()))
}

fn ascending(ids: &[ObjectId]) -> Option<Option<Arc<[u32]>>> {
    if ids.windows(2).all(|pair| pair[0] < pair[1]) {
        return Some(None);
    }
    let mut order = (0..ids.len() as u32).collect::<Vec<_>>();
    order.sort_unstable_by_key(|slot| ids[*slot as usize]);
    if order
        .windows(2)
        .any(|pair| ids[pair[0] as usize] == ids[pair[1] as usize])
    {
        return None;
    }
    Some(Some(order.into()))
}
