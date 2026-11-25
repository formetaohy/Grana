use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use xxhash_rust::xxh3::Xxh3;

use crate::Error;
use crate::model::Asset;

const BUFFER: usize = 1 << 20;

fn verified() -> &'static Mutex<HashMap<PathBuf, (u64, String)>> {
    static VERIFIED: OnceLock<Mutex<HashMap<PathBuf, (u64, String)>>> = OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sidecar(path: &Path) -> PathBuf {
    path.with_extension("xxh128")
}

pub(crate) fn checked(asset: &Asset, path: &Path, cancel: &AtomicBool) -> Result<bool, Error> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(false);
    };
    let length = metadata.len();
    if length != asset.length {
        return Ok(false);
    }
    let recorded = recorded(path)?;
    if recorded.as_deref() == Some(asset.checksum)
        && verified()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(path)
            .is_some_and(|(held, digest)| *held == length && digest == asset.checksum)
    {
        return Ok(true);
    }
    let digest = digest(path, cancel)?;
    if digest != asset.checksum {
        return Ok(false);
    }
    record(path)?;
    Ok(true)
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
    let mut hasher = Xxh3::new();
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
    Ok(format!("{:032x}", hasher.digest128()))
}
