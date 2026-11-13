use glam::Vec3;
use grana_object::Spot;
pub use grana_object::{ClusterSpec, LocalizationFilter, LocalizationSpec};
use grana_volume::Region;

mod cluster;
mod fit;
mod fourier;
mod frc;
mod grid;
mod group;
mod image;
mod nena;
mod report;
mod tone;

pub use cluster::{Clusters, cluster};
use grid::Grid;
pub use group::{GroupStat, Groups};
pub use image::LocalizationRenderer;
pub use report::{Frc, LocalizationGeometry, LocalizationImage, LocalizationReport, NeNa};

const PREVIEW: u32 = 2048;

const FRC_LIMIT: usize = 2048;

const FRC_MINIMUM: usize = 64;

const REACH: f32 = 1.0;

const MARGIN: f32 = 3.0;

const SAMPLE: usize = 100_000;

const FLOOR: f32 = 1.0e-6;

const UNGROUPED: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub(super) struct Point {
    center: [f32; 2],
    group: u32,
}

#[derive(Clone, Copy)]
pub struct LocalizationFrame<'a> {
    spots: &'a [Spot],
    visible: Option<&'a [bool]>,
    filter: LocalizationFilter,
}

impl<'a> LocalizationFrame<'a> {
    pub fn new(spots: &'a [Spot], visible: Option<&'a [bool]>, filter: LocalizationFilter) -> Self {
        if let Some(visible) = visible {
            assert_eq!(
                visible.len(),
                spots.len(),
                "a localization frame marks every spot it holds"
            );
        }
        Self {
            spots,
            visible,
            filter,
        }
    }

    pub fn spots(&self) -> &'a [Spot] {
        self.spots
    }

    pub fn filter(&self) -> LocalizationFilter {
        self.filter
    }

    fn visible(&self, index: usize) -> bool {
        let spot = &self.spots[index];
        (self
            .visible
            .is_none_or(|mask| mask.get(index).copied().unwrap_or(false)))
            && self.filter.keeps(spot.quality(), spot.sigma())
    }

    fn count(&self) -> usize {
        (0..self.spots.len())
            .filter(|index| self.visible(*index))
            .count()
    }

    fn points(&self, grouped: bool) -> Vec<Point> {
        self.spots
            .iter()
            .enumerate()
            .filter(|(index, _)| self.visible(*index))
            .map(|(_, spot)| {
                let center = spot.center();
                Point {
                    center: [center[0], center[1]],
                    group: spot.group().filter(|_| grouped).unwrap_or(UNGROUPED),
                }
            })
            .collect()
    }
}

