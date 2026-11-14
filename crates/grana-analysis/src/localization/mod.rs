use std::cell::RefCell;

use glam::Vec3;
pub use grana_object::{ClusterSpec, LocalizationFilter, LocalizationSpec, SpotWeight};
use grana_volume::Region;

mod cluster;
mod composition;
mod depth;
mod fit;
mod fourier;
mod frame;
mod frc;
mod grid;
mod group;
mod image;
mod layout;
mod nena;
mod report;
mod sample;
mod source;
mod stack;
mod survey;
mod tone;

pub use cluster::{Clusters, cluster};
pub use composition::{CompositionEntry, CompositionSpec, LocalizationComposition};
pub use depth::LocalizationDepth;
pub use frame::LocalizationFrame;
use grid::Grid;
pub use group::{GroupStat, Groups};
pub use image::LocalizationRenderer;
pub use layout::LocalizationLayout;
pub use report::{Frc, LocalizationGeometry, LocalizationImage, LocalizationReport, NeNa};
pub use source::LocalizationSource;
pub use stack::LocalizationStack;
pub use survey::LocalizationSurvey;

const PREVIEW: u32 = 2048;

const PREVIEW_BYTES: u64 = 256 << 20;

const FRC_LIMIT: usize = 2048;

const FRC_MINIMUM: usize = 64;

const REACH: f32 = 1.0;

const MARGIN: f32 = 3.0;

const FLOOR: f32 = 1.0e-6;

const UNGROUPED: u32 = u32::MAX;

const FRAME_PASSES: usize = 5;

const NANOMETRES_PER_MICROMETRE: f32 = 1000.0;

#[derive(Clone, Copy)]
pub(super) struct Point {
    center: [f32; 2],
    group: u32,
}

pub fn localization_report(
    source: &dyn LocalizationSource,
    spec: LocalizationSpec,
    camera_pixel: f32,
    grouped: bool,
    region: Option<Region>,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<LocalizationReport>, String> {
    assert!(
        camera_pixel.is_finite() && camera_pixel > 0.0,
        "a localization report knows its camera pixel"
    );
    if source.is_empty() {
        return Err("the frame range holds no frame".to_string());
    }
    let filter = spec.filter();
    let ticker = RefCell::new(Ticker::new(frame_steps(source.len()), report));
    let tick = |_: usize, _: usize| ticker.borrow_mut().tick();
    let Some(layout) = LocalizationLayout::new(source, spec, camera_pixel, &tick)? else {
        if ticker.borrow().cancelled() {
            return Ok(None);
        }
        return Err("the frame range holds no visible localization".to_string());
    };
    let survey = layout.survey();
    let low = layout.low();
    let span = layout.span();
    let blur = layout.blur();
    let depth = layout.depth();
    let geometry = layout.geometry();
    let weight = layout.weight();
    let preview = preview_geometry(low, span, spec.pixel() * 0.001, blur, depth)?;
    let preview_geometry = LocalizationGeometry::new(
        preview.origin(),
        preview.pixel(),
        preview.size(),
        blur,
        margin_pixels(blur, preview.pixel()),
    );
    ticker
        .borrow_mut()
        .retotal(total_steps(source.len(), depth, preview.size()));
    let (density, stack) = {
        let mut renderer = LocalizationRenderer::new(
            source,
            filter,
            preview_geometry,
            depth,
            layout.groups().clone(),
            weight,
            &tick,
        )?;
        let density = renderer.survey()?;
        let size = preview.size();
        let mut layers = Vec::with_capacity(depth.layers() as usize);
        for layer in 0..depth.layers() {
            renderer.begin(layer)?;
            let mut pixels = vec![0u8; size[0] as usize * size[1] as usize * 4];
            let stride = size[0] as usize * 4;
            for row in 0..size[1] as usize {
                renderer.row(&mut pixels[row * stride..(row + 1) * stride])?;
            }
            layers.push(LocalizationImage::new(size, pixels));
        }
        (
            density,
            LocalizationStack::new(preview_geometry, depth, layers),
        )
    };
    let nena = if source.len() > 1 {
        let mut progress = |_: usize, _: usize| ticker.borrow_mut().tick();
        match nena::measure(
            source,
            filter,
            REACH * camera_pixel,
            survey.sigma() * NANOMETRES_PER_MICROMETRE,
            grouped,
            &mut progress,
        )? {
            Some(nena) => Some(nena),
            None => return Ok(None),
        }
    } else {
        None
    };
    let frc = {
        let mut advance = || ticker.borrow_mut().tick();
        match measure_frc(&layout, source, filter, region, &mut advance)? {
            Some(frc) => frc,
            None => return Ok(None),
        }
    };
    if !ticker.borrow_mut().tick() {
        return Ok(None);
    }
    let precision = nena
        .as_ref()
        .map_or(survey.sigma(), |nena| nena.precision() * 0.001);
    let suggested = cluster::estimate(source, filter, precision, survey.axes());
    if !ticker.borrow_mut().tick() {
        return Ok(None);
    }
    Ok(Some(LocalizationReport {
        spec,
        filter,
        region,
        pixel: geometry.pixel() * 1000.0,
        origin: geometry.origin(),
        extent: [0, 1].map(|axis| geometry.size()[axis] as f32 * geometry.pixel()),
        localizations: layout.localizations(),
        geometry,
        density,
        nena,
        frc,
        suggested,
        groups: layout.groups().clone(),
        stack,
    }))
}

struct Ticker<'a> {
    done: usize,
    total: usize,
    report: &'a mut dyn FnMut(usize, usize) -> bool,
    cancelled: bool,
}

