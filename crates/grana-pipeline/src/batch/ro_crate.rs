use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use grana_interop::Crate;
use sha2::{Digest as _, Sha256};

use super::Plan;
use super::pipeline::artifacts;
use super::quality::Row;

const FULL: u64 = 8 << 20;

const SAMPLE: u64 = 1 << 20;

const LISTING_LIMIT: usize = 100_000;

const ROOT_PARTS: [(&str, &str); 11] = [
    ("protocol.json", "application/json"),
    ("run.json", "application/json"),
    ("summary.csv", "text/csv"),
    ("quality.csv", "text/csv"),
    ("comparison.csv", "text/csv"),
    ("comparison_frames.csv", "text/csv"),
    ("comparison_significance.csv", "text/csv"),
    ("samples.csv", "text/csv"),
    ("sheet.csv", "text/csv"),
    ("sheet.png", "image/png"),
    ("significance.png", "image/png"),
];

struct Fingerprint {
    sha256: String,
    size: u64,
    coverage: &'static str,
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
            coverage: "complete file",
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
        coverage: "three sampled megabytes",
    })
}

fn directory_digest(path: &Path) -> Result<Fingerprint, String> {
    let mut entries = Vec::new();
    collect(path, path, &mut entries)?;
    entries.sort();
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    for (name, length) in entries {
        hasher.update(name.as_bytes());
        hasher.update(length.to_le_bytes());
        size += length;
    }
    Ok(Fingerprint {
        sha256: hex(&hasher.finalize()),
        size,
        coverage: "directory listing",
    })
}

fn collect(root: &Path, folder: &Path, entries: &mut Vec<(String, u64)>) -> Result<(), String> {
    let listing = std::fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?;
    for entry in listing {
        let entry =
            entry.map_err(|error| format!("failed to read {}: {error}", folder.display()))?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if metadata.is_dir() {
            collect(root, &path, entries)?;
        } else {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            entries.push((name, metadata.len()));
        }
        if entries.len() >= LISTING_LIMIT {
            return Ok(());
        }
    }
    Ok(())
}

fn media_of(file: &str) -> &'static str {
    match file.rsplit('.').next() {
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("png") => "image/png",
        Some("mp4") => "video/mp4",
        Some("ome") => "application/octet-stream",
        Some("zarr") | Some("granaproj") => "application/octet-stream",
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

fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = (seconds / 86_400) as i64;
    let day = seconds % 86_400;
    let (year, month, date) = civil(days);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}Z",
        day / 3_600,
        day % 3_600 / 60,
        day % 60
    )
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}
