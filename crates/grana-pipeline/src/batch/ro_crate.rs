use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use grana_interop::Crate;
use grana_interop::utc_now;
use sha2::{Digest as _, Sha256};

use super::Plan;
use super::Row;
use super::pipeline::artifacts;

const FULL: u64 = 8 << 20;

const SAMPLE: u64 = 1 << 20;

const LISTING_LIMIT: usize = 100_000;

const CONTENT_SAMPLES: usize = 256;

const CONTENT_SAMPLE: u64 = 64 << 10;

const CONTENT_TAIL: u64 = 4 << 10;

const ROOT_PARTS: [(&str, &str); 14] = [
    (
        grana_protocol::Protocol::FILE,
        grana_protocol::Protocol::MEDIA_TYPE,
    ),
    (super::manifest::RUN, "application/toml"),
    ("summary.csv", "text/csv"),
    ("quality.csv", "text/csv"),
    ("comparison.csv", "text/csv"),
    ("comparison_frames.csv", "text/csv"),
    ("comparison_significance.csv", "text/csv"),
    ("comparison_trend.csv", "text/csv"),
    ("samples.csv", "text/csv"),
    ("sheet.csv", "text/csv"),
    ("comparison.svg", "image/svg+xml"),
    ("report.pdf", "application/pdf"),
    ("sheet.png", "image/png"),
    ("significance.png", "image/png"),
];

struct Fingerprint {
    sha256: String,
    size: u64,
    coverage: String,
}

pub(super) fn write(plan: &Plan, rows: &[Option<Row>]) -> Result<(), String> {
    let name = plan.output.file_name().map_or_else(
        || "grana-batch".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut record = Crate::new(
        &name,
        "A batch analysis of microscopy datasets",
        "Grana",
        env!("CARGO_PKG_VERSION"),
    );
    record.date(&utc_now());
    for (file, media) in ROOT_PARTS {
        let path = plan.output.join(file);
        if !path.exists() {
            continue;
        }
        let digest = fingerprint(&path)?;
        record.hashed_part(
            file,
            file,
            media,
            &format!("the batch artifact {file} ({})", digest.coverage),
            Some(digest.size),
            Some(&digest.sha256),
        );
    }
    for (index, dataset) in plan.datasets.iter().enumerate() {
        let input = format!("#input-{}", index + 1);
        let source = fingerprint(dataset.path())?;
        record.input(
            &input,
            &dataset.title(),
            &format!(
                "{} · group {} · fingerprint over {}",
                dataset.label(),
                dataset.group(),
                source.coverage
            ),
            Some(dataset.path()),
            Some(&source.sha256),
            Some(source.size),
        );
        let folder = dataset.folder();
        let relative = folder.display().to_string().replace('\\', "/");
        let output = format!("#output-{}", index + 1);
        let row = rows.get(index).and_then(Option::as_ref);
        record.collection(
            &output,
            &relative,
            &format!(
                "{} objects · {}",
                row.map_or(0, |row| row.objects),
                row.map_or("queued", |row| row.status)
            ),
            Some(&plan.output.join(&folder)),
            None,
        );
        let mut results = vec![output.clone()];
        let mut files = artifacts(
            &plan.protocol,
            &dataset.title(),
            plan.project,
            plan.channels,
        );
        files.push(super::RESULT.to_string());
        files.sort();
        files.dedup();
        for file in files {
            let path = plan.output.join(&folder).join(&file);
            if !path.exists() {
                continue;
            }
            let digest = fingerprint(&path)?;
            let id = format!("{relative}/{file}");
            record.hashed_part(
                &id,
                &file,
                media_of(&file),
                &format!("the artifact {file} of {relative} ({})", digest.coverage),
                Some(digest.size),
                Some(&digest.sha256),
            );
            results.push(id);
        }
        record.action_on(
            &format!("#analyse-{}", index + 1),
            &format!("Analyse {}", dataset.title()),
            &format!(
                "{} objects; protocol {:016x}",
                row.map_or(0, |row| row.objects),
                plan.protocol.digest()
            ),
            &input,
            &results,
        );
    }
    record
        .write(&plan.output)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if metadata.is_file() {
        file_digest(path, metadata.len())
    } else {
        directory_digest(path)
    }
}

fn file_digest(path: &Path, size: u64) -> Result<Fingerprint, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    if size <= FULL {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        return Ok(Fingerprint {
            sha256: hex(&hasher.finalize()),
            size,
            coverage: "complete file".to_string(),
        });
    }
    let mut buffer = vec![0u8; SAMPLE as usize];
    for offset in [0, size / 2 - SAMPLE / 2, size - SAMPLE] {
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("failed to seek {}: {error}", path.display()))?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        hasher.update(offset.to_le_bytes());
        hasher.update(&buffer[..count]);
    }
    Ok(Fingerprint {
        sha256: hex(&hasher.finalize()),
        size,
        coverage: "three sampled megabytes".to_string(),
    })
}

