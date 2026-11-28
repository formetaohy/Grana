use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use grana_store::{Crc32, DiskCache, DiskEntry};
use grana_volume::{VoxelData, VoxelFormat};

use crate::slice::Key;

const MAGIC: [u8; 8] = *b"GRANAMIP";
const VERSION: u32 = 1;
const HEADER: usize = 80;
const TRAILER: usize = 4;
const SUFFIX: &str = "slice";
const DISK_BUDGET: u64 = 4 << 30;
const DISK_RETAIN: u64 = 3 << 30;
const OVERRIDE: &str = "GRANA_PYRAMID_CACHE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: VoxelFormat,
}

pub(crate) struct Store {
    disk: DiskCache,
}

impl Store {
    pub(crate) fn open(root: &Path) -> Option<Self> {
        Some(Self {
            disk: DiskCache::open(root, DISK_BUDGET, DISK_RETAIN)?,
        })
    }

    pub(crate) fn load(&self, key: Key, shape: Shape, stamp: u64) -> Option<VoxelData> {
        let bytes = self.disk.load(&entry(key))?;
        if bytes.len() < HEADER + TRAILER {
            return None;
        }
        let head = &bytes[..HEADER];
        let length = read_u64(head, 56);
        let matches = head[0..8] == MAGIC
            && read_u32(head, 8) == VERSION
            && read_u64(head, 12) == key.namespace
            && read_u32(head, 20) == key.level
            && read_u32(head, 24) == key.time
            && read_u32(head, 28) == key.channel
            && read_u32(head, 32) == key.z
            && read_u32(head, 36) == shape.width
            && read_u32(head, 40) == shape.height
            && head[44] == shape.format.code()
            && read_u64(head, 48) == stamp
            && length
                == u64::from(shape.width) * u64::from(shape.height) * shape.format.bytes() as u64;
        if !matches {
            return None;
        }
        let payload = &bytes[HEADER..bytes.len() - TRAILER];
        if payload.len() as u64 != length {
            return None;
        }
        let stored = read_u32(&bytes[bytes.len() - TRAILER..], 0);
        if stored != crc(head, payload) {
            return None;
        }
        Some(VoxelData::from_le_bytes(shape.format, payload))
    }

    pub(crate) fn save(&self, key: Key, shape: Shape, stamp: u64, data: &VoxelData) {
        let payload = data.to_le_bytes();
        let mut head = [0u8; HEADER];
        head[0..8].copy_from_slice(&MAGIC);
        write_u32(&mut head, 8, VERSION);
        write_u64(&mut head, 12, key.namespace);
        write_u32(&mut head, 20, key.level);
        write_u32(&mut head, 24, key.time);
        write_u32(&mut head, 28, key.channel);
        write_u32(&mut head, 32, key.z);
        write_u32(&mut head, 36, shape.width);
        write_u32(&mut head, 40, shape.height);
        head[44] = shape.format.code();
        write_u64(&mut head, 48, stamp);
        write_u64(&mut head, 56, payload.len() as u64);
        let stored = crc(&head, &payload);
        let mut bytes = Vec::with_capacity(HEADER + payload.len() + TRAILER);
        bytes.extend_from_slice(&head);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&stored.to_le_bytes());
        self.disk.save(&entry(key), &bytes);
    }
}

fn entry(key: Key) -> DiskEntry {
    DiskEntry::new(
        format!("{:016x}-{:02x}", key.namespace, key.level),
        format!(
            "{:08x}-{:04x}-{:08x}.{SUFFIX}",
            key.time, key.channel, key.z
        ),
    )
}

pub(crate) fn default_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(OVERRIDE) {
        let root = PathBuf::from(value);
        return (!root.as_os_str().is_empty()).then_some(root);
    }
    grana_dir::cache().map(|root| root.join("pyramid"))
}

pub(crate) fn mix(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

pub(crate) fn modified(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

pub(crate) fn clear(root: &Path) -> std::io::Result<u64> {
    DiskCache::clear(root)
}

fn crc(head: &[u8], payload: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(head);
    crc.update(payload);
    crc.finish()
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
