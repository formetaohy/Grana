use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

const BUFFER: usize = 1 << 20;

#[derive(Serialize)]
struct Manifest {
    version: String,
    notes: String,
    portable: Entry,
    installer: Entry,
}

#[derive(Serialize)]
struct Entry {
    url: String,
    sha256: String,
    size: u64,
}

pub fn write(root: &Path) -> io::Result<PathBuf> {
    let version = env!("CARGO_PKG_VERSION");
    let repository = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let download = format!("{repository}/releases/download/v{version}");
    let name = format!("Grana-{version}-{}", platform()?);
    let manifest = Manifest {
        version: version.to_string(),
        notes: format!("{repository}/releases/tag/v{version}"),
        portable: entry(
            &root.join("dist").join(format!("{name}.zip")),
            &format!("{download}/{name}.zip"),
        )?,
        installer: entry(
            &root.join("dist").join(format!("{name}-setup.exe")),
            &format!("{download}/{name}-setup.exe"),
        )?,
    };
    let text = toml::to_string_pretty(&manifest).map_err(io::Error::other)?;
    let target = root.join("dist").join("latest.toml");
    fs::write(&target, text)?;
    Ok(target)
}

fn entry(path: &Path, url: &str) -> io::Result<Entry> {
    let metadata = fs::metadata(path).map_err(|error| {
        io::Error::other(format!(
            "the artifact `{}` is unavailable: {error}",
            path.display()
        ))
    })?;
    Ok(Entry {
        url: url.to_string(),
        sha256: digest(path)?,
        size: metadata.len(),
    })
}

fn digest(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    Ok(text)
}

fn platform() -> io::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("windows-x64"),
        (os, arch) => Err(io::Error::other(format!(
            "unsupported distribution platform: {os}-{arch}"
        ))),
    }
}
