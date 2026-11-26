use std::path::{Path, PathBuf};

use grana_display::{Camera, CameraState, OutputSize};
use grana_volume::{Pyramid, Region};

const WIDE_CEILING: u64 = 1 << 30;

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub time: u32,
    pub camera: CameraState,
}

pub enum Destination {
    Image {
        path: PathBuf,
    },
    Figure {
        path: PathBuf,
    },
    Lineage {
        path: PathBuf,
        set: grana_scene::ObjectSetId,
    },
    Chart {
        path: PathBuf,
        page: grana_chart::Page,
    },
    Movie {
        path: PathBuf,
    },
    Sequence {
        directory: PathBuf,
        stem: String,
    },
}

pub fn poses(timeline: &grana_animation::Timeline) -> Vec<Pose> {
    (0..timeline.frames())
        .filter_map(|frame| timeline.sample_frame(frame))
        .map(|sample| Pose {
            time: sample.time,
            camera: sample.camera,
        })
        .collect()
}

pub struct Plan {
    pub destination: Destination,
    pub size: OutputSize,
    pub transparent: bool,
    pub poses: Vec<Pose>,
    pub fps: u32,
    pub region: Region,
}

pub(crate) struct Needed {
    pub detail: grana_render::Detail,
    pub levels: Vec<usize>,
}

pub(crate) fn residency(plan: &Plan, pyramid: &Pyramid) -> Result<Needed, String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    let movie = matches!(plan.destination, Destination::Movie { .. });
    assert!(
        !movie || plan.size.height().is_multiple_of(2) && plan.size.width().is_multiple_of(2),
        "a movie frame holds an even number of pixels"
    );
    let size = plan.size.to_array();
    let mut detail = grana_render::Detail {
        level: pyramid.level_count() - 1,
        bricks: 0,
    };
    let mut levels = Vec::with_capacity(plan.poses.len());
    for pose in &plan.poses {
        let camera = camera(&pose.camera);
        let held = grana_render::residency(pyramid, &camera, size, plan.region);
        detail.bricks = detail.bricks.max(held.detail.bricks);
        detail.level = detail.level.min(held.detail.level);
        levels.push(held.detail.level);
    }
    Ok(Needed { detail, levels })
}

pub(crate) fn validate(plan: &Plan) -> Result<(), String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    if matches!(plan.destination, Destination::Image { .. }) && plan.poses.len() != 1 {
        return Err("an image export renders exactly one frame".to_string());
    }
    if matches!(plan.destination, Destination::Lineage { .. }) {
        if plan.poses.len() != 1 {
            return Err("a lineage page highlights exactly one frame".to_string());
        }
        if plan.transparent {
            return Err("a lineage page paints its own backdrop".to_string());
        }
    }
    if let Destination::Chart { path, .. } = &plan.destination {
        if plan.poses.len() != 1 {
            return Err("a statistics chart names exactly one frame".to_string());
        }
        if plan.transparent {
            return Err("a statistics chart paints its own backdrop".to_string());
        }
        if grana_page::Format::of(path).is_none() {
            return Err(grana_page::Format::error(path, "a statistics chart"));
        }
    }
    if let Destination::Movie { path } = &plan.destination {
        let format = grana_video::Format::required(path, plan.transparent)
            .map_err(|error| error.to_string())?;
        let expected = format.planned_bytes(plan.size.to_array(), plan.fps, plan.poses.len());
        grana_file::require(path, expected).map_err(|error| error.to_string())?;
    }
    let wide = match &plan.destination {
        Destination::Image { path } => still(path)? == Still::Tiff,
        Destination::Figure { .. } => true,
        Destination::Lineage { .. }
        | Destination::Chart { .. }
        | Destination::Movie { .. }
        | Destination::Sequence { .. } => false,
    };
    if wide {
        let size = plan.size.to_array();
        let bytes = grana_render::picture_bytes(size);
        if bytes > WIDE_CEILING {
            return Err(format!(
                "a sixteen bit still at {} × {} needs more than {} GiB of texture memory; lower the export width",
                size[0],
                size[1],
                WIDE_CEILING >> 30
            ));
        }
    }
    Ok(())
}

pub(crate) fn detail_lens(poses: &[Pose], height: u32) -> Option<(Camera, u32)> {
    let mut best: Option<(Camera, u32, f32)> = None;
    for pose in poses {
        let height = height.max(1);
        let camera = camera(&pose.camera);
        let scale = camera.pixels_per_world(height);
        if best.as_ref().is_none_or(|(_, _, held)| scale > *held) {
            best = Some((camera, height, scale));
        }
    }
    best.map(|(camera, height, _)| (camera, height))
}

pub(crate) fn camera(state: &CameraState) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.restore(*state);
    camera
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("the export")
        .to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Still {
    Png,
    Tiff,
}

pub(crate) fn still(path: &Path) -> Result<Still, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => Ok(Still::Png),
        Some("tif" | "tiff") => Ok(Still::Tiff),
        other => Err(format!(
            "a still image is saved as PNG or OME-TIFF, not `{}`",
            other.unwrap_or("a nameless file")
        )),
    }
}
