use std::path::Path;

use crate::Error;
use crate::store::Store;

pub(crate) const TAG_IMAGE_WIDTH: u16 = 256;
pub(crate) const TAG_IMAGE_LENGTH: u16 = 257;
pub(crate) const TAG_BITS_PER_SAMPLE: u16 = 258;
pub(crate) const TAG_COMPRESSION: u16 = 259;
pub(crate) const TAG_PHOTOMETRIC: u16 = 262;
pub(crate) const TAG_IMAGE_DESCRIPTION: u16 = 270;
pub(crate) const TAG_STRIP_OFFSETS: u16 = 273;
pub(crate) const TAG_SAMPLES_PER_PIXEL: u16 = 277;
pub(crate) const TAG_ROWS_PER_STRIP: u16 = 278;
pub(crate) const TAG_STRIP_BYTE_COUNTS: u16 = 279;
pub(crate) const TAG_X_RESOLUTION: u16 = 282;
pub(crate) const TAG_Y_RESOLUTION: u16 = 283;
pub(crate) const TAG_PLANAR_CONFIGURATION: u16 = 284;
pub(crate) const TAG_RESOLUTION_UNIT: u16 = 296;
pub(crate) const TAG_PREDICTOR: u16 = 317;
pub(crate) const TAG_TILE_WIDTH: u16 = 322;
pub(crate) const TAG_TILE_LENGTH: u16 = 323;
pub(crate) const TAG_TILE_OFFSETS: u16 = 324;
pub(crate) const TAG_TILE_BYTE_COUNTS: u16 = 325;
pub(crate) const TAG_SUB_IFDS: u16 = 330;
pub(crate) const TAG_SAMPLE_FORMAT: u16 = 339;

const KIND_BYTE: u16 = 1;
const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;
const KIND_RATIONAL: u16 = 5;
const KIND_IFD: u16 = 13;
const KIND_LONG8: u16 = 16;
const KIND_IFD8: u16 = 18;

const ENTRY_LIMIT: u64 = 4096;
const DATA_LIMIT: u64 = 64 << 20;

pub(crate) struct Header {
    pub big_endian: bool,
    pub big: bool,
    pub first: u64,
}

pub(crate) struct Directory {
    big_endian: bool,
    big: bool,
    entries: Vec<Entry>,
    pub next: u64,
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u64,
    value: [u8; 8],
}

impl Entry {
    fn size(&self) -> Option<u64> {
        kind_size(self.kind)?.checked_mul(self.count)
    }
}

pub(crate) fn header(store: &Store, path: &Path) -> Result<Header, Error> {
    if store.length() < 8 {
        return Err(Error::open(path, "the file is shorter than a TIFF header"));
    }
    let start = store.read(0, 8)?;
    let big_endian = match &start[..2] {
        b"II" => false,
        b"MM" => true,
        _ => {
            return Err(Error::open(
                path,
                "the file does not carry a TIFF byte order mark",
            ));
        }
    };
    let version = u16_at(&start[2..4], big_endian);
    match version {
        42 => Ok(Header {
            big_endian,
            big: false,
            first: u64::from(u32_at(&start[4..8], big_endian)),
        }),
        43 => {
            let sizes = store.read(4, 4)?;
            if u16_at(&sizes[0..2], big_endian) != 8 || u16_at(&sizes[2..4], big_endian) != 0 {
                return Err(Error::open(
                    path,
                    "the BigTIFF header declares an unsupported offset size",
                ));
            }
            let first = store.read(8, 8)?;
            Ok(Header {
                big_endian,
                big: true,
                first: u64_at(&first, big_endian),
            })
        }
        _ => Err(Error::open(
            path,
            "the file does not carry a TIFF version number",
        )),
    }
}

