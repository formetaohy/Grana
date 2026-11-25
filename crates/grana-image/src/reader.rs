use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{Extent, Pyramid, VolumeSeries, VoxelData};
use grana_zarr::{Labels, Plate};

use crate::{Error, Series, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
    Czi,
    Readout,
}

pub(crate) enum Reader {
    Memory(Arc<VolumeSeries>),
    Zarr(grana_zarr::Image),
    Tiff(grana_tiff::Image),
    Czi(grana_czi::Image),
    Readout(grana_readout::Image),
}

impl Reader {
    pub(crate) fn open_file(
        path: &Path,
        series: usize,
        group: Option<&str>,
    ) -> Result<(Self, Vec<Series>), Error> {
        if path.is_dir() {
            let root = grana_zarr::Root::local(path)?;
            let (image, series) = open_zarr(root, path, series, group)?;
            return Ok((Self::Zarr(image), series));
        }
        if is_czi(path)? {
            let image = grana_czi::Image::open(path, series)?;
            let scenes = image
                .scenes()
                .iter()
                .map(|scene| {
                    Series::new(
                        scene.extent(),
                        scene.channels(),
                        scene.frames(),
                        scene.name().map(str::to_string),
                    )
                })
                .collect();
            return Ok((Self::Czi(image), scenes));
        }
        if is_tiff(path)? {
            let image = grana_tiff::Image::open(path, series)?;
            let series = image
                .series()
                .iter()
                .map(|series| {
                    Series::new(
                        series.extent(),
                        series.channels(),
                        series.frames(),
                        series.name().map(str::to_string),
                    )
                })
                .collect();
            return Ok((Self::Tiff(image), series));
        }
        if grana_readout::is_supported(path) {
            let image = grana_readout::Image::open(path, series)?;
            let scenes = image
                .scenes()
                .iter()
                .map(|scene| {
                    Series::new(
                        scene.extent(),
                        scene.channels(),
                        scene.frames(),
                        scene.name().map(str::to_string),
                    )
                })
                .collect();
            return Ok((Self::Readout(image), scenes));
        }
        Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: "the file stores no image grana can read".to_string(),
        })
    }

    pub(crate) fn open_remote(
        url: &str,
        series: usize,
        group: Option<&str>,
    ) -> Result<(Self, Vec<Series>), Error> {
        let root = grana_zarr::Root::remote(url)?;
        let (image, series) = open_zarr(root, Path::new(url), series, group)?;
        Ok((Self::Zarr(image), series))
    }

    pub(crate) fn pyramid(&self) -> &Pyramid {
        match self {
            Self::Memory(series) => series.pyramid(),
            Self::Zarr(image) => image.pyramid(),
            Self::Tiff(image) => image.pyramid(),
            Self::Czi(image) => image.pyramid(),
            Self::Readout(image) => image.pyramid(),
        }
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Memory(_) => "synthetic",
            Self::Zarr(image) => image.name(),
            Self::Tiff(image) => image.name(),
            Self::Czi(image) => image.name(),
            Self::Readout(image) => image.name(),
        }
    }

    pub(crate) fn format(&self) -> Option<Format> {
        match self {
            Self::Memory(_) => None,
            Self::Zarr(_) => Some(Format::Zarr),
            Self::Tiff(_) => Some(Format::Tiff),
            Self::Czi(_) => Some(Format::Czi),
            Self::Readout(_) => Some(Format::Readout),
        }
    }

    pub(crate) fn family(&self) -> Option<grana_readout::Family> {
        match self {
            Self::Readout(image) => Some(image.family()),
            Self::Memory(_) | Self::Zarr(_) | Self::Tiff(_) | Self::Czi(_) => None,
        }
    }

    pub(crate) fn transfer(&self) -> Option<Arc<grana_zarr::Transfer>> {
        match self {
            Self::Zarr(image) => image.transfer(),
            Self::Memory(_) | Self::Tiff(_) | Self::Czi(_) | Self::Readout(_) => None,
        }
    }

    pub(crate) fn labels(&self) -> Result<Labels, Error> {
        match self {
            Self::Zarr(image) => Ok(image.labels()?),
            Self::Memory(_) | Self::Tiff(_) | Self::Czi(_) | Self::Readout(_) => {
                Ok(Labels::empty())
            }
        }
    }

    pub(crate) fn decode(&self) -> Option<grana_zarr::Decode> {
        match self {
            Self::Zarr(image) => Some(image.decode()),
            Self::Memory(_) | Self::Tiff(_) | Self::Czi(_) | Self::Readout(_) => None,
        }
    }

    pub(crate) fn digest(&self) -> Option<u64> {
        match self {
            Self::Zarr(image) => image.identity(),
            Self::Tiff(image) => Some(image.stamp()),
            Self::Memory(_) | Self::Czi(_) | Self::Readout(_) => None,
        }
    }

    pub(crate) fn read_plane(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        z: u32,
    ) -> Result<VoxelData, Error> {
        let extent = self.stored_extent(level);
        let x = 0..extent.x();
        let y = 0..extent.y();
        let plane = z..z + 1;
        match self {
            Self::Memory(series) => Ok(series.region(time, channel, x, y, plane)),
            Self::Zarr(image) => Ok(image.read_region(time, level, channel, x, y, plane)?),
            Self::Tiff(image) => Ok(image.read_region(time, level, channel, x, y, plane)?),
            Self::Czi(image) => Ok(image.read_region(time, level, channel, x, y, plane)?),
            Self::Readout(image) => Ok(image.read_region(time, level, channel, x, y, plane)?),
        }
    }

    pub(crate) fn stamp(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        z: Range<u32>,
    ) -> Result<Option<u64>, Error> {
        match self {
            Self::Memory(_) => Ok(None),
            Self::Zarr(image) => {
                let extent = self.stored_extent(level);
                Ok(image.stamps(level, time, channel, 0..extent.x(), 0..extent.y(), z)?)
            }
            Self::Tiff(image) => Ok(Some(image.stamp())),
            Self::Czi(_) | Self::Readout(_) => Ok(Some(0)),
        }
    }

    fn stored_extent(&self, level: usize) -> Extent {
        self.pyramid().level(level).extent()
    }
}

