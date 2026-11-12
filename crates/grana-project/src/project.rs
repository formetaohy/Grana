use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use grana_animation::Timeline;
use grana_display::CameraState;
use grana_geometry::GeometryStore;
use grana_scene::{Dataset, Measure, MeasureState, ObjectFrame, Scene, SceneShape, SceneState};
use grana_volume::Fingerprint;

use grana_codec::Encoder;

use crate::codec::{Decode, Encode, Reader};
use crate::error::Error;

const MAGIC: &[u8; 9] = b"granaproj";
const VERSION: u32 = 67;
const VERSION_BYTES: usize = size_of::<u32>();
const LENGTH: usize = size_of::<u64>();
const STEPS: usize = size_of::<u32>();
const HEADER: usize = MAGIC.len() + VERSION_BYTES + LENGTH + STEPS;
const COMPRESSION_LEVEL: i32 = 6;
const RAW_LIMIT: u64 = 16 << 30;
const STALE_AGE: Duration = Duration::from_secs(3600);

pub const EXTENSION: &str = "grana";

pub fn is_project(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

#[derive(Debug, PartialEq)]
pub struct Project {
    pub shape: Shape,
    pub source: Source,
    pub fingerprint: Fingerprint,
    pub scene: SceneState,
    pub measure: MeasureState,
    pub view: ViewState,
    pub views: Vec<NamedView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedView {
    pub name: String,
    pub view: ViewState,
}

impl NamedView {
    pub fn new(name: impl Into<String>, view: ViewState) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "a named view carries a name of its own"
        );
        Self {
            name: name.trim().to_string(),
            view,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Shape {
    pub extent: [u32; 3],
    pub spacing: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mismatch {
    Dataset,
    Shape,
}

#[derive(Debug, PartialEq)]
pub enum Source {
    Synthetic {
        name: String,
    },
    File {
        name: String,
        path: PathBuf,
        series: usize,
    },
    Remote {
        name: String,
        url: String,
        series: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    pub camera: CameraState,
    pub timeline: Timeline,
    pub export: Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Export {
    pub image: u32,
    pub movie: u32,
    pub transparent: bool,
}

impl Export {
    pub const NATIVE: Self = Self {
        image: 0,
        movie: grana_display::OutputSize::DEFAULT_WIDTH,
        transparent: false,
    };

    pub const fn new(image: u32, movie: u32, transparent: bool) -> Self {
        Self {
            image,
            movie,
            transparent,
        }
    }

    pub fn matches(&self) -> bool {
        (self.image == 0
            || (grana_display::OutputSize::MIN..=grana_display::OutputSize::MAX)
                .contains(&self.image))
            && (grana_display::OutputSize::MIN..=grana_display::OutputSize::MAX)
                .contains(&self.movie)
    }
}

impl Project {
    pub fn capture(
        scene: &Scene,
        measure: &Measure,
        source: Source,
        view: ViewState,
        views: Vec<NamedView>,
        fingerprint: Fingerprint,
    ) -> Self {
        let extent = scene.dataset.extent();
        let spacing = scene.dataset.spacing();
        Self {
            shape: Shape {
                extent: [extent.x(), extent.y(), extent.z()],
                spacing: [spacing.x(), spacing.y(), spacing.z()],
            },
            source,
            fingerprint,
            scene: scene.state(),
            measure: measure.state(),
            view,
            views,
        }
    }

    pub fn matches(&self, dataset: &Dataset, fingerprint: Fingerprint) -> Result<(), Mismatch> {
        if self.fingerprint != fingerprint {
            return Err(Mismatch::Dataset);
        }
        let extent = dataset.extent();
        let spacing = dataset.spacing();
        let shape = self.shape.extent == [extent.x(), extent.y(), extent.z()]
            && self.shape.spacing == [spacing.x(), spacing.y(), spacing.z()];
        let scene = self
            .scene
            .validate(SceneShape::new(
                dataset.channels().len(),
                dataset.frame_count(),
            ))
            .is_ok();
        if !shape || !scene {
            return Err(Mismatch::Shape);
        }
        Ok(())
    }

    pub fn write(
        &self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<(), Error> {
        let atomic = grana_file::Atomic::file(path).map_err(|error| Error::Write {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let total = self.steps();
        self.write_stream(atomic.staging(), path, total, cancel, progress)?;
        atomic.commit().map_err(|error| Error::Write {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })
    }

    pub fn read(
        path: &Path,
        store: Arc<GeometryStore>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Self, Error> {
        let format = |detail: &str| {
            if cancel.load(Ordering::Relaxed) {
                return Error::Cancelled;
            }
            Error::Format {
                path: path.to_path_buf(),
                detail: detail.to_string(),
            }
        };
        let read = |detail: String| {
            if cancel.load(Ordering::Relaxed) {
                return Error::Cancelled;
            }
            Error::Read {
                path: path.to_path_buf(),
                detail,
            }
        };
        let mut file = std::fs::File::open(path).map_err(|error| read(error.to_string()))?;
        let mut header = [0u8; HEADER];
        file.read_exact(&mut header)
            .map_err(|_| format("the file does not carry a grana project header"))?;
        if &header[..MAGIC.len()] != MAGIC {
            return Err(format("the file does not carry a grana project header"));
        }
        let mut version = [0u8; VERSION_BYTES];
        version.copy_from_slice(&header[MAGIC.len()..MAGIC.len() + VERSION_BYTES]);
        let version = u32::from_le_bytes(version);
        if version != VERSION {
            return Err(format(&format!(
                "the project was written by schema {version}, and this build reads schema {VERSION}"
            )));
        }
        let start = MAGIC.len() + VERSION_BYTES;
        let mut length = [0u8; LENGTH];
        length.copy_from_slice(&header[start..start + LENGTH]);
        let mut steps = [0u8; STEPS];
        steps.copy_from_slice(&header[start + LENGTH..HEADER]);
        let total = u32::from_le_bytes(steps) as usize;
        let raw_length = u64::from_le_bytes(length);
        if raw_length > RAW_LIMIT {
            return Err(format("the project declares an impossible payload size"));
        }
        let raw_length = usize::try_from(raw_length)
            .map_err(|_| format("the project declares an impossible payload size"))?;
        let mut decoder = zstd::Decoder::new(file)
            .map_err(|error| format(&format!("the payload does not decompress: {error}")))?;
        let mut report = |done: usize| progress(done, total);
        let mut reader = Reader::new(&mut decoder, raw_length, store, cancel, &mut report);
        let project = Project::decode(&mut reader).map_err(format)?;
        reader.done().map_err(format)?;
        Ok(project)
    }

    fn steps(&self) -> usize {
        self.scene
            .sets
            .iter()
            .flat_map(|set| set.frames.iter().flatten())
            .map(|frame| match &**frame {
                ObjectFrame::Spots(_) => 1,
                ObjectFrame::Surfaces(set) => 2 + usize::from(set.topology().is_some()),
                ObjectFrame::Filaments(_) => 2,
                ObjectFrame::Cells(set) => 2 + usize::from(set.bodies().topology().is_some()),
            })
            .sum()
    }

    fn write_stream(
        &self,
        temporary: &Path,
        path: &Path,
        total: usize,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<(), Error> {
        let write = |detail: String| {
            if cancel.load(Ordering::Relaxed) {
                return Error::Cancelled;
            }
            Error::Write {
                path: path.to_path_buf(),
                detail,
            }
        };
        sweep(path);
        let mut file =
            std::fs::File::create(temporary).map_err(|error| write(error.to_string()))?;
        file.write_all(MAGIC)
            .map_err(|error| write(error.to_string()))?;
        file.write_all(&VERSION.to_le_bytes())
            .map_err(|error| write(error.to_string()))?;
        file.write_all(&[0u8; LENGTH + STEPS])
            .map_err(|error| write(error.to_string()))?;
        let mut encoder = zstd::Encoder::new(file, COMPRESSION_LEVEL)
            .map_err(|error| write(error.to_string()))?;
        encoder
            .include_checksum(true)
            .map_err(|error| write(error.to_string()))?;
        let raw = {
            let mut report = |done: usize| progress(done, total);
            let mut writer = Encoder::stream(&mut encoder, cancel, &mut report);
            self.encode(&mut writer);
            if let Some(error) = writer.fault() {
                return Err(write(error.to_string()));
            }
            writer.len()
        };
        let mut file = encoder.finish().map_err(|error| write(error.to_string()))?;
        file.seek(SeekFrom::Start((MAGIC.len() + VERSION_BYTES) as u64))
            .map_err(|error| write(error.to_string()))?;
        file.write_all(&(raw as u64).to_le_bytes())
            .map_err(|error| write(error.to_string()))?;
        let steps = u32::try_from(total).map_err(|_| write("too many steps".to_string()))?;
        file.write_all(&steps.to_le_bytes())
            .map_err(|error| write(error.to_string()))?;
        file.flush().map_err(|error| write(error.to_string()))?;
        file.sync_all().map_err(|error| write(error.to_string()))?;
        Ok(())
    }
}

fn sweep(path: &Path) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(directory) = path.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let prefix = format!(".{name}.");
    for entry in entries.flatten() {
        let candidate = entry.path();
        let Some(candidate_name) = candidate.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !candidate_name.starts_with(&prefix) || !candidate_name.ends_with(".part") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_none_or(|age| age >= STALE_AGE);
        if stale {
            let _ = std::fs::remove_file(&candidate);
        }
    }
}
