use std::path::{Path, PathBuf};

use grana_archive::ImageSpec;

use crate::error::Error;
use crate::label::{self, LabelSpec};
use crate::write;

pub struct Archive {
    root: PathBuf,
    atomic: grana_file::Atomic,
    group: Option<String>,
    version: Option<String>,
    labels: Vec<String>,
}

impl Archive {
    pub fn create(path: &Path, expected: u64) -> Result<Self, Error> {
        if path.is_file() {
            return Err(fault(path, "the destination is a file".to_string()));
        }
        grana_file::require(path, expected).map_err(|error| fault(path, error.to_string()))?;
        let atomic =
            grana_file::Atomic::directory(path).map_err(|error| fault(path, error.to_string()))?;
        Ok(Self {
            root: path.to_path_buf(),
            atomic,
            group: None,
            version: None,
            labels: Vec::new(),
        })
    }

    pub fn image(&mut self, spec: &mut ImageSpec<'_>) -> Result<(), Error> {
        spec.check();
        write::build(self.atomic.staging(), spec).map_err(|detail| fault(&self.root, detail))?;
        if spec.cancelled() {
            return Err(fault(&self.root, write::CANCELLED.to_string()));
        }
        Ok(())
    }

    pub fn labels(&mut self, group: &str, spec: LabelSpec<'_>) -> Result<(), Error> {
        let name = spec.name.to_string();
        let version = label::write_inside(self.atomic.staging(), group, spec)
            .map_err(|detail| fault(&self.root, detail))?;
        match &self.version {
            Some(held) => assert_eq!(
                held, &version,
                "every label image of a dataset names one OME-Zarr version"
            ),
            None => self.version = Some(version),
        }
        match &self.group {
            Some(held) => assert_eq!(
                held, group,
                "every label image of a dataset joins one image group"
            ),
            None => self.group = Some(group.to_string()),
        }
        self.labels.push(name);
        Ok(())
    }

    pub fn commit(self) -> Result<u64, Error> {
        let Self {
            root,
            atomic,
            group,
            version,
            labels,
        } = self;
        if let (Some(version), Some(group)) = (version, group) {
            label::write_index(atomic.staging(), &group, &version, &labels)
                .map_err(|detail| fault(&root, detail))?;
        }
        let bytes = grana_file::bytes(atomic.staging()).map_err(|error| {
            fault(
                &root,
                format!("failed to measure the written dataset: {error}"),
            )
        })?;
        atomic.commit().map_err(|error| {
            fault(
                &root,
                format!("failed to move the dataset into place: {error}"),
            )
        })?;
        Ok(bytes)
    }
}

fn fault(path: &Path, detail: String) -> Error {
    Error::Write {
        path: path.display().to_string(),
        detail,
    }
}
