use std::path::{Path, PathBuf};

use grana_geometry::PackedMesh;
use grana_store::{Crc32, DiskCache, DiskEntry};

const MAGIC: [u8; 8] = *b"GRANASRF";
const VERSION: u32 = 1;
const HEADER: usize = 80;
const TRAILER: usize = 4;
const SUFFIX: &str = "surface";
const DISK_BUDGET: u64 = 4 << 30;
const DISK_RETAIN: u64 = 3 << 30;
const OVERRIDE: &str = "GRANA_SURFACE_CACHE";
const EMPTY: u8 = 0;
const TRACED: u8 = 1;
const COLOR_BYTES: usize = 3 * size_of::<f32>();

#[derive(Clone, Copy)]
pub struct Key {
    pub context: u64,
    pub channel: usize,
    pub time: u32,
    pub source: u64,
}

pub enum Record {
    Empty,
    Surface {
        mesh: PackedMesh,
        colors: Vec<[f32; 3]>,
    },
}

pub struct Store {
    disk: DiskCache,
    namespace: u64,
    geometry: u64,
}

impl Store {
    pub fn open(root: &Path, namespace: u64, geometry: u64) -> Option<Self> {
        Some(Self {
            disk: DiskCache::open(root, DISK_BUDGET, DISK_RETAIN)?,
            namespace,
            geometry,
        })
    }

    pub fn load(&self, key: Key) -> Option<Record> {
        let bytes = self.disk.load(&self.entry(key))?;
        if bytes.len() < HEADER + TRAILER {
            return None;
        }
        let head = &bytes[..HEADER];
        let channel32 = u32::try_from(key.channel).ok()?;
        let kind = head[44];
        let matches = head[0..8] == MAGIC
            && read_u32(head, 8) == VERSION
            && read_u64(head, 12) == self.namespace
            && read_u64(head, 20) == self.geometry
            && read_u64(head, 28) == key.context
            && read_u32(head, 36) == channel32
            && read_u32(head, 40) == key.time
            && head[45..48] == [0; 3]
            && read_u64(head, 48) == key.source
            && head[72..HEADER] == [0; 8]
            && matches!(kind, EMPTY | TRACED);
        if !matches {
            return None;
        }
        let mesh_length = usize::try_from(read_u64(head, 56)).ok()?;
        let colors = usize::try_from(read_u64(head, 64)).ok()?;
        let payload = &bytes[HEADER..bytes.len() - TRAILER];
        let expected = mesh_length.checked_add(colors.checked_mul(COLOR_BYTES)?)?;
        if payload.len() != expected {
            return None;
        }
        let stored = read_u32(&bytes[bytes.len() - TRAILER..], 0);
        if stored != crc(head, payload) {
            return None;
        }
        if kind == EMPTY {
            return (mesh_length == 0 && colors == 0).then_some(Record::Empty);
        }
        let (mesh, rest) = payload.split_at(mesh_length);
        let mesh = PackedMesh::from_bytes(mesh)?;
        if mesh.object_count() != colors || mesh.object_count() == 0 {
            return None;
        }
        let mut decoded = Vec::with_capacity(colors);
        for color in rest.as_chunks::<COLOR_BYTES>().0 {
            let color = [
                f32::from_le_bytes([color[0], color[1], color[2], color[3]]),
                f32::from_le_bytes([color[4], color[5], color[6], color[7]]),
                f32::from_le_bytes([color[8], color[9], color[10], color[11]]),
            ];
            if color.iter().any(|component| !component.is_finite()) {
                return None;
            }
            decoded.push(color);
        }
        Some(Record::Surface {
            mesh,
            colors: decoded,
        })
    }

    pub fn save_empty(&self, key: Key) {
        self.save(key, EMPTY, &[], 0);
    }

    pub fn save_surface(&self, key: Key, mesh: &PackedMesh, colors: &[[f32; 3]]) {
        assert_eq!(
            colors.len(),
            mesh.object_count(),
            "a traced surface names one colour per label"
        );
        let mesh = mesh.to_bytes();
        let mut payload = Vec::with_capacity(mesh.len() + colors.len() * COLOR_BYTES);
        payload.extend_from_slice(&mesh);
        for color in colors {
            for component in color {
                payload.extend_from_slice(&component.to_le_bytes());
            }
        }
        self.save(key, TRACED, &payload, mesh.len() as u64);
    }

    fn save(&self, key: Key, kind: u8, payload: &[u8], mesh_length: u64) {
        let colors = if kind == EMPTY {
            0
        } else {
            (payload.len() - mesh_length as usize) / COLOR_BYTES
        } as u64;
        let mut head = [0u8; HEADER];
        head[0..8].copy_from_slice(&MAGIC);
        write_u32(&mut head, 8, VERSION);
        write_u64(&mut head, 12, self.namespace);
        write_u64(&mut head, 20, self.geometry);
        write_u64(&mut head, 28, key.context);
        write_u32(
            &mut head,
            36,
            u32::try_from(key.channel).expect("a dataset holds fewer than 4G channels"),
        );
        write_u32(&mut head, 40, key.time);
        head[44] = kind;
        write_u64(&mut head, 48, key.source);
        write_u64(&mut head, 56, mesh_length);
        write_u64(&mut head, 64, colors);
        let stored = crc(&head, payload);
        let mut bytes = Vec::with_capacity(HEADER + payload.len() + TRAILER);
        bytes.extend_from_slice(&head);
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&stored.to_le_bytes());
        self.disk.save(&self.entry(key), &bytes);
    }

    fn entry(&self, key: Key) -> DiskEntry {
        DiskEntry::new(
            format!("{:016x}-{:016x}", self.namespace, self.geometry),
            format!(
                "{:016x}-{:016x}-{:04x}-{:08x}.{SUFFIX}",
                key.source, key.context, key.channel, key.time
            ),
        )
    }
}

pub fn default_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(OVERRIDE) {
        let root = PathBuf::from(value);
        return (!root.as_os_str().is_empty()).then_some(root);
    }
    grana_dir::cache().map(|root| root.join("surface"))
}

pub fn clear(root: &Path) -> std::io::Result<u64> {
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