fn directory_digest(path: &Path) -> Result<Fingerprint, String> {
    let mut listing = Listing::default();
    collect(path, path, &mut listing)?;
    listing.entries.sort();
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    for (name, length) in &listing.entries {
        hasher.update(name.as_bytes());
        hasher.update(length.to_le_bytes());
        size += length;
    }
    let mut sampled = 0usize;
    let step = listing.entries.len().div_ceil(CONTENT_SAMPLES).max(1);
    for (name, length) in listing.entries.iter().step_by(step) {
        if let Some(sample) = content(&path.join(name), *length)? {
            hasher.update(name.as_bytes());
            hasher.update(sample);
            sampled += 1;
        }
    }
    let mut coverage = format!(
        "{} directory entries with the contents of {sampled} sampled files",
        listing.entries.len()
    );
    if listing.truncated {
        coverage.push_str(&format!(
            ", truncated at {LISTING_LIMIT} entries of a larger tree"
        ));
    }
    Ok(Fingerprint {
        sha256: hex(&hasher.finalize()),
        size,
        coverage,
    })
}

#[derive(Default)]
struct Listing {
    entries: Vec<(String, u64)>,
    truncated: bool,
}

fn content(path: &Path, length: u64) -> Result<Option<Vec<u8>>, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut sample = Vec::with_capacity((CONTENT_SAMPLE + CONTENT_TAIL) as usize);
    let mut head = vec![0u8; CONTENT_SAMPLE.min(length) as usize];
    let count = file
        .read(&mut head)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    sample.extend_from_slice(&head[..count]);
    if length > 2 * CONTENT_SAMPLE {
        file.seek(SeekFrom::Start(length - CONTENT_TAIL))
            .map_err(|error| format!("failed to seek {}: {error}", path.display()))?;
        let mut tail = vec![0u8; CONTENT_TAIL as usize];
        let count = file
            .read(&mut tail)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        sample.extend_from_slice(&tail[..count]);
    }
    Ok((!sample.is_empty()).then_some(sample))
}

fn collect(root: &Path, folder: &Path, listing: &mut Listing) -> Result<(), String> {
    let mut names = std::fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|error| format!("failed to read {}: {error}", folder.display()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    names.sort();
    for name in names {
        if listing.entries.len() >= LISTING_LIMIT {
            listing.truncated = true;
            return Ok(());
        }
        let path = folder.join(&name);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if metadata.is_dir() {
            collect(root, &path, listing)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        listing.entries.push((relative, metadata.len()));
    }
    Ok(())
}

fn media_of(file: &str) -> &'static str {
    match file.rsplit('.').next() {
        Some("json") => "application/json",
        Some("toml") => "application/toml",
        Some("csv") => "text/csv",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("mp4") => "video/mp4",
        Some("ome") => "application/octet-stream",
        Some("zarr") | Some("grana") => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
