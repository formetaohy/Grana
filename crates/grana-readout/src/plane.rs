use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use grana_volume::{VoxelData, VoxelFormat};
use openreadout_core::{PixelType, Plane};

pub(crate) type Key = (u32, u32, u32, u32);

const SHARDS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) channel: u32,
    pub(crate) sample: usize,
    pub(crate) format: VoxelFormat,
}

enum Held {
    Loading {
        used: u64,
    },
    Ready {
        plane: Arc<Plane>,
        bytes: usize,
        used: u64,
    },
}

impl Held {
    fn used(&self) -> u64 {
        match self {
            Self::Loading { used } | Self::Ready { used, .. } => *used,
        }
    }
}

enum Claim {
    Ready(Arc<Plane>),
    Loading,
    Wait,
}

pub(crate) struct Cache {
    budget: usize,
    held: usize,
    tick: u64,
    planes: HashMap<Key, Held>,
}

impl Cache {
    pub(crate) fn new(budget: usize) -> Self {
        assert!(budget > 0, "a plane cache holds at least one byte");
        Self {
            budget,
            held: 0,
            tick: 0,
            planes: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, key: &Key) -> Option<Arc<Plane>> {
        let used = self.advance();
        let held = self.planes.get_mut(key)?;
        let Held::Ready {
            plane, used: stamp, ..
        } = held
        else {
            return None;
        };
        *stamp = used;
        Some(Arc::clone(plane))
    }

    pub(crate) fn put(&mut self, key: Key, plane: Arc<Plane>) {
        self.settle(&key, plane);
    }

    fn claim(&mut self, key: &Key) -> Claim {
        let used = self.advance();
        match self.planes.get_mut(key) {
            Some(Held::Ready { plane, used, .. }) => {
                let plane = Arc::clone(plane);
                *used = self.tick;
                Claim::Ready(plane)
            }
            Some(Held::Loading { .. }) => Claim::Wait,
            None => {
                self.planes.insert(*key, Held::Loading { used });
                Claim::Loading
            }
        }
    }

    fn settle(&mut self, key: &Key, plane: Arc<Plane>) {
        let used = self.advance();
        let bytes = plane.data.len();
        if let Some(Held::Ready { bytes, .. }) =
            self.planes.insert(*key, Held::Ready { plane, bytes, used })
        {
            self.held -= bytes;
        }
        self.held += bytes;
        while self.held > self.budget && self.planes.len() > 1 {
            let Some(oldest) = self
                .planes
                .iter()
                .filter(|(_, held)| matches!(held, Held::Ready { .. }))
                .min_by_key(|(_, held)| held.used())
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(Held::Ready { bytes, .. }) = self.planes.remove(&oldest) {
                self.held -= bytes;
            }
        }
    }

    fn abandon(&mut self, key: &Key) {
        self.planes.remove(key);
    }

    fn advance(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
}

struct Shard {
    cache: Mutex<Cache>,
    ready: Condvar,
}

impl Shard {
    fn lock(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn wait<'a>(&self, cache: MutexGuard<'a, Cache>) -> MutexGuard<'a, Cache> {
        self.ready
            .wait(cache)
            .unwrap_or_else(|error| error.into_inner())
    }

    fn wake(&self) {
        self.ready.notify_all();
    }
}

pub(crate) struct Shards {
    shards: Vec<Shard>,
}

impl Shards {
    pub(crate) fn new(budget: usize) -> Self {
        let each = (budget / SHARDS).max(1);
        Self {
            shards: (0..SHARDS)
                .map(|_| Shard {
                    cache: Mutex::new(Cache::new(each)),
                    ready: Condvar::new(),
                })
                .collect(),
        }
    }

    pub(crate) fn fetch<E>(
        &self,
        key: &Key,
        load: impl FnOnce() -> Result<Arc<Plane>, E>,
    ) -> Result<Arc<Plane>, E> {
        let shard = self.shard(key);
        {
            let mut cache = shard.lock();
            loop {
                match cache.claim(key) {
                    Claim::Ready(plane) => return Ok(plane),
                    Claim::Wait => cache = shard.wait(cache),
                    Claim::Loading => break,
                }
            }
        }
        let pending = Pending {
            shard,
            key: *key,
            armed: true,
        };
        let plane = load()?;
        Ok(pending.settle(plane))
    }

    fn shard(&self, key: &Key) -> &Shard {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        &self.shards[(hasher.finish() as usize) % self.shards.len()]
    }
}

struct Pending<'a> {
    shard: &'a Shard,
    key: Key,
    armed: bool,
}

impl Pending<'_> {
    fn settle(mut self, plane: Arc<Plane>) -> Arc<Plane> {
        let mut cache = self.shard.lock();
        cache.settle(&self.key, Arc::clone(&plane));
        drop(cache);
        self.armed = false;
        self.shard.wake();
        plane
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut cache = self.shard.lock();
        cache.abandon(&self.key);
        drop(cache);
        self.shard.wake();
    }
}

pub(crate) trait Sample: Send {
    fn sample(plane: &Plane, at: usize) -> Self;
}

impl Sample for u8 {
    fn sample(plane: &Plane, at: usize) -> Self {
        plane.data[at]
    }
}

impl Sample for u16 {
    fn sample(plane: &Plane, at: usize) -> Self {
        u16::from_le_bytes([plane.data[at], plane.data[at + 1]])
    }
}

impl Sample for f32 {
    fn sample(plane: &Plane, at: usize) -> Self {
        if plane.pixel_type == PixelType::Float {
            f32::from_le_bytes([
                plane.data[at],
                plane.data[at + 1],
                plane.data[at + 2],
                plane.data[at + 3],
            ])
        } else {
            plane.pixel_type.sample_f64(&plane.data[at..]) as f32
        }
    }
}

pub(crate) fn allocate(format: VoxelFormat, voxels: usize) -> VoxelData {
    match format {
        VoxelFormat::U8 => VoxelData::U8(vec![0; voxels]),
        VoxelFormat::U16 => VoxelData::U16(vec![0; voxels]),
        VoxelFormat::F32 => VoxelData::F32(vec![0.0; voxels]),
    }
}

pub(crate) fn rows<T: Sample>(
    plane: &Plane,
    sample: usize,
    x: &Range<u32>,
    y: &Range<u32>,
    target: &mut [T],
) {
    let width = (x.end - x.start) as usize;
    let height = (y.end - y.start) as usize;
    assert_eq!(
        target.len(),
        width * height,
        "a plane rectangle fills exactly the requested voxels"
    );
    let samples = plane.samples_per_pixel as usize;
    let bytes = plane.pixel_type.bytes_per_sample();
    let row_bytes = plane.row_bytes();
    for (row, line) in y.clone().enumerate() {
        let start = row_bytes * line as usize + (x.start as usize * samples + sample) * bytes;
        let slot = row * width;
        for (column, value) in target[slot..slot + width].iter_mut().enumerate() {
            *value = T::sample(plane, start + column * samples * bytes);
        }
    }
}
