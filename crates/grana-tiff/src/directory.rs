use std::collections::HashSet;
use std::path::Path;

use crate::error::Error;
use crate::store::Store;

const KIND_BYTE: u16 = 1;
const KIND_ASCII: u16 = 2;
const KIND_SHORT: u16 = 3;
const KIND_LONG: u16 = 4;
const KIND_RATIONAL: u16 = 5;
const KIND_SBYTE: u16 = 6;
const KIND_UNDEFINED: u16 = 7;
const KIND_SSHORT: u16 = 8;
const KIND_SLONG: u16 = 9;
const KIND_SRATIONAL: u16 = 10;
const KIND_FLOAT: u16 = 11;
const KIND_DOUBLE: u16 = 12;
const KIND_IFD: u16 = 13;
const KIND_LONG8: u16 = 16;
const KIND_SLONG8: u16 = 17;
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
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u64,
    field: [u8; 8],
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
    pub(crate) fn chain(store: &Store, header: &Header, path: &Path) -> Result<Vec<u64>, Error> {
        let mut offsets = Vec::new();
        let mut seen = HashSet::new();
        let mut offset = header.first;
        while offset != 0 {
            if !seen.insert(offset) {
                return Err(Error::open(path, "the TIFF directories form a cycle"));
            }
            let count = entry_count(store, header, offset, path)?;
            let next = offset
                .checked_add(header.count_size() as u64)
                .and_then(|start| start.checked_add(count * header.entry_size() as u64))
                .ok_or_else(|| Error::open(path, "the TIFF directory offset overflows"))?;
            let raw = store.read(next, header.next_size())?;
            offsets.push(offset);
            offset = header.offset_at(&raw);
        }
        if offsets.is_empty() {
            return Err(Error::unsupported(path, "the file holds no TIFF page"));
        }
        Ok(offsets)
    }

    pub(crate) fn read(
        store: &Store,
        header: &Header,
        offset: u64,
        path: &Path,
    ) -> Result<Self, Error> {
        let count = entry_count(store, header, offset, path)?;
        let body_offset = offset
            .checked_add(header.count_size() as u64)
            .ok_or_else(|| Error::open(path, "the TIFF directory offset overflows"))?;
        let entry_size = header.entry_size();
        let body = store.read(body_offset, count as usize * entry_size)?;
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let start = index * entry_size;
            let entry = &body[start..start + entry_size];
            let tag = u16_at(&entry[0..2], header.big_endian);
            let kind = u16_at(&entry[2..4], header.big_endian);
            let (count, value_size) = if header.big {
                (u64_at(&entry[4..12], header.big_endian), 8)
            } else {
                (u64::from(u32_at(&entry[4..8], header.big_endian)), 4)
            };
            let value_start = entry_size - value_size;
            let mut field = [0u8; 8];
            field[..value_size].copy_from_slice(&entry[value_start..value_start + value_size]);
            entries.push(Entry {
                tag,
                kind,
                count,
                field,
            });
        }
        Ok(Self {
            big_endian: header.big_endian,
            big: header.big,
            entries,
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
        let size = width(entry.kind).ok_or_else(|| {
            Error::unsupported(path, format!("the TIFF tag {tag} carries an unknown type"))
        })?;
        let mut values = Vec::with_capacity(entry.count as usize);
        for index in 0..entry.count as usize {
            let raw = &data[index * size..index * size + size];
            let value = match entry.kind {
                KIND_BYTE | KIND_UNDEFINED => u64::from(raw[0]),
                KIND_SBYTE => (raw[0] as i8 as i64) as u64,
                KIND_SHORT => u64::from(u16_at(raw, self.big_endian)),
                KIND_SSHORT => (u16_at(raw, self.big_endian) as i16 as i64) as u64,
                KIND_LONG | KIND_IFD => u64::from(u32_at(raw, self.big_endian)),
                KIND_SLONG => (u32_at(raw, self.big_endian) as i32 as i64) as u64,
                KIND_LONG8 | KIND_IFD8 => u64_at(raw, self.big_endian),
                KIND_SLONG8 => i64_at(raw, self.big_endian) as u64,
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
        if entry.kind != KIND_ASCII && entry.kind != KIND_BYTE && entry.kind != KIND_UNDEFINED {
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
        let text = match std::str::from_utf8(&data[..end]) {
            Ok(text) => text.to_string(),
            Err(_) => data[..end].iter().map(|byte| char::from(*byte)).collect(),
        };
        Ok(Some(text))
    }

    pub(crate) fn rational(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Option<f64>, Error> {
        Ok(self.rationals(store, tag, path)?.first().copied())
    }

    pub(crate) fn rationals(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Vec<f64>, Error> {
        let Some(entry) = self.entry(tag) else {
            return Ok(Vec::new());
        };
        if entry.kind != KIND_RATIONAL && entry.kind != KIND_SRATIONAL {
            return Err(Error::unsupported(
                path,
                format!("the TIFF tag {tag} is not a rational number"),
            ));
        }
        let data = self.data(store, entry, path)?;
        let mut values = Vec::with_capacity(entry.count as usize);
        for index in 0..entry.count as usize {
            let raw = &data[index * 8..index * 8 + 8];
            let (numerator, denominator) = (
                u32_at(&raw[0..4], self.big_endian) as i32,
                u32_at(&raw[4..8], self.big_endian) as i32,
            );
            if denominator == 0 {
                return Err(Error::metadata(
                    path,
                    format!("the TIFF tag {tag} divides by zero"),
                ));
            }
            values.push(f64::from(numerator) / f64::from(denominator));
        }
        Ok(values)
    }

    pub(crate) fn bytes(
        &self,
        store: &Store,
        tag: u16,
        path: &Path,
    ) -> Result<Option<Vec<u8>>, Error> {
        let Some(entry) = self.entry(tag) else {
            return Ok(None);
        };
        self.data(store, entry, path).map(Some)
    }

    fn entry(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.tag == tag)
    }

    fn data(&self, store: &Store, entry: &Entry, path: &Path) -> Result<Vec<u8>, Error> {
        let size = width(entry.kind)
            .and_then(|width| width.checked_mul(entry.count as usize))
            .ok_or_else(|| {
                Error::unsupported(
                    path,
                    format!("the TIFF tag {} carries an unknown type", entry.tag),
                )
            })?;
        if size as u64 > DATA_LIMIT {
            return Err(Error::metadata(
                path,
                format!("the TIFF tag {} is too large", entry.tag),
            ));
        }
        let inline = if self.big { 8 } else { 4 };
        if size <= inline {
            return Ok(entry.field[..size].to_vec());
        }
        let offset = if self.big {
            u64_at(&entry.field, self.big_endian)
        } else {
            u64::from(u32_at(&entry.field, self.big_endian))
        };
        store.read(offset, size)
    }
}

impl Header {
    fn count_size(&self) -> usize {
        if self.big { 8 } else { 2 }
    }

    fn entry_size(&self) -> usize {
        if self.big { 20 } else { 12 }
    }

    fn next_size(&self) -> usize {
        if self.big { 8 } else { 4 }
    }

    fn offset_at(&self, raw: &[u8]) -> u64 {
        if self.big {
            u64_at(raw, self.big_endian)
        } else {
            u64::from(u32_at(raw, self.big_endian))
        }
    }
}

fn entry_count(store: &Store, header: &Header, offset: u64, path: &Path) -> Result<u64, Error> {
    let raw = store.read(offset, header.count_size())?;
    let count = if header.big {
        u64_at(&raw, header.big_endian)
    } else {
        u64::from(u16_at(&raw, header.big_endian))
    };
    if count == 0 || count > ENTRY_LIMIT {
        return Err(Error::open(
            path,
            format!("the TIFF directory holds {count} entries"),
        ));
    }
    Ok(count)
}

fn width(kind: u16) -> Option<usize> {
    match kind {
        KIND_BYTE | KIND_ASCII | KIND_SBYTE | KIND_UNDEFINED => Some(1),
        KIND_SHORT | KIND_SSHORT => Some(2),
        KIND_LONG | KIND_SLONG | KIND_FLOAT | KIND_IFD => Some(4),
        KIND_RATIONAL | KIND_SRATIONAL | KIND_DOUBLE | KIND_LONG8 | KIND_SLONG8 | KIND_IFD8 => {
            Some(8)
        }
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

fn i64_at(bytes: &[u8], big_endian: bool) -> i64 {
    u64_at(bytes, big_endian) as i64
}
