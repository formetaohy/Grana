use std::path::Path;

use grana_volume::Extent;
use openreadout_core::FileInfo;

use crate::error::Error;
use crate::open;

const EXTENSIONS: [&str; 9] = [
    "nd2", "lif", "lifext", "lof", "xlif", "xlef", "xlcf", "xllf", "ims",
];

const RGB_SAMPLES: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    extent: Extent,
    channels: usize,
    frames: u32,
    name: Option<String>,
}

impl Scene {
    fn new(extent: Extent, channels: usize, frames: u32, name: Option<String>) -> Self {
        Self {
            extent,
            channels,
            frames,
            name,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

pub fn scenes(path: &Path) -> Result<Vec<Scene>, Error> {
    let document = open::open(path)?;
    describe(path, &document.info)
}

pub fn scene_count(path: &Path) -> Result<usize, Error> {
    scenes(path).map(|scenes| scenes.len())
}

pub fn is_supported(path: &Path) -> bool {
    !path.is_dir() && open::registry().detect(path).is_ok()
}

pub fn has_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

pub fn dataset_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("dataset")
        .to_string()
}

pub(crate) fn describe(path: &Path, info: &FileInfo) -> Result<Vec<Scene>, Error> {
    if info.images.is_empty() {
        return Err(Error::unsupported(path, "the file holds no image"));
    }
    info.images
        .iter()
        .map(|image| {
            let extent = Extent::new(
                positive(path, image.size_x, "width")?,
                positive(path, image.size_y, "height")?,
                positive(path, image.size_z, "depth")?,
            );
            Ok(Scene::new(
                extent,
                channel_count(path, image.size_c, image.samples_per_pixel)?,
                image.size_t.max(1),
                image.name.clone().filter(|name| !name.trim().is_empty()),
            ))
        })
        .collect()
}

pub(crate) fn channel_count(path: &Path, channels: u32, samples: u32) -> Result<usize, Error> {
    let channels = usize::try_from(channels.max(1))
        .map_err(|_| Error::unsupported(path, "the image declares too many channels"))?;
    match samples {
        1 => Ok(channels),
        RGB_SAMPLES => Ok(channels * RGB_SAMPLES as usize),
        other => Err(Error::unsupported(
            path,
            format!("the image stores {other} samples per pixel"),
        )),
    }
}

fn positive(path: &Path, value: u32, axis: &str) -> Result<u32, Error> {
    if value == 0 {
        return Err(Error::unsupported(
            path,
            format!("the image declares an empty {axis}"),
        ));
    }
    Ok(value)
}