impl Directory {
    pub(crate) fn read(
        store: &Store,
        header: &Header,
        offset: u64,
        path: &Path,
    ) -> Result<Self, Error> {
        let (count_size, entry_size, next_size) = if header.big {
            (8usize, 20usize, 8usize)
        } else {
            (2usize, 12usize, 4usize)
        };
        let count = {
            let raw = store.read(offset, count_size)?;
            if header.big {
                u64_at(&raw, header.big_endian)
            } else {
                u64::from(u16_at(&raw, header.big_endian))
            }
        };
        if count == 0 || count > ENTRY_LIMIT {
            return Err(Error::open(
                path,
                format!("the TIFF directory holds {count} entries"),
            ));
        }
        let body_offset = offset
            .checked_add(count_size as u64)
            .ok_or_else(|| Error::open(path, "the TIFF directory offset overflows"))?;
        let body = store.read(body_offset, count as usize * entry_size + next_size)?;
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let start = index * entry_size;
            let entry = &body[start..start + entry_size];
            let tag = u16_at(&entry[0..2], header.big_endian);
            let kind = u16_at(&entry[2..4], header.big_endian);
            let count = if header.big {
                u64_at(&entry[4..12], header.big_endian)
            } else {
                u64::from(u32_at(&entry[4..8], header.big_endian))
            };
            let mut value = [0u8; 8];
            let (value_start, value_size) = if header.big { (12, 8) } else { (8, 4) };
            value[..value_size].copy_from_slice(&entry[value_start..value_start + value_size]);
            entries.push(Entry {
                tag,
                kind,
                count,
                value,
            });
        }
        let next = {
            let start = count as usize * entry_size;
            let raw = &body[start..start + next_size];
            if header.big {
                u64_at(raw, header.big_endian)
            } else {
                u64::from(u32_at(raw, header.big_endian))
            }
        };
        Ok(Self {
            big_endian: header.big_endian,
            big: header.big,
            entries,
            next,
        })
    }

    pub(crate) fn integer(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Option<u64>, Error> {
        self.integers(store, tag, path)
            .map(|values| values.first().copied())
    }

    pub(crate) fn integers(&self, store: &Store, tag: u16, path: &Path) -> Result<Vec<u64>, Error> {
        let Some(entry) = self.entry(tag) else {
            return Ok(Vec::new());
        };
        let data = self.data(store, entry, path)?;
        let size = kind_size(entry.kind).ok_or_else(|| {
            Error::unsupported(path, format!("the TIFF tag {tag} carries an unknown type"))
        })? as usize;
        let mut values = Vec::with_capacity(entry.count as usize);
        for index in 0..entry.count as usize {
            let raw = &data[index * size..index * size + size];
            let value = match entry.kind {
                KIND_BYTE => u64::from(raw[0]),
                KIND_SHORT => u64::from(u16_at(raw, self.big_endian)),
                KIND_LONG | KIND_IFD => u64::from(u32_at(raw, self.big_endian)),
                KIND_LONG8 | KIND_IFD8 => u64_at(raw, self.big_endian),
                _ => {
                    return Err(Error::unsupported(
                        path,
                        format!("the TIFF tag {tag} does not hold integers"),
                    ));
                }
            };
            values.push(value);
        }
        Ok(values)
    }

    pub(crate) fn ascii(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Option<String>, Error> {
        let Some(entry) = self.entry(tag) else {
            return Ok(None);
        };
        if entry.kind != KIND_ASCII {
            return Err(Error::unsupported(
                path,
                format!("the TIFF tag {tag} is not an ASCII string"),
            ));
        }
        let data = self.data(store, entry, path)?;
        let end = data
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |index| index + 1);
        let text = std::str::from_utf8(&data[..end]).map_err(|error| {
            Error::metadata(path, format!("the TIFF tag {tag} is not UTF-8: {error}"))
        })?;
        Ok(Some(text.to_string()))
    }

    pub(crate) fn rational(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Option<f64>, Error> {
        let Some(entry) = self.entry(tag) else {
            return Ok(None);
        };
        if entry.kind != KIND_RATIONAL || entry.count == 0 {
            return Err(Error::unsupported(
                path,
                format!("the TIFF tag {tag} is not a rational number"),
            ));
        }
        let data = self.data(store, entry, path)?;
        let numerator = u32_at(&data[0..4], self.big_endian);
        let denominator = u32_at(&data[4..8], self.big_endian);
        if denominator == 0 {
            return Err(Error::metadata(
                path,
                format!("the TIFF tag {tag} divides by zero"),
            ));
        }
        Ok(Some(f64::from(numerator) / f64::from(denominator)))
    }

    fn entry(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.tag == tag)
    }

    fn data(&self, store: &Store, entry: &Entry, path: &Path) -> Result<Vec<u8>, Error> {
        let size = entry.size().ok_or_else(|| {
            Error::unsupported(
                path,
                format!("the TIFF tag {} carries an unknown type", entry.tag),
            )
        })?;
        if size > DATA_LIMIT {
            return Err(Error::metadata(
                path,
                format!("the TIFF tag {} is too large", entry.tag),
            ));
        }
        let inline = if self.big { 8 } else { 4 };
        if size <= inline {
            return Ok(entry.value[..size as usize].to_vec());
        }
        let offset = if self.big {
            u64_at(&entry.value, self.big_endian)
        } else {
            u64::from(u32_at(&entry.value, self.big_endian))
        };
        store.read(offset, size as usize)
    }
}

fn kind_size(kind: u16) -> Option<u64> {
    match kind {
        KIND_BYTE | KIND_ASCII => Some(1),
        KIND_SHORT => Some(2),
        KIND_LONG | KIND_IFD => Some(4),
        KIND_RATIONAL => Some(8),
        KIND_LONG8 | KIND_IFD8 => Some(8),
        _ => None,
    }
}

pub(crate) fn u16_at(bytes: &[u8], big_endian: bool) -> u16 {
    let bytes: [u8; 2] = bytes[..2].try_into().expect("two bytes are read");
    if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}

pub(crate) fn u32_at(bytes: &[u8], big_endian: bool) -> u32 {
    let bytes: [u8; 4] = bytes[..4].try_into().expect("four bytes are read");
    if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

pub(crate) fn u64_at(bytes: &[u8], big_endian: bool) -> u64 {
    let bytes: [u8; 8] = bytes[..8].try_into().expect("eight bytes are read");
    if big_endian {
        u64::from_be_bytes(bytes)
    } else {
        u64::from_le_bytes(bytes)
    }
}
