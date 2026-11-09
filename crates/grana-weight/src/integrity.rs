use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::Error;
use crate::model::Asset;

const BUFFER: usize = 1 << 20;

fn verified() -> &'static Mutex<HashMap<PathBuf, (u64, String)>> {
    static VERIFIED: OnceLock<Mutex<HashMap<PathBuf, (u64, String)>>> = OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("sha256")
}

pub(crate) fn checked(asset: &Asset, path: &Path, cancel: &AtomicBool) -> Result<bool, Error> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(false);
    };
    let length = metadata.len();
    if length != asset.length && length != asset.stored_length {
        return Ok(false);
    }
    let recorded = recorded(path)?;
    if let Some((held, digest)) = verified()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        && *held == length
        && recorded.as_deref() == Some(digest.as_str())
    {
        return Ok(true);
    }
    let digest = digest(path, cancel)?;
    if digest == asset.checksum {
        crate::archive::normalize(path)?;
        record(path)?;
        return Ok(true);
    }
    if length == asset.stored_length && recorded.as_deref() == Some(digest.as_str()) {
        remember(path, &digest);
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn record(path: &Path) -> Result<(), Error> {
    let digest = digest(path, &AtomicBool::new(false))?;
    let sidecar = sidecar(path);
    fs::write(&sidecar, &digest)
        .map_err(|error| Error::storage(format!("`{}`: {error}", sidecar.display())))?;
    remember(path, &digest);
    Ok(())
}

fn remember(path: &Path, digest: &str) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    verified()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(path.to_path_buf(), (metadata.len(), digest.to_string()));
}

fn recorded(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(sidecar(path)) {
        Ok(text) => Ok(Some(text.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::storage(format!(
            "`{}`: {error}",
            sidecar(path).display()
        ))),
    }
}

fn digest(path: &Path, cancel: &AtomicBool) -> Result<String, Error> {
    let mut file = File::open(path)
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let read = file
            .read(&mut buffer)
            .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
