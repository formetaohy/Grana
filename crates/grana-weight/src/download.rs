use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;

use xxhash_rust::xxh3::Xxh3;

use crate::Error;
use crate::model::Asset;

const BUFFER: usize = 1 << 20;
const WORKERS: usize = 32;
const CHUNK: u64 = 2 << 20;
const ATTEMPTS: usize = 4;

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
    progress(0, asset.length);
    if !segmented(asset, &staging, cancel, progress)? {
        stream(asset, &staging, cancel, progress)?;
    }
    atomic
        .commit()
        .map_err(|error| Error::storage(format!("`{}`: {error}", target.display())))?;
    if let Err(error) = crate::integrity::record(target) {
        let _ = fs::remove_file(target);
        return Err(error);
    }
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .build()
        .new_agent()
}

fn segmented(
    asset: &Asset,
    staging: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<bool, Error> {
    let probe = match agent().get(asset.url).header("Range", "bytes=0-0").call() {
        Ok(response) => response,
        Err(_) => return Ok(false),
    };
    if probe.status() != 206 {
        return Ok(false);
    }
    drop(probe);
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(staging)
        .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?;
    file.set_len(asset.length)
        .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?;
    let next = Arc::new(AtomicU64::new(0));
    let failure = Arc::new(Mutex::new(None));
    let aborted = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel::<u64>();
    let mut handles = Vec::with_capacity(WORKERS);
    for _ in 0..WORKERS {
        let next = Arc::clone(&next);
        let failure = Arc::clone(&failure);
        let aborted = Arc::clone(&aborted);
        let sender = sender.clone();
        let url = asset.url;
        let total = asset.length;
        let path = staging.to_path_buf();
        handles.push(std::thread::spawn(move || {
            let agent = agent();
            let file = match OpenOptions::new().write(true).open(&path) {
                Ok(file) => file,
                Err(error) => {
                    *failure.lock().unwrap_or_else(|error| error.into_inner()) =
                        Some(Error::storage(format!("`{}`: {error}", path.display())));
                    aborted.store(true, Ordering::SeqCst);
                    return;
                }
            };
            loop {
                if aborted.load(Ordering::SeqCst) {
                    break;
                }
                let start = next.fetch_add(CHUNK, Ordering::SeqCst);
                if start >= total {
                    break;
                }
                let end = (start + CHUNK).min(total) - 1;
                let mut data = None;
                for attempt in 0..ATTEMPTS {
                    match agent
                        .get(url)
                        .header("Range", &format!("bytes={start}-{end}"))
                        .call()
                    {
                        Ok(response) => {
                            let mut body = Vec::with_capacity((end - start + 1) as usize);
                            let mut reader = response.into_body().into_reader();
                            if reader.read_to_end(&mut body).is_ok()
                                && body.len() as u64 == end - start + 1
                            {
                                data = Some(body);
                                break;
                            }
                        }
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(500 * (attempt as u64 + 1)));
                        }
                    }
                }
                let Some(data) = data else {
                    *failure.lock().unwrap_or_else(|error| error.into_inner()) = Some(
                        Error::download(format!("the byte range {start}-{end} never arrived")),
                    );
                    aborted.store(true, Ordering::SeqCst);
                    return;
                };
                if let Err(error) = write_at(&file, &data, start) {
                    *failure.lock().unwrap_or_else(|error| error.into_inner()) =
                        Some(Error::storage(format!("`{}`: {error}", path.display())));
                    aborted.store(true, Ordering::SeqCst);
                    return;
                }
                if sender.send(data.len() as u64).is_err() {
                    return;
                }
            }
        }));
    }
    drop(sender);
    let mut written = 0u64;
    for received in receiver {
        written += received;
        progress(written.min(asset.length), asset.length);
        if cancel.load(Ordering::Relaxed) {
            aborted.store(true, Ordering::SeqCst);
            break;
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if let Some(error) = failure
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        return Err(error);
    }
    if written != asset.length {
        return Err(Error::download("the segmented download is incomplete"));
    }
    verify(asset, staging)?;
    Ok(true)
}

fn stream(
    asset: &Asset,
    staging: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Error> {
    let response = agent()
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
        File::create(staging)
            .map_err(|error| Error::storage(format!("`{}`: {error}", staging.display())))?,
    );
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
    if written != asset.length {
        return Err(Error::download("the download is incomplete"));
    }
    verify(asset, staging)
}

#[cfg(windows)]
fn write_at(file: &File, data: &[u8], offset: u64) -> std::io::Result<()> {
    let mut written = 0;
    while written < data.len() {
        let count = file.seek_write(&data[written..], offset + written as u64)?;
        if count == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero));
        }
        written += count;
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &File, data: &[u8], offset: u64) -> std::io::Result<()> {
    file.write_all_at(data, offset)
}

fn verify(asset: &Asset, path: &Path) -> Result<(), Error> {
    let mut file = File::open(path)
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let mut hasher = Xxh3::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:032x}", hasher.digest128()) != asset.checksum {
        let _ = fs::remove_file(path);
        return Err(Error::Checksum);
    }
    Ok(())
}
