use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_archive::{Block, Channel as WriteChannel, Fill, Format, ImageSpec};
use grana_scene::ObjectSet;
use grana_volume::{Calibration, Extent, Intensity, Level, Region, VoxelData, VoxelFormat};
use grana_warp::Warps;

use crate::channels::Channels;

const LABEL_LIMIT: usize = u32::MAX as usize;

pub struct Crop {
    pub levels: Vec<Level>,
    pub origins: Vec<[u32; 3]>,
}

pub fn cropped_levels(region: Region, levels: &[Level]) -> Result<Crop, String> {
    let mut cropped = Vec::with_capacity(levels.len());
    let mut origins = Vec::with_capacity(levels.len());
    for (index, level) in levels.iter().enumerate() {
        let ranges = region.voxel_range(*level).ok_or_else(|| {
            format!("the region does not overlap resolution level {index} of the dataset")
        })?;
        let start = [ranges[0].start, ranges[1].start, ranges[2].start];
        let extent = Extent::new(
            ranges[0].end - ranges[0].start,
            ranges[1].end - ranges[1].start,
            ranges[2].end - ranges[2].start,
        );
        let origin = (Vec3::from(level.origin())
            + level.basis().matrix()
                * Vec3::new(start[0] as f32, start[1] as f32, start[2] as f32))
        .to_array();
        cropped.push(Level::placed(extent, level.basis(), origin));
        origins.push(start);
    }
    Ok(Crop {
        levels: cropped,
        origins,
    })
}

pub struct ChannelLayer {
    pub index: usize,
    pub name: String,
    pub tint: [f32; 3],
    pub data: Intensity,
    pub window: Intensity,
    pub color_table: Option<grana_volume::ColorTable>,
}

pub struct LabelLayer {
    name: String,
    frames: Vec<u32>,
    color: [f32; 3],
    opacity: f32,
    labeling: crate::label::Labeling,
}

