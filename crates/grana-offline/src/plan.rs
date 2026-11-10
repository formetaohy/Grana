use std::path::{Path, PathBuf};

use grana_display::{
    Camera, CameraState, PANE_COUNT, PaneKind, PaneView, SectionCamera, SectionState,
};
use grana_render::{Lens, PaneFrame};
use grana_volume::{Pyramid, Region};

const WIDE_BYTES: u64 = 16;
const WIDE_CEILING: u64 = 1 << 30;

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub time: u32,
    pub views: [PaneView; PANE_COUNT],
}

pub enum Destination {
    Image { path: PathBuf },
    Movie { path: PathBuf },
    Sequence { directory: PathBuf, stem: String },
}

pub struct Plan {
    pub destination: Destination,
    pub panels: grana_engine::Panels,
    pub width: u32,
    pub transparent: bool,
    pub poses: Vec<Pose>,
    pub fps: u32,
    pub region: Region,
}

pub(crate) struct Residency {
    pub(crate) slots: usize,
    pub(crate) level: usize,
}

pub(crate) fn validate(plan: &Plan) -> Result<(), String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    if let Destination::Image { path } = &plan.destination
        && still(path)? == Still::Tiff
    {
        let size = plan.panels.export(plan.width, false).size;
        let bytes = u64::from(size[0]) * u64::from(size[1]) * WIDE_BYTES;
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

pub(crate) fn residency(plan: &Plan, pyramid: &Pyramid) -> Result<Residency, String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    let movie = matches!(plan.destination, Destination::Movie { .. });
    let regions = plan.panels.export(plan.width, movie);
    let mut slots = 1usize;
    let mut level = pyramid.level_count() - 1;
    for pose in &plan.poses {
        let cameras = pose.views.map(|view| camera(&view.camera));
        let sections = pose.views.map(|view| section(&view.section));
        let panes = std::array::from_fn(|index| {
            let pane = regions.panes[index]?;
            let aspect = pane.region[2].max(1) as f32 / pane.region[3].max(1) as f32;
            Some(match pane.kind {
                PaneKind::Volume => PaneFrame::Volume {
                    camera: &cameras[index],
                    lens: Lens {
                        camera: &cameras[index],
                        aspect,
                    },
                    region: pane.region,
                },
                PaneKind::Section => PaneFrame::Section {
                    section: &sections[index],
                    region: pane.region,
                },
            })
        });
        let held = grana_render::residency(pyramid, &panes, plan.region);
        slots = slots.max(held.slots);
        for detail in held.panes.iter().flatten() {
            level = level.min(detail.level);
        }
    }
    Ok(Residency { slots, level })
}

pub(crate) fn detail_lens(
    poses: &[Pose],
    regions: &grana_engine::Regions,
) -> Option<(Camera, u32)> {
    let mut best: Option<(Camera, u32, f32)> = None;
    for pose in poses {
        for (index, pane) in regions.panes.iter().enumerate() {
            let Some(pane) = pane else {
                continue;
            };
            if pane.kind != PaneKind::Volume {
                continue;
            }
            let height = pane.region[3].max(1);
            let camera = camera(&pose.views[index].camera);
            let scale = camera.pixels_per_world(height);
            if best.as_ref().is_none_or(|(_, _, held)| scale > *held) {
                best = Some((camera, height, scale));
            }
        }
    }
    best.map(|(camera, height, _)| (camera, height))
}

pub(crate) fn camera(state: &CameraState) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.restore(*state);
    camera
}

pub(crate) fn section(state: &SectionState) -> SectionCamera {
    let mut section = SectionCamera::new();
    section.restore(*state);
    section
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
