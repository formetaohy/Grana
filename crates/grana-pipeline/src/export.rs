use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_scene::ObjectSet;
use grana_volume::{Extent, Intensity, Level, Region};
use grana_warp::Warps;
use grana_zarr::{ImageSpec, LabelSpec, WriteChannel};

use crate::channels::Channels;

const LABEL_LIMIT: usize = u16::MAX as usize;

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
    pub range: Intensity,
}

pub struct LabelLayer {
    name: String,
    frames: Vec<u32>,
    color: [f32; 3],
    opacity: f32,
    labeling: crate::label::Labeling,
}

impl LabelLayer {
    pub fn of(set: &ObjectSet, levels: &[Level]) -> Option<Self> {
        let frames = (0..set.frame_count())
            .filter(|time| set.frame_stamp(*time).is_some())
            .collect::<Vec<_>>();
        let labels = frames
            .iter()
            .map(|time| set.object_count(*time))
            .max()
            .unwrap_or(0);
        if labels == 0 || labels > LABEL_LIMIT {
            return None;
        }
        if !(0..set.frame_count())
            .any(|time| (0..set.object_count(time)).any(|index| set.visible_object(time, index)))
        {
            return None;
        }
        Some(Self {
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
        })
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
        &mut self,
        level: usize,
        time: u32,
        origin: [u32; 3],
        extent: [u32; 3],
        data: &mut [u16],
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
    pub drift: Vec<Warps>,
    pub labels: Vec<LabelLayer>,
}

pub struct Summary {
    path: PathBuf,
    channels: usize,
    labels: usize,
    frames: u32,
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
    let write_channels = layers
        .iter()
        .map(|layer| WriteChannel {
            name: &layer.name,
            tint: layer.tint,
            range: layer.range,
        })
        .collect::<Vec<_>>();
    let mut fill = |level: usize,
                    time: u32,
                    channel: usize,
                    origin: [u32; 3],
                    extent: [u32; 3],
                    data: &mut [f32]| {
        if cancel.load(Ordering::Relaxed) {
            return Err("the dataset export was cancelled".to_string());
        }
        let index = layers
            .get(channel)
            .map(|layer| layer.index)
            .ok_or_else(|| format!("the export names no channel {channel}"))?;
        let base = origins
            .get(level)
            .ok_or_else(|| format!("the export names no resolution level {level}"))?;
        let start = [
            origin[0] + base[0],
            origin[1] + base[1],
            origin[2] + base[2],
        ];
        let ranges = [
            start[0]..start[0] + extent[0],
            start[1]..start[1] + extent[1],
            start[2]..start[2] + extent[2],
        ];
        let warp = drift
            .get(time as usize)
            .map_or(&grana_warp::Warp::IDENTITY, |warps| warps.get(index));
        let values = channels
            .read_region(time, level, index, &ranges, warp)
            .map_err(|error| error.to_string())?;
        values.write_values(data);
        Ok(())
    };
    let mut image_total = 0usize;
    let mut image_progress = |done: usize, total: usize| {
        image_total = total;
        progress(done, total);
    };
    grana_zarr::write_image(
        &path,
        ImageSpec {
            name: &name,
            channels: &write_channels,
            levels: &levels,
            placement,
            frames,
            interval,
            fill: &mut fill,
            progress: &mut image_progress,
        },
    )
    .map_err(|error| error.to_string())?;
    let mut base = image_total;
    let mut written = 0usize;
    for mut layer in labels {
        let name = layer.name().to_string();
        let frames = layer.frames().to_vec();
        let count = layer.labels();
        let color = layer.color();
        let opacity = layer.opacity();
        let properties = layer.properties();
        let mut fill =
            |level: usize, time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
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
        grana_zarr::write_group_labels(
            &path,
            "/",
            LabelSpec {
                name: &name,
                frames: &frames,
                labels: count,
                color,
                opacity,
                properties: &properties,
                fill: &mut fill,
                progress: &mut label_progress,
            },
        )
        .map_err(|error| error.to_string())?;
        base += total;
        written += 1;
    }
    Ok(Summary {
        path,
        channels: layers.len(),
        labels: written,
        frames,
        cropped: origins.iter().any(|origin| *origin != [0, 0, 0]),
    })
}
