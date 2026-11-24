use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) const START: u64 = FNV_BASE;

pub(crate) struct Local {
    root: PathBuf,
}

impl Local {
    pub(crate) fn new(root: &std::path::Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub(crate) fn metadata(&self, key: &str) -> io::Result<Option<(u64, u128)>> {
        match fs::metadata(self.root.join(key)) {
            Ok(metadata) => Ok(Some((metadata.len(), modified(&metadata)))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn mix(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn modified(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}
