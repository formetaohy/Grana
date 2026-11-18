use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub(crate) const START: u64 = FNV_BASE;

pub(crate) struct Local {
    entries: Vec<(u64, u64, u128)>,
}

impl Local {
    pub(crate) fn of(root: &Path, directory: &str) -> io::Result<Self> {
        let prefix = directory.trim_matches('/');
        let mut entries = Vec::new();
        walk(&root.join(prefix), prefix, &mut entries)?;
        entries.sort_unstable_by_key(|entry| entry.0);
        Ok(Self { entries })
    }

    pub(crate) fn metadata(&self, key: &str) -> Option<(u64, u128)> {
        let hash = mix(FNV_BASE, key.as_bytes());
        self.entries
            .binary_search_by_key(&hash, |entry| entry.0)
            .ok()
            .map(|index| (self.entries[index].1, self.entries[index].2))
    }
}

pub(crate) fn mix(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn walk(base: &Path, prefix: &str, entries: &mut Vec<(u64, u64, u128)>) -> io::Result<()> {
    let directory = match fs::read_dir(base) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in directory {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a stored key is not utf-8",
            ));
        };
        let key = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            walk(&entry.path(), &key, entries)?;
        } else if metadata.is_file() {
            entries.push((
                mix(FNV_BASE, key.as_bytes()),
                metadata.len(),
                modified(&metadata),
            ));
        }
    }
    Ok(())
}

fn modified(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}
