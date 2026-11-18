use std::path::Path;

use serde::{Deserialize, Serialize};

use grana_report::Quality;

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
    pub(super) labels: Vec<String>,
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
    #[serde(default)]
    pub(super) modified: Option<u64>,
}

pub(super) fn completed(
    folder: &Path,
    digest: u64,
    build: &str,
    content: u64,
    artifacts: &[String],
    modified: Option<u64>,
) -> Option<Record> {
    let text = std::fs::read_to_string(folder.join(RESULT)).ok()?;
    let record = serde_json::from_str::<Record>(&text).ok()?;
    (record.protocol == digest
        && record.build == build
        && record.content == content
        && record.modified.is_none_or(|held| modified == Some(held))
        && artifacts.iter().all(|name| record.artifacts.contains(name))
        && record
            .artifacts
            .iter()
            .all(|name| folder.join(name).exists()))
    .then_some(record)
}

pub(super) fn build() -> String {
    format!("{}/{}", env!("CARGO_PKG_VERSION"), ANALYSIS_REVISION)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Evidence {
    pub(super) content: u64,
    pub(super) bytes: u64,
    pub(super) modified: Option<u64>,
}

pub(super) fn evidence(dataset: &Path) -> Result<Evidence, String> {
    match std::fs::metadata(dataset) {
        Err(error) => Err(format!(
            "the batch cannot measure {}: {error}",
            dataset.display()
        )),
        Ok(metadata) if metadata.is_dir() => directory_evidence(dataset),
        Ok(metadata) => Ok(Evidence {
            content: file_content(dataset, metadata.len())?,
            bytes: metadata.len(),
            modified: modified(&metadata),
        }),
    }
}

fn directory_evidence(root: &Path) -> Result<Evidence, String> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let mut hash = mix_u64(HASH, entries.len() as u64);
    let mut bytes = 0;
    let mut modified_at = None;
    for entry in &entries {
        hash = mix_u64(hash, entry.size);
        hash = mix_u64(hash, entry.name.len() as u64);
        hash = mix_bytes(hash, entry.name.as_bytes());
        bytes += entry.size;
        modified_at = modified_at.max(entry.modified);
        hash = file_bytes(hash, &root.join(&entry.name), entry.size)?;
    }
    Ok(Evidence {
        content: hash,
        bytes,
        modified: modified_at,
    })
}

struct Entry {
    name: String,
    size: u64,
    modified: Option<u64>,
}

fn collect(root: &Path, folder: &Path, entries: &mut Vec<Entry>) -> Result<(), String> {
    let listing = std::fs::read_dir(folder)
        .map_err(|error| format!("the batch cannot list {}: {error}", folder.display()))?;
    for entry in listing {
        let entry = entry.map_err(|error| {
            format!(
                "the batch cannot read an entry of {}: {error}",
                folder.display()
            )
        })?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("the batch cannot measure {}: {error}", path.display()))?;
        if metadata.is_dir() {
            collect(root, &path, entries)?;
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if derived(&name) {
            continue;
        }
        entries.push(Entry {
            size: metadata.len(),
            modified: modified(&metadata),
            name,
        });
    }
    Ok(())
}

fn derived(name: &str) -> bool {
    name.split('/').any(|part| part == "labels")
}

fn modified(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

fn file_content(path: &Path, length: u64) -> Result<u64, String> {
    file_bytes(HASH, path, length)
}

fn file_bytes(seed: u64, path: &Path, length: u64) -> Result<u64, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut hash = mix_u64(seed, length);
    let mut file = std::fs::File::open(path).map_err(|error| {
        format!(
            "the batch cannot open {} to fingerprint it: {error}",
            path.display()
        )
    })?;
    let span = CONTENT_SAMPLE.min(length);
    let mut buffer = vec![0u8; span as usize];
    for offset in [
        0,
        length.saturating_sub(span) / 2,
        length.saturating_sub(span),
    ] {
        file.seek(SeekFrom::Start(offset)).map_err(|error| {
            format!(
                "the batch cannot seek {} to fingerprint it: {error}",
                path.display()
            )
        })?;
        let count = file.read(&mut buffer).map_err(|error| {
            format!(
                "the batch cannot read {} to fingerprint it: {error}",
                path.display()
            )
        })?;
        hash = mix_bytes(hash, &buffer[..count]);
    }
    Ok(hash)
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
