use std::path::Path;
use std::time::UNIX_EPOCH;

const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) fn dataset(root: &Path, levels: &[(String, Vec<u8>)]) -> Option<u64> {
    let directory = std::fs::metadata(root).ok()?;
    let mut hash = mix(FNV_BASE, &modified(&directory).to_le_bytes());
    hash = mix(hash, &(levels.len() as u64).to_le_bytes());
    for (path, metadata) in levels {
        hash = mix(hash, path.as_bytes());
        hash = mix(hash, &[0xff]);
        hash = mix(hash, metadata);
    }
    Some(hash)
}

pub(crate) fn chunks(root: &Path, keys: impl Iterator<Item = String>) -> Option<u64> {
    let mut hash = FNV_BASE;
    for key in keys {
        hash = mix(hash, key.as_bytes());
        match std::fs::metadata(root.join(&key)) {
            Ok(metadata) => {
                hash = mix(hash, &[1]);
                hash = mix(hash, &metadata.len().to_le_bytes());
                hash = mix(hash, &modified(&metadata).to_le_bytes());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash = mix(hash, &[0]),
            Err(_) => return None,
        }
    }
    Some(hash)
}

fn modified(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn mix(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}