pub fn localization_report(
    frames: &[LocalizationFrame<'_>],
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
    if frames.is_empty() {
        return Err("the frame range holds no frame".to_string());
    }
    let filter = frames[0].filter();
    assert!(
        frames.iter().all(|frame| frame.filter() == filter),
        "a localization report filters every frame alike"
    );
    let survey = Survey::of(frames);
    if survey.localizations == 0 {
        return Err("the frame range holds no visible localization".to_string());
    }
    let (low, high) = survey
        .bounds
        .ok_or_else(|| "the localizations lie at an infinite position".to_string())?;
    let span = [0, 1].map(|axis| (high[axis] - low[axis]).max(FLOOR));
    let blur = spec.blur() * 0.001;
    let pixel = spec.pixel() * 0.001;
    let geometry = rasterize(&spec, low, span, pixel, blur)?;
    let preview = preview_geometry(low, span, geometry.pixel(), blur)?;
    let mut progress = Ticker::new(total_steps(frames.len()), report);
    let mut raster = if survey.groups.chromatic() {
        Grid::tinted(preview.size(), preview.pixel(), preview.origin())
    } else {
        Grid::empty(preview.size(), preview.pixel(), preview.origin())
    };
    for frame in frames {
        for (index, spot) in frame.spots.iter().enumerate() {
            if !frame.visible(index) {
                continue;
            }
            let center = spot.center();
            let tint = survey.groups.chromatic().then(|| {
                survey
                    .groups
                    .slot_tint(survey.groups.tint_slot(spot.group()))
            });
            raster.add([center[0], center[1]], 1.0, tint);
        }
        if !progress.tick() {
            return Ok(None);
        }
    }
    let nena = if frames.len() > 1 {
        let mut advance = |_: usize, _: usize| progress.tick();
        match nena::measure(
            frames,
            REACH * camera_pixel,
            survey.sigma,
            grouped,
            &mut advance,
        )? {
            Some(nena) => Some(nena),
            None => return Ok(None),
        }
    } else {
        None
    };
    let frc = {
        let mut advance = || progress.tick();
        match measure_frc(frames, low, span, survey.sigma, region, &mut advance)? {
            Some(frc) => frc,
            None => return Ok(None),
        }
    };
    raster.blur(blur);
    let (image, density) = tone::tone(&raster);
    if !progress.tick() {
        return Ok(None);
    }
    let precision = nena.as_ref().map_or(survey.sigma, |nena| nena.precision()) * 0.001;
    let suggested = cluster::estimate(frames, precision);
    if !progress.tick() {
        return Ok(None);
    }
    Ok(Some(LocalizationReport {
        spec,
        filter,
        region,
        pixel: geometry.pixel() * 1000.0,
        origin: geometry.origin(),
        extent: [0, 1].map(|axis| geometry.size()[axis] as f32 * geometry.pixel()),
        localizations: survey.localizations,
        geometry,
        density,
        nena,
        frc,
        suggested,
        groups: survey.groups,
        image,
    }))
}

struct Survey {
    localizations: u64,
    bounds: Option<([f32; 2], [f32; 2])>,
    sigma: f32,
    groups: Groups,
}

impl Survey {
    fn of(frames: &[LocalizationFrame<'_>]) -> Self {
        let total = frames.iter().map(LocalizationFrame::count).sum::<usize>();
        let stride = (total / SAMPLE).max(1);
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        let mut sigmas = Vec::with_capacity(total.div_ceil(stride).min(SAMPLE) + 1);
        let mut seen = 0usize;
        for frame in frames {
            for (index, spot) in frame.spots.iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let center = spot.center();
                for (axis, value) in center[..2].iter().enumerate() {
                    low[axis] = low[axis].min(*value);
                    high[axis] = high[axis].max(*value);
                }
                if seen.is_multiple_of(stride) {
                    let sigma = spot.sigma();
                    sigmas.push(0.5 * (sigma[0] + sigma[1]) * 1000.0);
                }
                seen += 1;
            }
        }
        sigmas.sort_by(|left, right| left.total_cmp(right));
        Self {
            localizations: total as u64,
            bounds: (seen > 0).then_some((low, high)),
            sigma: sigmas.get(sigmas.len() / 2).copied().unwrap_or(0.0),
            groups: Groups::measure(frames),
        }
    }
}

struct Ticker<'a> {
    done: usize,
    total: usize,
    report: &'a mut dyn FnMut(usize, usize) -> bool,
}

impl Ticker<'_> {
    fn new(total: usize, report: &mut dyn FnMut(usize, usize) -> bool) -> Ticker<'_> {
        Ticker {
            done: 0,
            total,
            report,
        }
    }

    fn tick(&mut self) -> bool {
        self.done += 1;
        (self.report)(self.done, self.total)
    }
}

fn total_steps(frames: usize) -> usize {
    2 * frames + if frames > 1 { frames } else { 0 } + 2
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
) -> Result<Preview, String> {
    let scale = ((span[0].max(span[1]) / pixel).ceil() / PREVIEW as f32)
        .ceil()
        .max(1.0);
    let pixel = pixel * scale;
    let border = margin_pixels(blur, pixel);
    let size = raster_size(span, pixel, border)?;
    Ok(Preview {
        origin: [
            low[0] - border as f32 * pixel,
            low[1] - border as f32 * pixel,
        ],
        pixel,
        size: [size[0].max(1), size[1].max(1)],
    })
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
    frames: &[LocalizationFrame<'_>],
    origin: [f32; 2],
    span: [f32; 2],
    sigma: f32,
    region: Option<Region>,
    report: &mut dyn FnMut() -> bool,
) -> Result<Option<Frc>, String> {
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
    let preferred = sigma * 0.0005;
    let grid = ((side / preferred).ceil() as usize)
        .next_power_of_two()
        .clamp(FRC_MINIMUM, FRC_LIMIT);
    let pixel = side / grid as f32;
    let mut first = Grid::empty([grid as u32; 2], pixel, corner);
    let mut second = Grid::empty([grid as u32; 2], pixel, corner);
    let mut index = 0u64;
    let mut kept = 0usize;
    for frame in frames {
        for (spot_index, spot) in frame.spots.iter().enumerate() {
            if !frame.visible(spot_index) {
                continue;
            }
            let center = spot.center();
            if region.is_some_and(|region| !region.contains(Vec3::from(center))) {
                continue;
            }
            kept += 1;
            if split(index) {
                first.add([center[0], center[1]], 1.0, None);
            } else {
                second.add([center[0], center[1]], 1.0, None);
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