fn open_zarr(
    root: grana_zarr::Root,
    location: &Path,
    series: usize,
    group: Option<&str>,
) -> Result<(grana_zarr::Image, Vec<Series>), Error> {
    let plate = grana_zarr::group_kind(&root)? == grana_zarr::GroupKind::Plate;
    if let Some(group) = group {
        let image = grana_zarr::Image::open_root(root.clone(), group)?;
        let listed = if plate {
            vec![Series::of(image.pyramid())]
        } else if let Some(collection) = collection_of_root(&root)? {
            collection_series(&root, &collection, series, &image)?
        } else {
            single_series(image.pyramid(), location, series)?
        };
        return Ok((image, listed));
    }
    if plate {
        return Err(Error::Unsupported {
            path: location.to_path_buf(),
            detail: "the OME-Zarr plate lists its fields of view as series".to_string(),
        });
    }
    let Some(collection) = collection_of_root(&root)? else {
        let image = grana_zarr::Image::open_root(root, "/")?;
        let listed = single_series(image.pyramid(), location, series)?;
        return Ok((image, listed));
    };
    let entry = collection
        .series_at(series)
        .ok_or_else(|| Error::Unsupported {
            path: location.to_path_buf(),
            detail: format!(
                "the collection holds {} series and series {series} was requested",
                collection.series().len()
            ),
        })?;
    let image = grana_zarr::Image::open_root(root.clone(), entry.path())?;
    let listed = collection_series(&root, &collection, series, &image)?;
    Ok((image, listed))
}

