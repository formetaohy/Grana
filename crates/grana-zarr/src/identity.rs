use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::witness;

const FNV_BASE: u64 = witness::START;

pub(crate) fn dataset(directory: Option<&Path>, levels: &[(String, Vec<u8>)]) -> Option<u64> {
    let mut hash = FNV_BASE;
    if let Some(directory) = directory {
        let metadata = std::fs::metadata(directory).ok()?;
        hash = witness::mix(hash, &modified(&metadata).to_le_bytes());
    }
    hash = witness::mix(hash, &(levels.len() as u64).to_le_bytes());
    for (path, metadata) in levels {
        hash = witness::mix(hash, path.as_bytes());
        hash = witness::mix(hash, &[0xff]);
        hash = witness::mix(hash, metadata);
    }
    Some(hash)
}

pub(crate) fn chunks(
    keys: impl Iterator<Item = String>,
    metadata: impl Fn(&str) -> std::io::Result<Option<(u64, u128)>>,
) -> std::io::Result<u64> {
    let mut hash = FNV_BASE;
    for key in keys {
        hash = witness::mix(hash, key.as_bytes());
        match metadata(&key)? {
            Some((length, modified)) => {
                hash = witness::mix(hash, &[1]);
                hash = witness::mix(hash, &length.to_le_bytes());
                hash = witness::mix(hash, &modified.to_le_bytes());
            }
            None => hash = witness::mix(hash, &[0]),
        }
    }
    Ok(hash)
}

fn modified(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}
