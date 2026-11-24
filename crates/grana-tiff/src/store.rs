use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::Error;

const HANDLE_LIMIT: usize = 64;

pub(crate) struct Handles {
    limit: usize,
    open: Mutex<Vec<(PathBuf, Arc<File>)>>,
}

impl Handles {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        assert!(
            limit > 0,
            "a handle pool keeps at least one file open at a time"
        );
        Arc::new(Self {
            limit,
            open: Mutex::new(Vec::new()),
        })
    }

    fn acquire(&self, path: &Path) -> Result<Arc<File>, Error> {
        {
            let mut open = self.open.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(index) = open.iter().position(|(held, _)| held == path) {
                let entry = open.remove(index);
                let file = Arc::clone(&entry.1);
                open.push(entry);
                return Ok(file);
            }
        }
        let file =
            Arc::new(File::open(path).map_err(|error| Error::open(path, error.to_string()))?);
        let mut open = self.open.lock().unwrap_or_else(|error| error.into_inner());
        open.push((path.to_path_buf(), Arc::clone(&file)));
        while open.len() > self.limit {
            open.remove(0);
        }
        Ok(file)
    }
}

pub(crate) struct Store {
    path: PathBuf,
    length: u64,
    handles: Arc<Handles>,
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        Self::open_with(path, Handles::new(HANDLE_LIMIT))
    }

    pub(crate) fn open_with(path: &Path, handles: Arc<Handles>) -> Result<Self, Error> {
        let length = std::fs::metadata(path)
            .map_err(|error| Error::open(path, error.to_string()))?
            .len();
        Ok(Self {
            path: path.to_path_buf(),
            length,
            handles,
        })
    }

    pub(crate) fn handles(&self) -> Arc<Handles> {
        Arc::clone(&self.handles)
    }

    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    pub(crate) fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; length];
        self.read_into(&mut buffer, offset)?;
        Ok(buffer)
    }

    pub(crate) fn read_into(&self, buffer: &mut [u8], offset: u64) -> Result<(), Error> {
        let end = offset
            .checked_add(buffer.len() as u64)
            .filter(|end| *end <= self.length);
        if end.is_none() {
            return Err(Error::read(
                &self.path,
                "the requested range lies outside the file",
            ));
        }
        let file = self.handles.acquire(&self.path)?;
        let mut read = 0;
        while read < buffer.len() {
            let offset = offset + read as u64;
            let count = read_at(&file, &mut buffer[read..], offset)
                .map_err(|error| Error::read(&self.path, error.to_string()))?;
            if count == 0 {
                return Err(Error::read(
                    &self.path,
                    "the file ended before the requested range",
                ));
            }
            read += count;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buffer, offset)
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buffer, offset)
}
