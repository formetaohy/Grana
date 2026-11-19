use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use grana_exec::Cancel;
use grana_report::Quality;
use grana_volume::Fingerprint;

use super::{ANALYSIS_REVISION, RESULT};

const CHUNK: usize = 1 << 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    #[serde(with = "super::hash::hex")]
    pub(super) protocol: u64,
    pub(super) build: String,
    #[serde(with = "super::hash::fingerprint")]
    pub(super) content: Fingerprint,
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
    content: Fingerprint,
    artifacts: &[String],
    modified: Option<u64>,
) -> Option<Record> {
    let text = std::fs::read_to_string(folder.join(RESULT)).ok()?;
    let record = toml::from_str::<Record>(&text).ok()?;
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
    pub(super) content: Fingerprint,
    pub(super) bytes: u64,
    pub(super) modified: Option<u64>,
}

pub(super) fn evidence(dataset: &Path, cancel: &Cancel) -> Result<Option<Evidence>, String> {
    match std::fs::metadata(dataset) {
        Err(error) => Err(format!(
            "the batch cannot measure {}: {error}",
            dataset.display()
        )),
        Ok(metadata) if metadata.is_dir() => directory_evidence(dataset, cancel),
        Ok(metadata) => {
            let mut hasher = Sha256::new();
            if !digest_file(&mut hasher, dataset, cancel)? {
                return Ok(None);
            }
            Ok(Some(Evidence {
                content: Fingerprint::new(hasher.finalize().into()),
                bytes: metadata.len(),
                modified: modified(&metadata),
            }))
        }
    }
}

fn directory_evidence(root: &Path, cancel: &Cancel) -> Result<Option<Evidence>, String> {
    let mut entries = Vec::new();
    if !collect(root, root, &mut entries, cancel)? {
        return Ok(None);
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let mut hasher = Sha256::new();
    hasher.update((entries.len() as u64).to_le_bytes());
    let mut bytes = 0;
    let mut modified_at = None;
    for entry in &entries {
        hasher.update(entry.size.to_le_bytes());
        hasher.update((entry.name.len() as u64).to_le_bytes());
        hasher.update(entry.name.as_bytes());
        if !digest_file(&mut hasher, &root.join(&entry.name), cancel)? {
            return Ok(None);
        }
        bytes += entry.size;
        modified_at = modified_at.max(entry.modified);
    }
    Ok(Some(Evidence {
        content: Fingerprint::new(hasher.finalize().into()),
        bytes,
        modified: modified_at,
    }))
}

struct Entry {
    name: String,
    size: u64,
    modified: Option<u64>,
}

fn collect(
    root: &Path,
    folder: &Path,
    entries: &mut Vec<Entry>,
    cancel: &Cancel,
) -> Result<bool, String> {
    if cancel.cancelled() {
        return Ok(false);
    }
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
            if !collect(root, &path, entries, cancel)? {
                return Ok(false);
            }
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
    Ok(true)
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

fn digest_file(hasher: &mut Sha256, path: &Path, cancel: &Cancel) -> Result<bool, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| {
        format!(
            "the batch cannot open {} to fingerprint it: {error}",
            path.display()
        )
    })?;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if cancel.cancelled() {
            return Ok(false);
        }
        let count = file.read(&mut buffer).map_err(|error| {
            format!(
                "the batch cannot read {} to fingerprint it: {error}",
                path.display()
            )
        })?;
        if count == 0 {
            return Ok(true);
        }
        hasher.update(&buffer[..count]);
    }
}
