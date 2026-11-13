use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use grana_object::{LabelRaster, Shape};
use grana_scene::{ObjectFrame, ObjectSet};
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
    pub name: String,
    pub frames: Vec<u32>,
    pub labels: usize,
    pub color: [f32; 3],
    pub opacity: f32,
    pub properties: Vec<(u32, String)>,
    pub shapes: Vec<Option<Arc<ObjectFrame>>>,
    pub visible: Vec<Vec<bool>>,
}

impl LabelLayer {
    pub fn of(set: &ObjectSet) -> Option<Self> {
        let frames = set.frames().map(|(time, _)| time).collect::<Vec<_>>();
        let labels = frames
            .iter()
            .filter_map(|time| set.frame(*time).map(ObjectFrame::object_count))
            .max()
            .unwrap_or(0);
        if labels == 0 || labels > LABEL_LIMIT {
            return None;
        }
        let shapes = (0..set.frame_count())
            .map(|time| set.frame_shared(time))
            .collect::<Vec<_>>();
        let visible = (0..set.frame_count())
            .map(|time| {
                set.frame(time).map_or_else(Vec::new, |frame| {
                    (0..frame.object_count())
                        .map(|index| set.visible_object(time, index))
                        .collect()
                })
            })
            .collect::<Vec<_>>();
        let title = set.name().to_string();
        let properties = (1..=labels)
            .map(|label| (label as u32, format!("{title} {label}")))
            .collect();
        Some(Self {
            name: grana_scene::slug(set.name()),
            frames,
            labels,
            color: set.color(),
            opacity: set.opacity(),
            properties,
            shapes,
            visible,
        })
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
    for layer in labels {
        let LabelLayer {
            name: label_name,
            frames: label_frames,
            labels: count,
            color,
            opacity,
            properties,
            shapes,
            visible,
        } = layer;
        let mut cached: Option<(u32, Shape)> = None;
        let mut fill =
            |level: usize, time: u32, origin: [u32; 3], extent: [u32; 3], data: &mut [u16]| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("the dataset export was cancelled".to_string());
                }
                if cached.as_ref().map(|(cached, _)| *cached) != Some(time) {
                    let frame = shapes
                        .get(time as usize)
                        .and_then(|frame| frame.as_ref())
                        .ok_or_else(|| {
                            format!("the object set holds no objects at frame {time}")
                        })?;
                    let shape = frame.shape().map_err(|error| error.to_string())?;
                    if shape.objects() > LABEL_LIMIT {
                        return Err(format!(
                            "a uint16 label image addresses at most {LABEL_LIMIT} objects"
                        ));
                    }
                    cached = Some((time, shape));
                }
                let shape = &cached.as_ref().expect("the frame shape is cached").1;
                let level = levels.get(level).copied().ok_or_else(|| {
                    "the label image names a level the dataset does not hold".to_string()
                })?;
                let raster = LabelRaster::new(shape, level);
                let keep = visible.get(time as usize).map(Vec::as_slice);
                let rendered = raster.render_visible(
                    keep,
                    origin[0]..origin[0] + extent[0],
                    origin[1]..origin[1] + extent[1],
                    origin[2]..origin[2] + extent[2],
                );
                data.copy_from_slice(&rendered);
                Ok(())
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
                name: &label_name,
                frames: &label_frames,
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
