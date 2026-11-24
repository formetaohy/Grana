use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use grana_volume::{Extent, VoxelData, VoxelFormat};

const MAGIC: [u8; 8] = *b"GRANATIL";
const VERSION: u32 = 5;
const HEADER: usize = 80;
const TRAILER: usize = 4;
const SUFFIX: &str = "tile";
const STAGING: &str = "staging";
const BUDGET: u64 = 4 << 30;
const RETAIN: u64 = 3 << 30;
const MEMORY_BUDGET: u64 = 256 << 20;
const DECODE_BUDGET: u64 = 512 << 20;
const OVERRIDE: &str = "GRANA_PYRAMID_CACHE";
const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const CRC_SEED: u32 = 0xffff_ffff;
const CRC_POLYNOMIAL: u32 = 0xedb8_8320;

static NONCE: AtomicU64 = AtomicU64::new(0);

static DECODE: AtomicU64 = AtomicU64::new(DECODE_BUDGET);

pub fn decode_budget() -> u64 {
    DECODE.load(Ordering::Relaxed)
}

pub fn set_decode_budget(bytes: u64) {
    assert!(bytes > 0, "a decode cache holds at least one byte");
    DECODE.store(bytes, Ordering::Relaxed);
}

type Accounting = Arc<Mutex<Option<u64>>>;

static ACCOUNTING: OnceLock<Mutex<HashMap<PathBuf, Accounting>>> = OnceLock::new();

const CRC_TABLE: [u32; 256] = crc_table();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) identity: u64,
    pub(crate) level: u32,
    pub(crate) time: u32,
    pub(crate) channel: u32,
    pub(crate) coord: [u32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) extent: Extent,
    pub(crate) format: VoxelFormat,
}

pub(crate) type Cache = grana_store::LruCache<Key, VoxelData>;

pub(crate) fn bricks() -> Cache {
    Cache::new(MEMORY_BUDGET, size)
}

pub(crate) struct Store {
    root: PathBuf,
    staging: PathBuf,
    bytes: Accounting,
    recent: Mutex<HashMap<PathBuf, u64>>,
    tick: AtomicU64,
}

impl Store {
    pub(crate) fn open(root: &Path) -> Option<Self> {
        let staging = root.join(STAGING);
        std::fs::create_dir_all(&staging).ok()?;
        Some(Self {
            root: root.to_path_buf(),
            staging,
            bytes: accounting(root),
            recent: Mutex::new(HashMap::new()),
            tick: AtomicU64::new(0),
        })
    }

    pub(crate) fn load(&self, key: Key, shape: Shape, stamp: u64) -> Option<VoxelData> {
        let path = self.entry(key);
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() < HEADER + TRAILER {
            return None;
        }
        let head = &bytes[..HEADER];
        let extent = shape.extent;
        let length = read_u64(head, 72);
        let matches = head[0..8] == MAGIC
            && read_u32(head, 8) == VERSION
            && read_u64(head, 12) == key.identity
            && read_u32(head, 20) == key.level
            && read_u32(head, 24) == key.time
            && read_u32(head, 28) == key.channel
            && read_u32(head, 32) == key.coord[0]
            && read_u32(head, 36) == key.coord[1]
            && read_u32(head, 40) == key.coord[2]
            && read_u32(head, 44) == extent.x()
            && read_u32(head, 48) == extent.y()
            && read_u32(head, 52) == extent.z()
            && head[56] == shape.format.code()
            && read_u64(head, 64) == stamp
            && length == extent.voxel_count() * shape.format.bytes() as u64;
        if !matches {
            return None;
        }
        let payload = &bytes[HEADER..bytes.len() - TRAILER];
        if payload.len() as u64 != length {
            return None;
        }
        let stored = read_u32(&bytes[bytes.len() - TRAILER..], 0);
        if stored != crc(update(update(CRC_SEED, head), payload)) {
            return None;
        }
        self.touch(&path);
        Some(decode(shape.format, payload))
    }

    pub(crate) fn save(&self, key: Key, shape: Shape, stamp: u64, data: &VoxelData) {
        let path = self.entry(key);
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err()
            || std::fs::create_dir_all(&self.staging).is_err()
        {
            return;
        }
        let payload = encode(data);
        let mut head = [0u8; HEADER];
        head[0..8].copy_from_slice(&MAGIC);
        write_u32(&mut head, 8, VERSION);
        write_u64(&mut head, 12, key.identity);
        write_u32(&mut head, 20, key.level);
        write_u32(&mut head, 24, key.time);
        write_u32(&mut head, 28, key.channel);
        write_u32(&mut head, 32, key.coord[0]);
        write_u32(&mut head, 36, key.coord[1]);
        write_u32(&mut head, 40, key.coord[2]);
        write_u32(&mut head, 44, shape.extent.x());
        write_u32(&mut head, 48, shape.extent.y());
        write_u32(&mut head, 52, shape.extent.z());
        head[56] = shape.format.code();
        write_u64(&mut head, 64, stamp);
        write_u64(&mut head, 72, payload.len() as u64);
        let stored = crc(update(update(CRC_SEED, &head), &payload));
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let staging = self
            .staging
            .join(format!("{:08x}-{nonce:016x}.{SUFFIX}", std::process::id()));
        let written = (|| {
            let mut file = File::create(&staging)?;
            file.write_all(&head)?;
            file.write_all(&payload)?;
            file.write_all(&stored.to_le_bytes())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&staging);
            return;
        }
        if std::fs::rename(&staging, &path).is_err() {
            let _ = std::fs::remove_file(&staging);
            return;
        }
        self.grow((HEADER + payload.len() + TRAILER) as u64);
    }

