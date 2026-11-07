use std::path::Path;

use serde::{Deserialize, Serialize};

use super::quality::Quality;
use super::{ANALYSIS_REVISION, RESULT};

const CONTENT_SAMPLE: u64 = 1 << 16;

const HASH: u64 = 0xcbf2_9ce4_8422_2325;

const PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) protocol: u64,
    pub(super) build: String,
    pub(super) content: u64,
    pub(super) artifacts: Vec<String>,
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
}

pub(super) fn completed(
    folder: &Path,
    digest: u64,
    build: &str,
    content: u64,
    artifacts: &[String],
) -> Option<Record> {
    let text = std::fs::read_to_string(folder.join(RESULT)).ok()?;
    let record = serde_json::from_str::<Record>(&text).ok()?;
    (record.protocol == digest
        && record.build == build
        && record.content == content
        && record.artifacts == artifacts
        && artifacts.iter().all(|name| folder.join(name).exists()))
    .then_some(record)
}

pub(super) fn build() -> String {
    format!("{}/{}", env!("CARGO_PKG_VERSION"), ANALYSIS_REVISION)
}

pub(super) fn content(dataset: &Path) -> u64 {
    match std::fs::metadata(dataset) {
        Err(_) => 0,
        Ok(metadata) if metadata.is_dir() => directory_content(dataset),
        Ok(metadata) => file_content(dataset, metadata.len()),
    }
}

fn file_content(path: &Path, length: u64) -> u64 {
    file_bytes(HASH, path, length)
}

fn file_bytes(seed: u64, path: &Path, length: u64) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    let mut hash = mix_u64(seed, length);
    let Ok(mut file) = std::fs::File::open(path) else {
        return hash;
    };
    let span = CONTENT_SAMPLE.min(length);
    let mut buffer = vec![0u8; span as usize];
    for offset in [
        0,
        length.saturating_sub(span) / 2,
        length.saturating_sub(span),
    ] {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            break;
        }
        let Ok(count) = file.read(&mut buffer) else {
            break;
        };
        hash = mix_bytes(hash, &buffer[..count]);
    }
    hash
}

fn directory_content(root: &Path) -> u64 {
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort();
    let mut hash = mix_u64(HASH, entries.len() as u64);
    for (name, size) in entries {
        hash = mix_u64(hash, size);
        hash = mix_u64(hash, name.len() as u64);
        hash = mix_bytes(hash, name.as_bytes());
        hash = file_bytes(hash, &root.join(&name), size);
    }
    hash
}

fn collect(root: &Path, folder: &Path, entries: &mut Vec<(String, u64)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect(root, &path, entries);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((name, metadata.len()));
    }
}

fn mix_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
    }
    hash
}

fn mix_u64(hash: u64, value: u64) -> u64 {
    mix_bytes(hash, &value.to_le_bytes())
}