impl Ticker<'_> {
    fn new(total: usize, report: &mut dyn FnMut(usize, usize) -> bool) -> Ticker<'_> {
        Ticker {
            done: 0,
            total,
            report,
            cancelled: false,
        }
    }

    fn retotal(&mut self, total: usize) {
        self.total = total.max(self.done);
    }

    fn cancelled(&self) -> bool {
        self.cancelled
    }

    fn tick(&mut self) -> bool {
        self.done += 1;
        if !(self.report)(self.done, self.total) {
            self.cancelled = true;
        }
        !self.cancelled
    }
}

fn frame_steps(frames: usize) -> usize {
    frames
        * if frames > 1 {
            FRAME_PASSES
        } else {
            FRAME_PASSES - 1
        }
}

fn total_steps(frames: usize, depth: LocalizationDepth, size: [u32; 2]) -> usize {
    let bands = (size[1] as usize).div_ceil(512);
    frame_steps(frames) + 2 * depth.layers() as usize + bands * depth.layers() as usize + 2
}

struct Preview {
    origin: [f32; 2],
    pixel: f32,
    size: [u32; 2],
}

impl Preview {
    fn origin(&self) -> [f32; 2] {
        self.origin
    }

    fn pixel(&self) -> f32 {
        self.pixel
    }

    fn size(&self) -> [u32; 2] {
        self.size
    }
}

fn rasterize(
    spec: &LocalizationSpec,
    low: [f32; 2],
    span: [f32; 2],
    pixel: f32,
    blur: f32,
) -> Result<LocalizationGeometry, String> {
    let border = margin_pixels(blur, pixel);
    let size = raster_size(span, pixel, border)?;
    if size[0] > LocalizationGeometry::MAXIMUM || size[1] > LocalizationGeometry::MAXIMUM {
        return Err(format!(
            "the localization image would span {}x{} pixels; raise the pixel size of the {} nm render",
            size[0],
            size[1],
            spec.pixel()
        ));
    }
    Ok(LocalizationGeometry::new(
        [
            low[0] - border as f32 * pixel,
            low[1] - border as f32 * pixel,
        ],
        pixel,
        size,
        blur,
        border,
    ))
}

