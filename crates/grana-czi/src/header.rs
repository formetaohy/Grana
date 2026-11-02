use std::path::Path;

use crate::Error;
use crate::binary::i64_at;
use crate::segment;
use crate::store::Store;

const FILE_NAME: &[u8] = b"ZISRAWFILE";
const HEADER_DATA: usize = 80;

pub(crate) struct Header {
    pub(crate) directory: u64,
    pub(crate) metadata: u64,
}

pub(crate) fn read(store: &Store, path: &Path) -> Result<Header, Error> {
    let segment = segment::read(store, 0, path)?;
    if !segment.is(FILE_NAME) {
        return Err(Error::open(path, "the file does not start with ZISRAWFILE"));
    }
    let data = store.read(segment.data_offset(), HEADER_DATA)?;
    let major =
        i64_at(&data, 0).ok_or_else(|| Error::open(path, "the file header is truncated"))?;
    if major != 1 {
        return Err(Error::unsupported(
            path,
            format!("the CZI file version {major} is not supported"),
        ));
    }
    let directory = i64_at(&data, 52)
        .filter(|position| *position > 0)
        .ok_or_else(|| Error::metadata(path, "the file header has no subblock directory"))?;
    let metadata = i64_at(&data, 60)
        .filter(|position| *position >= 0)
        .ok_or_else(|| Error::metadata(path, "the file header has an invalid metadata position"))?;
    Ok(Header {
        directory: directory as u64,
        metadata: metadata as u64,
    })
}