impl LabelLayer {
    pub fn of(set: &ObjectSet, levels: &[Level]) -> Result<Option<Self>, String> {
        let frames = (0..set.frame_count())
            .filter(|time| set.frame_stamp(*time).is_some())
            .collect::<Vec<_>>();
        let labels = frames
            .iter()
            .map(|time| set.object_count(*time))
            .max()
            .unwrap_or(0);
        if labels == 0 {
            return Ok(None);
        }
        if labels > LABEL_LIMIT {
            return Err(format!(
                "a label image addresses at most {LABEL_LIMIT} objects, and {} holds {labels}",
                set.name()
            ));
        }
        if !(0..set.frame_count())
            .any(|time| (0..set.object_count(time)).any(|index| set.visible_object(time, index)))
        {
            return Ok(None);
        }
        Ok(Some(Self {
            name: grana_scene::slug(set.name()),
            frames,
            color: set.color(),
            opacity: set.opacity(),
            labeling: crate::label::Labeling::new(
                set.source(),
                labels,
                levels.to_vec(),
                set.name().to_string(),
            ),
        }))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn frames(&self) -> &[u32] {
        &self.frames
    }

    pub fn labels(&self) -> usize {
        self.labeling.labels()
    }

    pub fn color(&self) -> [f32; 3] {
        self.color
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn properties(&self) -> Vec<(u32, String)> {
        self.labeling.properties()
    }

    pub fn fill(
        &self,
        level: usize,
        time: u32,
        origin: [u32; 3],
        extent: [u32; 3],
        data: &mut [u32],
    ) -> Result<(), String> {
        self.labeling.fill(level, time, origin, extent, data)
    }
}

pub struct DatasetSpec {
    pub path: PathBuf,
    pub name: String,
    pub channels: Vec<ChannelLayer>,
    pub levels: Vec<Level>,
    pub origins: Vec<[u32; 3]>,
    pub placement: [f32; 3],
    pub frames: u32,
    pub interval: f32,
    pub calibration: Calibration,
    pub drift: Vec<Warps>,
    pub labels: Vec<LabelLayer>,
}

pub struct Summary {
    path: PathBuf,
    channels: usize,
    labels: usize,
    frames: u32,
    levels: usize,
    cropped: bool,
}

impl Summary {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn labels(&self) -> usize {
        self.labels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn levels(&self) -> usize {
        self.levels
    }

    pub fn cropped(&self) -> bool {
        self.cropped
    }
}

pub fn write_dataset(
    spec: DatasetSpec,
    channels: &Channels,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Summary, String> {
    let DatasetSpec {
        path,
        name,
        channels: layers,
        levels,
        origins,
        placement,
        frames,
        interval,
        calibration,
        drift,
        labels,
    } = spec;
    if layers.is_empty() {
        return Err("the dataset holds no channel to export".to_string());
    }
    if levels.is_empty() {
        return Err("the dataset holds no stored resolution level".to_string());
    }
    assert_eq!(
        origins.len(),
        levels.len(),
        "every exported level names the voxel its crop starts at"
    );
    let format = Format::of(&path).ok_or_else(|| {
        format!(
            "an archive path ends with .ome.zarr or .ome.tif, not {}",
            path.display()
        )
    })?;
    if format == Format::Tiff && !labels.is_empty() {
        return Err(format!(
            "an OME-TIFF archive carries no object label layer, and this dataset holds {}; export it as OME-Zarr to keep them",
            labels.len()
        ));
    }
    let (levels, origins) = match format {
        Format::Zarr => (levels, origins),
        Format::Tiff => {
            let depth = levels[0].extent().z();
            if let Some(index) = levels.iter().position(|level| level.extent().z() != depth) {
                return Err(format!(
                    "resolution level {index} stores {} planes where level 0 stores {depth}, and an OME-TIFF keeps one depth for every level; export it as OME-Zarr to keep the whole pyramid",
                    levels[index].extent().z()
                ));
            }
            (levels, origins)
        }
    };
    let sample = sample_of(channels, &layers)?;
    let write_channels = layers
        .iter()
        .map(|layer| WriteChannel {
            name: &layer.name,
            tint: layer.tint,
            data: layer.data,
            window: layer.window,
            color_table: layer
                .color_table
                .as_ref()
                .filter(|_| sample == VoxelFormat::U8),
        })
        .collect::<Vec<_>>();
    let fill = |block: Block| -> Result<VoxelData, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("the dataset export was cancelled".to_string());
        }
        let index = layers
            .get(block.channel)
            .map(|layer| layer.index)
            .ok_or_else(|| format!("the export names no channel {}", block.channel))?;
        let base = origins
            .get(block.level)
            .ok_or_else(|| format!("the export names no resolution level {}", block.level))?;
        let start = [
            block.origin[0] + base[0],
            block.origin[1] + base[1],
            block.origin[2] + base[2],
        ];
        let ranges = [
            start[0]..start[0] + block.extent[0],
            start[1]..start[1] + block.extent[1],
            start[2]..start[2] + block.extent[2],
        ];
        let warp = drift
            .get(block.frame as usize)
            .map_or(&grana_warp::Warp::IDENTITY, |warps| warps.get(index));
        channels
            .read_region(block.frame, block.level, index, &ranges, warp)
            .map_err(|error| error.to_string())
    };
    let mut image_total = 0usize;
    let mut image_progress = |done: usize, total: usize| {
        image_total = total;
        progress(done, total);
    };
    let mut image = ImageSpec {
        name: &name,
        channels: &write_channels,
        levels: &levels,
        placement,
        frames,
        interval: Some(interval),
        sample,
        calibration,
        fill: Fill::EveryLevel(&fill),
        cancel,
        progress: &mut image_progress,
    };
    let mut written = 0usize;
    match format {
        Format::Zarr => {
            let expected = grana_zarr::image_space(&levels, frames, layers.len(), sample)
                + labels
                    .iter()
                    .map(|layer| {
                        grana_zarr::label_space(&levels, layer.frames().len(), layer.labels())
                    })
                    .sum::<u64>();
            let mut archive =
                grana_zarr::Archive::create(&path, expected).map_err(|error| error.to_string())?;
            archive
                .image(&mut image)
                .map_err(|error| error.to_string())?;
            let mut base = image_total;
            for layer in labels {
                let label_name = layer.name().to_string();
                let frames = layer.frames().to_vec();
                let count = layer.labels();
                let color = layer.color();
                let opacity = layer.opacity();
                let properties = layer.properties();
                let fill = |level: usize,
                            time: u32,
                            origin: [u32; 3],
                            extent: [u32; 3],
                            data: &mut [u32]| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("the dataset export was cancelled".to_string());
                    }
                    layer.fill(level, time, origin, extent, data)
                };
                let mut total = 0usize;
                let mut label_progress = |done: usize, total_seen: usize| {
                    total = total_seen;
                    progress(base + done, base + total_seen);
                };
                archive
                    .labels(
                        "/",
                        grana_zarr::LabelSpec {
                            name: &label_name,
                            frames: &frames,
                            labels: count,
                            color,
                            opacity,
                            properties: &properties,
                            fill: &fill,
                            cancel,
                            progress: &mut label_progress,
                        },
                    )
                    .map_err(|error| error.to_string())?;
                base += total;
                written += 1;
            }
            archive.commit().map_err(|error| error.to_string())?;
        }
        Format::Tiff => {
            grana_tiff::write_archive(&path, &mut image).map_err(|error| error.to_string())?;
        }
    }
    Ok(Summary {
        path,
        channels: layers.len(),
        labels: written,
        frames,
        levels: levels.len(),
        cropped: origins.iter().any(|origin| *origin != [0, 0, 0]),
    })
}

fn sample_of(channels: &Channels, layers: &[ChannelLayer]) -> Result<VoxelFormat, String> {
    let mut sample: Option<VoxelFormat> = None;
    for layer in layers {
        let format = channels
            .pyramid()
            .channels()
            .get(layer.index)
            .ok_or_else(|| format!("the export names no channel {}", layer.index))?
            .format();
        sample = Some(match sample {
            None => format,
            Some(held) => held.union(format).ok_or_else(|| {
                format!(
                    "the dataset stores the voxel types {held:?} and {format:?}, and one archive image holds a single type"
                )
            })?,
        });
    }
    sample.ok_or_else(|| "the dataset holds no channel to export".to_string())
}
