use std::path::Path;

use crate::Error;
use crate::binary::i64_at;
use crate::store::Store;

pub(crate) const HEADER_LENGTH: u64 = 32;

pub(crate) struct Segment {
    pub(crate) id: [u8; 16],
    offset: u64,
    allocated: u64,
    used: u64,
}

impl Segment {
    pub(crate) fn is(&self, id: &[u8]) -> bool {
        self.id.starts_with(id)
    }

    pub(crate) fn data_offset(&self) -> u64 {
        self.offset + HEADER_LENGTH
    }

    pub(crate) fn body(&self) -> u64 {
        if self.used == 0 {
            self.allocated
        } else {
            self.used
        }
    }
}

pub(crate) fn read(store: &Store, offset: u64, path: &Path) -> Result<Segment, Error> {
    let header = store.read(offset, HEADER_LENGTH as usize)?;
    let mut id = [0u8; 16];
    id.copy_from_slice(&header[..16]);
    let allocated = i64_at(&header, 16)
        .filter(|size| *size >= 0)
        .ok_or_else(|| invalid(path, offset, "allocated size"))?;
    let used = i64_at(&header, 24)
        .filter(|size| *size >= 0)
        .ok_or_else(|| invalid(path, offset, "used size"))?;
    let segment = Segment {
        id,
        offset,
        allocated: allocated as u64,
        used: used as u64,
    };
    offset
        .checked_add(HEADER_LENGTH)
        .and_then(|start| start.checked_add(segment.body()))
        .filter(|end| *end <= store.length())
        .ok_or_else(|| invalid(path, offset, "extent"))?;
    Ok(segment)
}

fn invalid(path: &Path, offset: u64, what: &str) -> Error {
    Error::read(
        path,
        format!("the segment at {offset} declares an invalid {what}"),
    )
}
