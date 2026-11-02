use std::fs::File;
use std::path::{Path, PathBuf};

use crate::Error;

pub(crate) struct Store {
    path: PathBuf,
    file: File,
    length: u64,
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path).map_err(|error| Error::open(path, error.to_string()))?;
        let length = file
            .metadata()
            .map_err(|error| Error::open(path, error.to_string()))?
            .len();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            length,
        })
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
        let inside = offset
            .checked_add(buffer.len() as u64)
            .is_some_and(|end| end <= self.length);
        if !inside {
            return Err(Error::read(
                &self.path,
                "the requested range lies outside the file",
            ));
        }
        let mut read = 0;
        while read < buffer.len() {
            let offset = offset + read as u64;
            let count = read_at(&self.file, &mut buffer[read..], offset)
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