fn collection_series(
    root: &grana_zarr::Root,
    collection: &grana_zarr::Collection,
    selected: usize,
    opened: &grana_zarr::Image,
) -> Result<Vec<Series>, Error> {
    let unresolved = collection
        .series()
        .iter()
        .enumerate()
        .filter(|(index, entry)| *index != selected && entry.outline().is_none())
        .map(|(_, entry)| entry.path())
        .collect::<Vec<_>>();
    let mut resolved = grana_zarr::outlines(root, &unresolved)?.into_iter();
    let mut listed = Vec::with_capacity(collection.series().len());
    for (index, entry) in collection.series().iter().enumerate() {
        let name = entry.name().map(str::to_string);
        if index == selected {
            let pyramid = opened.pyramid();
            listed.push(Series::new(
                pyramid.level(0).extent(),
                pyramid.channel_count(),
                pyramid.frame_count(),
                name,
            ));
            continue;
        }
        let outline = match entry.outline() {
            Some(outline) => outline,
            None => resolved
                .next()
                .expect("every unresolved series yields its outline in one batch"),
        };
        listed.push(Series::new(
            outline.extent(),
            outline.channels() as usize,
            outline.frames(),
            name,
        ));
    }
    Ok(listed)
}

fn single_series(pyramid: &Pyramid, path: &Path, series: usize) -> Result<Vec<Series>, Error> {
    if series > 0 {
        return Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: format!("the image holds a single series and series {series} was requested"),
        });
    }
    Ok(vec![Series::of(pyramid)])
}

pub(crate) fn plate_of_root(root: &grana_zarr::Root) -> Result<Option<Plate>, Error> {
    if grana_zarr::group_kind(root)? != grana_zarr::GroupKind::Plate {
        return Ok(None);
    }
    let plate = grana_zarr::open_plate(root)?;
    if plate.is_empty() {
        return Err(Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: "the OME-Zarr plate holds no field of view".to_string(),
        });
    }
    Ok(Some(plate))
}

pub(crate) fn collection_of_root(
    root: &grana_zarr::Root,
) -> Result<Option<grana_zarr::Collection>, Error> {
    Ok(grana_zarr::collection(root)?)
}

