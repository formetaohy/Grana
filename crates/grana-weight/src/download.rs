use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::Error;
use crate::model::Asset;

const BUFFER: usize = 1 << 20;

pub(crate) fn fetch(
    asset: &Asset,
    target: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Error> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| Error::storage(format!("`{}`: {error}", parent.display())))?;
    }
    let atomic = grana_file::Atomic::file(target)
        .map_err(|error| Error::storage(format!("`{}`: {error}", target.display())))?;
    let staging = atomic.staging().to_path_buf();
    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .new_agent();
    let response = agent
        .get(asset.url)
        .call()
        .map_err(|error| Error::download(error.to_string()))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(asset.length);
    let mut reader = response.into_body().into_reader();
    let mut writer = BufWriter::new(
        File::create(&staging)
            .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?,
    );
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    let mut written = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            return Err(Error::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| Error::download(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?;
        written += read as u64;
        progress(written.min(total), total);
    }
    writer
        .flush()
        .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?;
    drop(writer);
    if written != asset.length || crate::integrity::hex(&hasher.finalize()) != asset.checksum {
        return Err(Error::Checksum);
    }
    crate::archive::normalize(&staging)?;
    atomic
        .commit()
        .map_err(|error| Error::storage(format!("`{}`: {error}", target.display())))?;
    if let Err(error) = crate::integrity::record(target) {
        let _ = fs::remove_file(target);
        return Err(error);
    }
    Ok(())
}