    fn entry(&self, key: Key) -> PathBuf {
        self.root
            .join(format!("{:016x}-{:02x}", key.identity, key.level))
            .join(format!(
                "{:08x}-{:04x}-{:08x}{:08x}{:08x}.{SUFFIX}",
                key.time, key.channel, key.coord[0], key.coord[1], key.coord[2]
            ))
    }

    fn touch(&self, path: &Path) {
        let tick = self.tick.fetch_add(1, Ordering::Relaxed) + 1;
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        recent.insert(path.to_path_buf(), tick);
    }

    fn forget(&self, path: &Path) {
        self.recent
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(path);
    }

    fn grow(&self, length: u64) {
        let mut guard = self.bytes.lock().unwrap_or_else(|error| error.into_inner());
        let total = guard.get_or_insert_with(|| self.scan());
        *total = total.saturating_add(length);
        if *total > BUDGET {
            self.evict(total);
        }
    }

    fn scan(&self) -> u64 {
        self.entries().iter().map(|(_, length, _)| *length).sum()
    }

    fn evict(&self, total: &mut u64) {
        let mut entries = self.entries();
        entries.sort_by_key(|entry| entry.0);
        for (_, length, path) in entries {
            if *total <= RETAIN {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                self.forget(&path);
                *total = total.saturating_sub(length);
            }
        }
    }

    fn entries(&self) -> Vec<(Recency, u64, PathBuf)> {
        let recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut entries = Vec::new();
        let Ok(directories) = std::fs::read_dir(&self.root) else {
            return entries;
        };
        for directory in directories.flatten() {
            if directory.file_name() == OsStr::new(STAGING) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(directory.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let path = file.path();
                let recency = match recent.get(&path) {
                    Some(tick) => Recency {
                        touched: 1,
                        stamp: u128::from(*tick),
                    },
                    None => Recency {
                        touched: 0,
                        stamp: modified(&metadata),
                    },
                };
                entries.push((recency, metadata.len(), path));
            }
        }
        entries
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Recency {
    touched: u8,
    stamp: u128,
}

pub fn default_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(OVERRIDE) {
        let root = PathBuf::from(value);
        return (!root.as_os_str().is_empty()).then_some(root);
    }
    grana_dir::cache().map(|root| root.join("pyramid"))
}

fn accounting(root: &Path) -> Accounting {
    let mut accounts = ACCOUNTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Arc::clone(accounts.entry(root.to_path_buf()).or_default())
}

pub(crate) fn identity(
    source: &crate::Source,
    geometry: &[u8],
    dataset: Option<u64>,
) -> Option<u64> {
    let mut hash = mix(FNV_BASE, geometry);
    match source {
        crate::Source::Memory(_) => None,
        crate::Source::File {
            path,
            series,
            group,
        } => {
            let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            hash = mix(hash, resolved.to_string_lossy().as_bytes());
            hash = mix(hash, &(*series as u64).to_le_bytes());
            if let Some(group) = group {
                hash = mix(hash, group.as_bytes());
            }
            match dataset {
                Some(digest) => Some(mix(hash, &digest.to_le_bytes())),
                None => {
                    let metadata = std::fs::metadata(path).ok()?;
                    if metadata.is_dir() {
                        return None;
                    }
                    Some(stamp(hash, &metadata))
                }
            }
        }
        crate::Source::Remote { url, series, group } => {
            let mut hash = mix(hash, url.as_bytes());
            hash = mix(hash, &(*series as u64).to_le_bytes());
            if let Some(group) = group {
                hash = mix(hash, group.as_bytes());
            }
            Some(mix(hash, &dataset?.to_le_bytes()))
        }
    }
}

fn stamp(hash: u64, metadata: &std::fs::Metadata) -> u64 {
    let hash = mix(hash, &metadata.len().to_le_bytes());
    mix(hash, &modified(metadata).to_le_bytes())
}

fn modified(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

pub fn clear(root: &Path) -> std::io::Result<u64> {
    let mut freed = 0;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            freed += directory_bytes(&path);
            remove(&path, true)?;
        } else {
            freed += metadata.len();
            remove(&path, false)?;
        }
    }
    reset(root);
    Ok(freed)
}

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            std::fs::symlink_metadata(&path).map_or(0, |metadata| {
                if metadata.file_type().is_symlink() {
                    0
                } else if metadata.is_dir() {
                    directory_bytes(&path)
                } else {
                    metadata.len()
                }
            })
        })
        .sum()
}

fn remove(path: &Path, directory: bool) -> std::io::Result<()> {
    let outcome = if directory {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match outcome {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn reset(root: &Path) {
    let accounts = ACCOUNTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(account) = accounts.get(root) {
        *account.lock().unwrap_or_else(|error| error.into_inner()) = Some(0);
    }
}

fn mix(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn size(data: &VoxelData) -> u64 {
    data.bytes()
}

fn encode(data: &VoxelData) -> Vec<u8> {
    data.to_le_bytes()
}

fn decode(format: VoxelFormat, bytes: &[u8]) -> VoxelData {
    VoxelData::from_le_bytes(format, bytes)
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

fn update(state: u32, bytes: &[u8]) -> u32 {
    bytes.iter().fold(state, |state, byte| {
        CRC_TABLE[((state ^ u32::from(*byte)) & 0xff) as usize] ^ (state >> 8)
    })
}

fn crc(state: u32) -> u32 {
    state ^ CRC_SEED
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                CRC_POLYNOMIAL ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}
