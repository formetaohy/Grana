use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use grana_volume::Volume;

use crate::dataset::{Dataset, LoadLimit};

pub struct Loaded {
    pub dataset: Dataset,
    pub volume: Volume,
}

pub struct Loader {
    path: PathBuf,
    receiver: Receiver<Result<Loaded, String>>,
}

impl Loader {
    pub fn spawn(path: PathBuf, limit: LoadLimit) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = path.clone();
        std::thread::spawn(move || {
            let result =
                Dataset::open(&worker, limit).map(|(dataset, volume)| Loaded { dataset, volume });
            let _ = sender.send(result);
        });
        Self { path, receiver }
    }

    pub fn name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("OME-Zarr dataset")
    }

    pub fn poll(&self) -> Option<Result<Loaded, String>> {
        self.receiver.try_recv().ok()
    }
}
