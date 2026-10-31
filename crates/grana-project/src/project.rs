use std::io::Write;
use std::path::{Path, PathBuf};

use grana_render::{CameraState, SectionState};
use grana_scene::{Dataset, Measure, MeasureState, Scene, SceneState};

use crate::binary::{Decode, Encode, Reader, Writer};
use crate::error::Error;

const MAGIC: &[u8; 9] = b"granaproj";
const HEADER: usize = MAGIC.len() + size_of::<u64>();
const COMPRESSION_LEVEL: i32 = 6;
const RAW_LIMIT: u64 = 16 << 30;

#[derive(Debug, PartialEq)]
pub struct Project {
    pub shape: Shape,
    pub source: Source,
    pub scene: SceneState,
    pub measure: MeasureState,
    pub view: ViewState,
}

#[derive(Debug, PartialEq)]
pub struct Shape {
    pub extent: [u32; 3],
    pub spacing: [f32; 3],
}

#[derive(Debug, PartialEq)]
pub enum Source {
    Synthetic { name: String },
    Zarr { name: String, path: PathBuf },
}

#[derive(Debug, PartialEq)]
pub struct ViewState {
    pub camera: CameraState,
    pub section: SectionState,
    pub split: f32,
}

impl Project {
    pub fn capture(scene: &Scene, measure: &Measure, source: Source, view: ViewState) -> Self {
        let extent = scene.dataset.extent();
        let spacing = scene.dataset.spacing();
        Self {
            shape: Shape {
                extent: [extent.x(), extent.y(), extent.z()],
                spacing: [spacing.x(), spacing.y(), spacing.z()],
            },
            source,
            scene: scene.state(),
            measure: measure.state(),
            view,
        }
    }

    pub fn matches(&self, dataset: &Dataset) -> bool {
        let extent = dataset.extent();
        let spacing = dataset.spacing();
        self.shape.extent == [extent.x(), extent.y(), extent.z()]
            && self.shape.spacing == [spacing.x(), spacing.y(), spacing.z()]
            && self
                .scene
                .matches(dataset.channels().len(), dataset.frame_count())
    }

    pub fn write(&self, path: &Path) -> Result<(), Error> {
        let mut writer = Writer::new();
        self.encode(&mut writer);
        let raw = writer.finish();
        let compressed = compress(&raw).map_err(|detail| Error::Write {
            path: path.to_path_buf(),
            detail,
        })?;
        let mut file = Vec::with_capacity(HEADER + compressed.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        file.extend_from_slice(&compressed);
        std::fs::write(path, file).map_err(|error| Error::Write {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })
    }

    pub fn read(path: &Path) -> Result<Self, Error> {
        let format = |detail: &str| Error::Format {
            path: path.to_path_buf(),
            detail: detail.to_string(),
        };
        let read = |detail: String| Error::Read {
            path: path.to_path_buf(),
            detail,
        };
        let bytes = std::fs::read(path).map_err(|error| read(error.to_string()))?;
        if bytes.len() < HEADER || &bytes[..MAGIC.len()] != MAGIC {
            return Err(format("the file does not carry a grana project header"));
        }
        let mut length = [0u8; 8];
        length.copy_from_slice(&bytes[MAGIC.len()..HEADER]);
        let raw_length = u64::from_le_bytes(length);
        if raw_length > RAW_LIMIT {
            return Err(format("the project declares an impossible payload size"));
        }
        let raw = zstd::bulk::decompress(&bytes[HEADER..], raw_length as usize)
            .map_err(|error| format(&format!("the payload does not decompress: {error}")))?;
        let mut reader = Reader::new(&raw);
        let project = Project::decode(&mut reader).map_err(format)?;
        reader.done().map_err(format)?;
        Ok(project)
    }
}

fn compress(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder =
        zstd::Encoder::new(Vec::new(), COMPRESSION_LEVEL).map_err(|error| error.to_string())?;
    encoder
        .include_checksum(true)
        .map_err(|error| error.to_string())?;
    encoder.write_all(raw).map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}
