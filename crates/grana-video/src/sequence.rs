use std::path::{Path, PathBuf};

use crate::error::Error;

pub fn frame_name(stem: &str, index: u32, time: u32) -> String {
    format!("{stem}-{index:04}-t{time:04}.png")
}

pub struct Sequence {
    directory: PathBuf,
    atomic: Option<grana_file::Atomic>,
    stem: String,
    frames: u32,
    written: u32,
}

impl Sequence {
    pub fn start(directory: &Path, stem: &str, frames: u32) -> Result<Self, Error> {
        assert!(frames > 0, "a frame sequence holds at least one frame");
        if directory.exists() {
            return Err(Error::Exists {
                path: directory.to_path_buf(),
            });
        }
        let atomic = grana_file::Atomic::directory(directory).map_err(|error| Error::Write {
            path: directory.to_path_buf(),
            detail: error.to_string(),
        })?;
        Ok(Self {
            directory: directory.to_path_buf(),
            atomic: Some(atomic),
            stem: stem.to_string(),
            frames,
            written: 0,
        })
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn frame(&self) -> u32 {
        self.written
    }

    pub fn complete(&self) -> bool {
        self.written >= self.frames
    }

    pub fn push(&mut self, time: u32, png: &[u8]) -> Result<(), Error> {
        assert!(
            self.written < self.frames,
            "a frame sequence holds no more than its frames"
        );
        let staging = self
            .atomic
            .as_ref()
            .expect("the sequence is open")
            .staging();
        let path = staging.join(frame_name(&self.stem, self.written, time));
        std::fs::write(&path, png).map_err(|error| Error::Write {
            path,
            detail: error.to_string(),
        })?;
        self.written += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<PathBuf, Error> {
        if !self.complete() {
            return Err(Error::Incomplete {
                index: self.written,
                frames: self.frames,
            });
        }
        self.atomic
            .take()
            .expect("the sequence is open")
            .commit()
            .map_err(|error| Error::Write {
                path: self.directory.clone(),
                detail: error.to_string(),
            })?;
        Ok(self.directory.clone())
    }

    pub fn discard(mut self) {
        self.atomic.take();
    }
}