fn is_tiff(path: &Path) -> Result<bool, Error> {
    magic(path, 4).map(|magic| {
        magic
            .as_deref()
            .is_some_and(|magic| matches!(magic, b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+"))
    })
}

fn is_czi(path: &Path) -> Result<bool, Error> {
    magic(path, 10).map(|magic| magic.as_deref().is_some_and(|magic| magic == b"ZISRAWFILE"))
}

fn magic(path: &Path, length: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut file = File::open(path).map_err(|error| Error::Open {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut bytes = vec![0u8; length];
    if file.read_exact(&mut bytes).is_err() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn is_zarr(path: &Path) -> bool {
    [".zarray", ".zgroup", "zarr.json"]
        .iter()
        .any(|name| path.join(name).exists())
}

pub fn is_image(path: &Path) -> bool {
    if path.is_dir() {
        return is_zarr(path);
    }
    if let Some(prefix) = prefix(path, 10)
        && recognised(&prefix)
    {
        return true;
    }
    grana_readout::has_extension(path) && grana_readout::is_supported(path)
}

fn recognised(prefix: &[u8]) -> bool {
    prefix.starts_with(b"ZISRAWFILE")
        || matches!(
            prefix.get(..4),
            Some(b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+")
        )
}

fn prefix(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut bytes = vec![0u8; limit];
    let mut filled = 0;
    while filled < limit {
        match file.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(_) => return None,
        }
    }
    bytes.truncate(filled);
    Some(bytes)
}

pub fn series_count(path: &Path) -> Result<usize, Error> {
    if path.is_dir() {
        return series_count_at(&grana_zarr::Root::local(path)?);
    }
    if is_czi(path)? {
        return Ok(grana_czi::scene_count(path)?);
    }
    if grana_readout::is_supported(path) {
        return Ok(grana_readout::scene_count(path)?);
    }
    if is_tiff(path)? {
        return Ok(grana_tiff::series_count(path)?);
    }
    Ok(1)
}

fn series_count_at(root: &grana_zarr::Root) -> Result<usize, Error> {
    if let Some(plate) = plate_of_root(root)? {
        return Ok(plate.len().max(1));
    }
    Ok(collection_of_root(root)?.map_or(1, |collection| collection.series().len().max(1)))
}

pub fn series_names(path: &Path) -> Result<Vec<Option<String>>, Error> {
    if path.is_dir() {
        return series_names_at(&grana_zarr::Root::local(path)?);
    }
    if is_czi(path)? {
        return Ok(grana_czi::scenes(path)?
            .into_iter()
            .map(|scene| scene.name().map(str::to_string))
            .collect());
    }
    if grana_readout::is_supported(path) {
        return Ok(grana_readout::scenes(path)?
            .into_iter()
            .map(|scene| scene.name().map(str::to_string))
            .collect());
    }
    if is_tiff(path)? {
        return Ok(grana_tiff::series_names(path)?);
    }
    Ok(Vec::new())
}

fn series_names_at(root: &grana_zarr::Root) -> Result<Vec<Option<String>>, Error> {
    if let Some(plate) = plate_of_root(root)? {
        return Ok(plate
            .fields()
            .iter()
            .map(|field| Some(field.name().to_string()))
            .collect());
    }
    Ok(collection_of_root(root)?
        .map(|collection| {
            collection
                .series()
                .iter()
                .map(|series| series.name().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

pub fn series_group(path: &Path, series: usize) -> Result<Option<String>, Error> {
    if !path.is_dir() {
        return Ok(None);
    }
    series_group_at(&grana_zarr::Root::local(path)?, series)
}

pub fn remote_series_group(url: &str, series: usize) -> Result<Option<String>, Error> {
    series_group_at(&grana_zarr::Root::remote(url)?, series)
}

fn series_group_at(root: &grana_zarr::Root, series: usize) -> Result<Option<String>, Error> {
    if let Some(plate) = plate_of_root(root)? {
        let field = plate.field(series).ok_or_else(|| Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: format!(
                "the plate holds {} fields of view and field {series} was requested",
                plate.len()
            ),
        })?;
        return Ok(Some(field.path().to_string()));
    }
    let Some(collection) = collection_of_root(root)? else {
        return Ok(None);
    };
    let entry = collection
        .series_at(series)
        .ok_or_else(|| Error::Unsupported {
            path: PathBuf::from(root.display()),
            detail: format!(
                "the collection holds {} series and series {series} was requested",
                collection.series().len()
            ),
        })?;
    Ok(Some(entry.path().to_string()))
}

pub fn plate(source: &Source) -> Result<Option<Plate>, Error> {
    match source {
        Source::Memory(_) => Ok(None),
        Source::File { path, .. } => {
            if !path.is_dir() {
                return Ok(None);
            }
            plate_of_root(&grana_zarr::Root::local(path)?)
        }
        Source::Remote { url, .. } => plate_of_root(&grana_zarr::Root::remote(url)?),
    }
}

pub fn plate_at(path: &Path) -> Result<Option<Plate>, Error> {
    if !path.is_dir() || !is_zarr(path) {
        return Ok(None);
    }
    plate_of_root(&grana_zarr::Root::local(path)?)
}

pub fn is_remote(location: &str) -> bool {
    grana_zarr::Root::remote(location).is_ok()
}

pub fn resolve_series(source: &Source) -> Result<Source, Error> {
    match source {
        Source::Memory(_) => Ok(source.clone()),
        Source::File {
            path,
            series,
            group,
        } => match group {
            Some(_) => Ok(source.clone()),
            None => Ok(match series_group(path, *series)? {
                Some(group) => Source::file_group(path.clone(), *series, group),
                None => source.clone(),
            }),
        },
        Source::Remote { url, series, group } => match group {
            Some(_) => Ok(source.clone()),
            None => Ok(match remote_series_group(url, *series)? {
                Some(group) => Source::remote_group(url.clone(), *series, group),
                None => source.clone(),
            }),
        },
    }
}

pub fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "dataset".to_string();
    };
    if let Some(stem) = name.strip_suffix(".zarr") {
        return stem.to_string();
    }
    if name.to_ascii_lowercase().ends_with(".czi") {
        return grana_czi::dataset_name(Path::new(name));
    }
    if grana_readout::has_extension(Path::new(name)) {
        return grana_readout::dataset_name(Path::new(name));
    }
    grana_tiff::dataset_name(Path::new(name))
}
