use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Extent, VoxelData, VoxelFormat};

const MAGIC: [u8; 8] = *b"GRANATIL";
const VERSION: u32 = 2;
const HEADER: usize = 72;
const TRAILER: usize = 4;
const SUFFIX: &str = "tile";
const STAGING: &str = "staging";
const BUDGET: u64 = 4 << 30;
const RETAIN: u64 = 3 << 30;
const MEMORY_BUDGET: u64 = 256 << 20;
const OVERRIDE: &str = "GRANA_PYRAMID_CACHE";
const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const CRC_SEED: u32 = 0xffff_ffff;
const CRC_POLYNOMIAL: u32 = 0xedb8_8320;

static NONCE: AtomicU64 = AtomicU64::new(0);

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

struct Slot {
    data: Arc<VoxelData>,
    stamp: u64,
}

pub(crate) struct Memory {
    slots: HashMap<Key, Slot>,
    bytes: u64,
    stamp: u64,
}

impl Memory {
    pub(crate) fn new() -> Self {
        Self {
            slots: HashMap::new(),
            bytes: 0,
            stamp: 0,
        }
    }

    pub(crate) fn fetch(&mut self, key: Key) -> Option<Arc<VoxelData>> {
        self.stamp += 1;
        let stamp = self.stamp;
        let slot = self.slots.get_mut(&key)?;
        slot.stamp = stamp;
        Some(Arc::clone(&slot.data))
    }

    pub(crate) fn insert(&mut self, key: Key, data: Arc<VoxelData>) {
        self.stamp += 1;
        let bytes = size(&data);
        let previous = self.slots.insert(
            key,
            Slot {
                data,
                stamp: self.stamp,
            },
        );
        if let Some(previous) = previous {
            self.bytes -= size(&previous.data);
        }
        self.bytes += bytes;
        while self.bytes > MEMORY_BUDGET && self.slots.len() > 1 {
            let oldest = self
                .slots
                .iter()
                .min_by_key(|(_, slot)| slot.stamp)
                .map(|(key, _)| *key)
                .expect("the memory cache holds a tile to evict");
            if let Some(removed) = self.slots.remove(&oldest) {
                self.bytes -= size(&removed.data);
            }
        }
    }
}

pub(crate) struct Store {
    root: PathBuf,
    staging: PathBuf,
    bytes: Accounting,
}

impl Store {
    pub(crate) fn open(root: &Path) -> Option<Self> {
        let staging = root.join(STAGING);
        std::fs::create_dir_all(&staging).ok()?;
        Some(Self {
            root: root.to_path_buf(),
            staging,
            bytes: accounting(root),
        })
    }

    pub(crate) fn load(&self, key: Key, shape: Shape) -> Option<VoxelData> {
        let path = self.entry(key);
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() < HEADER + TRAILER {
            return None;
        }
        let head = &bytes[..HEADER];
        let extent = shape.extent;
        let length = read_u64(head, 64);
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
            && head[56] == tag(shape.format)
            && length == extent.voxel_count() * sample_bytes(shape.format);
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

    pub(crate) fn save(&self, key: Key, shape: Shape, data: &VoxelData) {
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
        head[56] = tag(shape.format);
        write_u64(&mut head, 64, payload.len() as u64);
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
        let Ok(file) = OpenOptions::new().write(true).open(path) else {
            return;
        };
        let _ = file.set_modified(SystemTime::now());
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
                *total = total.saturating_sub(length);
            }
        }
    }

    fn entries(&self) -> Vec<(SystemTime, u64, PathBuf)> {
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
                let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
                entries.push((modified, metadata.len(), file.path()));
            }
        }
        entries
    }
}

pub fn default_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(OVERRIDE) {
        let root = PathBuf::from(value);
        return (!root.as_os_str().is_empty()).then_some(root);
    }
    base().map(|base| base.join("grana").join("pyramid"))
}

fn accounting(root: &Path) -> Accounting {
    let mut accounts = ACCOUNTING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Arc::clone(accounts.entry(root.to_path_buf()).or_default())
}

pub(crate) fn identity(source: &crate::Source, geometry: &[u8]) -> u64 {
    let mut hash = mix(FNV_BASE, geometry);
    if let crate::Source::File { path, series } = source {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        hash = mix(hash, resolved.to_string_lossy().as_bytes());
        hash = mix(hash, &(*series as u64).to_le_bytes());
        hash = contents(hash, path);
    }
    hash
}

fn contents(hash: u64, path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Err(_) => mix(hash, &u64::MAX.to_le_bytes()),
        Ok(metadata) if metadata.is_dir() => listing(hash, path),
        Ok(metadata) => stamp(hash, &metadata),
    }
}

fn listing(hash: u64, root: &Path) -> u64 {
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort_unstable();
    let mut hash = mix(hash, &(entries.len() as u64).to_le_bytes());
    for (name, length, modified) in entries {
        hash = mix(hash, name.as_bytes());
        hash = mix(hash, &[0]);
        hash = mix(hash, &length.to_le_bytes());
        hash = mix(hash, &modified.to_le_bytes());
    }
    hash
}

