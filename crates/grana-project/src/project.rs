use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use grana_animation::Timeline;
use grana_display::{CameraState, SectionState};
use grana_scene::{Dataset, Measure, MeasureState, Scene, SceneState};

use crate::binary::{Decode, Encode, Reader, Writer};
use crate::error::Error;

const MAGIC: &[u8; 10] = b"granaproj4";
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
    pub timeline: Timeline,
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
        let temporary = temporary_path(path);
        let result = self.write_stream(&temporary, path).and_then(|()| {
            std::fs::rename(&temporary, path).map_err(|error| Error::Write {
                path: path.to_path_buf(),
                detail: error.to_string(),
            })
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
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
        let mut file = std::fs::File::open(path).map_err(|error| read(error.to_string()))?;
        let mut header = [0u8; HEADER];
        file.read_exact(&mut header)
            .map_err(|_| format("the file does not carry a grana project header"))?;
        if &header[..MAGIC.len()] != MAGIC {
            return Err(format("the file does not carry a grana project header"));
        }
        let mut length = [0u8; 8];
        length.copy_from_slice(&header[MAGIC.len()..HEADER]);
        let raw_length = u64::from_le_bytes(length);
        if raw_length > RAW_LIMIT {
            return Err(format("the project declares an impossible payload size"));
        }
        let raw_length = usize::try_from(raw_length)
            .map_err(|_| format("the project declares an impossible payload size"))?;
        let mut decoder = zstd::Decoder::new(file)
            .map_err(|error| format(&format!("the payload does not decompress: {error}")))?;
        let mut reader = Reader::new(&mut decoder, raw_length);
        let project = Project::decode(&mut reader).map_err(format)?;
        reader.done().map_err(format)?;
        Ok(project)
    }

    fn write_stream(&self, temporary: &Path, path: &Path) -> Result<(), Error> {
        let write = |detail: String| Error::Write {
            path: path.to_path_buf(),
            detail,
        };
        let mut file =
            std::fs::File::create(temporary).map_err(|error| write(error.to_string()))?;
        file.write_all(MAGIC)
            .map_err(|error| write(error.to_string()))?;
        file.write_all(&[0u8; size_of::<u64>()])
            .map_err(|error| write(error.to_string()))?;
        let mut encoder = zstd::Encoder::new(file, COMPRESSION_LEVEL)
            .map_err(|error| write(error.to_string()))?;
        encoder
            .include_checksum(true)
            .map_err(|error| write(error.to_string()))?;
        let raw = {
            let mut writer = Writer::new(&mut encoder);
            self.encode(&mut writer);
            if let Some(error) = writer.fault() {
                return Err(write(error.to_string()));
            }
            writer.raw()
        };
        let mut file = encoder.finish().map_err(|error| write(error.to_string()))?;
        file.seek(SeekFrom::Start(MAGIC.len() as u64))
            .map_err(|error| write(error.to_string()))?;
        file.write_all(&(raw as u64).to_le_bytes())
            .map_err(|error| write(error.to_string()))?;
        file.flush().map_err(|error| write(error.to_string()))?;
        Ok(())
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}
