use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex};

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

pub(crate) struct Cache {
    budget: usize,
    held: usize,
    tick: u64,
    planes: HashMap<Key, Held>,
}

struct Held {
    plane: Arc<Plane>,
    bytes: usize,
    used: u64,
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
        self.tick += 1;
        let tick = self.tick;
        self.planes.get_mut(key).map(|held| {
            held.used = tick;
            Arc::clone(&held.plane)
        })
    }

    pub(crate) fn put(&mut self, key: Key, plane: Arc<Plane>, bytes: usize) {
        self.tick += 1;
        if let Some(held) = self.planes.insert(
            key,
            Held {
                plane,
                bytes,
                used: self.tick,
            },
        ) {
            self.held -= held.bytes;
        }
        self.held += bytes;
        while self.held > self.budget && self.planes.len() > 1 {
            let Some(oldest) = self
                .planes
                .iter()
                .min_by_key(|(_, held)| held.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(held) = self.planes.remove(&oldest) {
                self.held -= held.bytes;
            }
        }
    }
}

pub(crate) struct Shards {
    shards: Vec<Mutex<Cache>>,
}

impl Shards {
    pub(crate) fn new(budget: usize) -> Self {
        let each = (budget / SHARDS).max(1);
        Self {
            shards: (0..SHARDS).map(|_| Mutex::new(Cache::new(each))).collect(),
        }
    }

    pub(crate) fn get(&self, key: &Key) -> Option<Arc<Plane>> {
        self.shard(key)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
    }

    pub(crate) fn put(&self, key: Key, plane: Arc<Plane>, bytes: usize) {
        self.shard(&key)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .put(key, plane, bytes);
    }

    fn shard(&self, key: &Key) -> &Mutex<Cache> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        &self.shards[(hasher.finish() as usize) % self.shards.len()]
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