fn collect(root: &Path, folder: &Path, entries: &mut Vec<(String, u64, u128)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect(root, &path, entries);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((name, metadata.len(), modified(&metadata)));
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

#[cfg(target_os = "windows")]
fn base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn base() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library").join("Caches"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn base() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
}

fn size(data: &VoxelData) -> u64 {
    data.voxel_count() as u64 * sample_bytes(data.format())
}

fn sample_bytes(format: VoxelFormat) -> u64 {
    match format {
        VoxelFormat::U8 => 1,
        VoxelFormat::U16 => 2,
        VoxelFormat::F32 => 4,
    }
}

fn tag(format: VoxelFormat) -> u8 {
    match format {
        VoxelFormat::U8 => 0,
        VoxelFormat::U16 => 1,
        VoxelFormat::F32 => 2,
    }
}

fn encode(data: &VoxelData) -> Vec<u8> {
    match data {
        VoxelData::U8(values) => values.clone(),
        VoxelData::U16(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        VoxelData::F32(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn decode(format: VoxelFormat, bytes: &[u8]) -> VoxelData {
    match format {
        VoxelFormat::U8 => VoxelData::U8(bytes.to_vec()),
        VoxelFormat::U16 => VoxelData::U16(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_le_bytes(*chunk))
                .collect(),
        ),
        VoxelFormat::F32 => VoxelData::F32(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect(),
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(extent: Extent) -> Shape {
        Shape {
            extent,
            format: VoxelFormat::U16,
        }
    }

    fn key(coord: [u32; 3]) -> Key {
        Key {
            identity: 7,
            level: 2,
            time: 1,
            channel: 0,
            coord,
        }
    }

    fn directory() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grana-cache-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).expect("the cache directory is created");
        path
    }

    #[test]
    fn round_trips_a_tile_and_rejects_a_corrupted_entry() {
        let directory = directory();
        let store = Store::open(&directory).expect("the store opens");
        let extent = Extent::new(4, 3, 2);
        let data = VoxelData::U16((0..24u16).collect());
        store.save(key([5, 0, 1]), shape(extent), &data);
        assert_eq!(
            store.load(key([5, 0, 1]), shape(extent)),
            Some(data.clone())
        );
        assert_eq!(store.load(key([6, 0, 1]), shape(extent)), None);

        let entry = store.entry(key([5, 0, 1]));
        let mut bytes = std::fs::read(&entry).expect("the entry reads");
        assert_eq!(bytes.len(), HEADER + 48 + TRAILER);
        bytes[HEADER] ^= 0xff;
        std::fs::write(&entry, &bytes).expect("the entry is rewritten");
        assert_eq!(store.load(key([5, 0, 1]), shape(extent)), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn evicts_entries_beyond_the_budget() {
        let directory = directory();
        let store = Store::open(&directory).expect("the store opens");
        let extent = Extent::new(8, 8, 8);
        let data = VoxelData::U16(vec![1; extent.voxel_count() as usize]);
        *store.bytes.lock().expect("the accounting is shared") = Some(BUDGET - 512);
        store.save(key([0, 0, 0]), shape(extent), &data);
        assert!(
            store.entries().is_empty(),
            "the store evicts its oldest tile"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn fingerprints_the_contents_of_a_directory_source() {
        let directory = directory();
        let nested = directory.join("0").join("0");
        std::fs::create_dir_all(&nested).expect("the store layout is created");
        let chunk = nested.join("0.0.0");
        std::fs::write(&chunk, b"first").expect("the chunk is written");
        let source = crate::Source::file_series(directory.clone(), 0);
        let first = identity(&source, b"geometry");
        assert_eq!(
            identity(&source, b"geometry"),
            first,
            "an unchanged directory keeps its fingerprint"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&chunk, b"second").expect("the chunk is overwritten in place");
        assert_ne!(
            identity(&source, b"geometry"),
            first,
            "an overwritten chunk invalidates the fingerprint"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn clears_every_entry_of_a_cache_root() {
        let directory = directory();
        let store = Store::open(&directory).expect("the store opens");
        let extent = Extent::new(4, 3, 2);
        let data = VoxelData::U16((0..24u16).collect());
        store.save(key([5, 0, 1]), shape(extent), &data);
        assert_eq!(store.entries().len(), 1);
        let freed = clear(&directory).expect("the cache root clears");
        assert!(freed > 0, "the cleared entries report their bytes");
        assert!(store.entries().is_empty(), "no entry survives the clear");
        assert_eq!(
            clear(&directory).expect("an empty cache root clears"),
            0,
            "clearing an empty cache frees nothing"
        );
        store.save(key([6, 0, 1]), shape(extent), &data);
        assert_eq!(
            store.entries().len(),
            1,
            "the store writes tiles after its root was cleared"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