fn preview_geometry(
    low: [f32; 2],
    span: [f32; 2],
    pixel: f32,
    blur: f32,
    depth: LocalizationDepth,
) -> Result<Preview, String> {
    let budget = PREVIEW_BYTES / 4;
    let mut scale = ((span[0].max(span[1]) / pixel).ceil() / PREVIEW as f32)
        .ceil()
        .max(1.0);
    loop {
        let scaled = pixel * scale;
        let border = margin_pixels(blur, scaled);
        let size = raster_size(span, scaled, border)?;
        let pixels = u64::from(size[0].max(1)) * u64::from(size[1].max(1));
        if size[0] <= PREVIEW && size[1] <= PREVIEW && pixels * u64::from(depth.layers()) <= budget
        {
            return Ok(Preview {
                origin: [
                    low[0] - border as f32 * scaled,
                    low[1] - border as f32 * scaled,
                ],
                pixel: scaled,
                size: [size[0].max(1), size[1].max(1)],
            });
        }
        scale *= 2.0;
        if !scale.is_finite() {
            return Err("the localization image cannot be bounded to a preview".to_string());
        }
    }
}

fn raster_size(span: [f32; 2], pixel: f32, border: u32) -> Result<[u32; 2], String> {
    let sides = 2 * u64::from(border) + 1;
    let Some(room) = u64::from(u32::MAX).checked_sub(sides) else {
        return Err("the localization image blurs past the pixels of a raster".to_string());
    };
    let mut size = [0u32; 2];
    for axis in 0..2 {
        let cells = (f64::from(span[axis]) / f64::from(pixel)).ceil();
        if !(cells.is_finite() && cells <= room as f64) {
            return Err("the localization image spans more pixels than a raster holds".to_string());
        }
        size[axis] = cells as u32 + sides as u32;
    }
    Ok(size)
}

fn margin_pixels(blur: f32, pixel: f32) -> u32 {
    if blur > 0.0 {
        (MARGIN * blur / pixel).ceil().max(1.0) as u32
    } else {
        1
    }
}

fn measure_frc(
    layout: &LocalizationLayout,
    source: &dyn LocalizationSource,
    filter: LocalizationFilter,
    region: Option<Region>,
    report: &mut dyn FnMut() -> bool,
) -> Result<Option<Frc>, String> {
    let origin = layout.low();
    let span = layout.span();
    let sigma = layout.survey().sigma();
    let weight = layout.weight();
    let (side, corner) = match region {
        Some(region) => {
            let (low, high) = region.bounds();
            let size = high - low;
            (size.x.min(size.y), [low.x, low.y])
        }
        None => {
            let side = span[0].min(span[1]);
            (
                side,
                [
                    origin[0] + 0.5 * (span[0] - side),
                    origin[1] + 0.5 * (span[1] - side),
                ],
            )
        }
    };
    if !(side.is_finite() && side > 0.0) {
        return Err("the region of interest spans no square to correlate".to_string());
    }
    let preferred = 0.5 * sigma;
    let grid = ((side / preferred).ceil() as usize)
        .next_power_of_two()
        .clamp(FRC_MINIMUM, FRC_LIMIT);
    let pixel = side / grid as f32;
    let mut first = Grid::empty([grid as u32; 2], pixel, corner);
    let mut second = Grid::empty([grid as u32; 2], pixel, corner);
    let mut index = 0u64;
    let mut kept = 0usize;
    for frame_index in 0..source.len() {
        let frame = source.frame(frame_index, filter);
        for (spot_index, spot) in frame.spots().iter().enumerate() {
            if !frame.visible(spot_index) {
                continue;
            }
            let center = spot.center();
            if region.is_some_and(|region| !region.contains(Vec3::from(center))) {
                continue;
            }
            kept += 1;
            let value = weight.of(spot);
            if split(index) {
                first.add([center[0], center[1]], value, None);
            } else {
                second.add([center[0], center[1]], value, None);
            }
            index += 1;
        }
        if !report() {
            return Ok(None);
        }
    }
    if kept == 0 {
        return Err("the region of interest holds no visible localization".to_string());
    }
    Ok(Some(frc::measure(&first, &second, region.is_some())))
}

fn split(index: u64) -> bool {
    let mut value = index.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (value ^ (value >> 31)) & 1 == 1
}
