pub use grana_object::LocalizationSpec;
use grana_object::Spot;

mod cluster;
mod fit;
mod fourier;
mod frc;
mod grid;
mod image;
mod nena;
mod report;
mod tone;

pub use cluster::{ClusterSpec, Clusters, cluster};
use grid::Grid;
pub use image::LocalizationRenderer;
pub use report::{Frc, LocalizationGeometry, LocalizationImage, LocalizationReport, NeNa};

const PREVIEW: u32 = 2048;

const FRC_LIMIT: usize = 2048;

const FRC_MINIMUM: usize = 64;

const REACH: f32 = 1.0;

const MARGIN: f32 = 3.0;

const SAMPLE: usize = 100_000;

const FLOOR: f32 = 1.0e-6;

#[derive(Clone, Copy)]
pub struct LocalizationFrame<'a> {
    spots: &'a [Spot],
    visible: Option<&'a [bool]>,
}

impl<'a> LocalizationFrame<'a> {
    pub fn new(spots: &'a [Spot], visible: Option<&'a [bool]>) -> Self {
        if let Some(visible) = visible {
            assert_eq!(
                visible.len(),
                spots.len(),
                "a localization frame marks every spot it holds"
            );
        }
        Self { spots, visible }
    }

    pub fn spots(&self) -> &'a [Spot] {
        self.spots
    }

    fn visible(&self, index: usize) -> bool {
        self.visible
            .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
    }

    fn count(&self) -> usize {
        match self.visible {
            Some(mask) => mask.iter().filter(|held| **held).count(),
            None => self.spots.len(),
        }
    }

    fn centers(&self) -> Vec<[f32; 2]> {
        self.spots
            .iter()
            .enumerate()
            .filter(|(index, _)| self.visible(*index))
            .map(|(_, spot)| {
                let center = spot.center();
                [center[0], center[1]]
            })
            .collect()
    }
}

pub fn localization_report(
    frames: &[LocalizationFrame<'_>],
    spec: LocalizationSpec,
    camera_pixel: f32,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<LocalizationReport>, String> {
    assert!(
        camera_pixel.is_finite() && camera_pixel > 0.0,
        "a localization report knows its camera pixel"
    );
    if frames.is_empty() {
        return Err("the frame range holds no frame".to_string());
    }
    let localizations = frames.iter().map(LocalizationFrame::count).sum::<usize>();
    if localizations == 0 {
        return Err("the frame range holds no visible localization".to_string());
    }
    let (low, high) = bounds(frames)
        .ok_or_else(|| "the localizations lie at an infinite position".to_string())?;
    let span = [0, 1].map(|axis| (high[axis] - low[axis]).max(FLOOR));
    let sigma = median_sigma(frames);
    let total = 3 * frames.len() + 1;
    let mut step = 0usize;
    let blur = spec.blur() * 0.001;
    let pixel = spec.pixel() * 0.001;
    let geometry = rasterize(&spec, low, span, pixel, blur)?;
    let preview = preview_geometry(low, span, geometry.pixel(), blur)?;
    let mut raster = Grid::empty(preview.size(), preview.pixel(), preview.origin());
    for frame in frames {
        for (index, spot) in frame.spots.iter().enumerate() {
            if !frame.visible(index) {
                continue;
            }
            let center = spot.center();
            raster.add([center[0], center[1]], 1.0);
        }
        step += 1;
        if !report(step, total) {
            return Ok(None);
        }
    }
    let nena = {
        let mut advance = |_: usize, _: usize| {
            step += 1;
            report(step, total)
        };
        match nena::measure(frames, REACH * camera_pixel, sigma, &mut advance)? {
            Some(nena) => nena,
            None => return Ok(None),
        }
    };
    let Some(frc) = measure_frc(frames, low, span, nena.precision(), &mut || {
        step += 1;
        report(step, total)
    })?
    else {
        return Ok(None);
    };
    raster.blur(blur);
    let (image, density) = tone::tone(&raster);
    step += 1;
    if !report(step, total) {
        return Ok(None);
    }
    Ok(Some(LocalizationReport {
        spec,
        pixel: geometry.pixel() * 1000.0,
        origin: geometry.origin(),
        extent: [0, 1].map(|axis| geometry.size()[axis] as f32 * geometry.pixel()),
        localizations: localizations as u64,
        geometry,
        density,
        nena,
        frc,
        image,
    }))
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

fn bounds(frames: &[LocalizationFrame<'_>]) -> Option<([f32; 2], [f32; 2])> {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    let mut found = false;
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
            found = true;
        }
    }
    found.then_some((low, high))
}

fn median_sigma(frames: &[LocalizationFrame<'_>]) -> f32 {
    let mut held = Vec::new();
    'outer: for frame in frames {
        for (index, spot) in frame.spots.iter().enumerate() {
            if !frame.visible(index) {
                continue;
            }
            let sigma = spot.sigma();
            held.push(0.5 * (sigma[0] + sigma[1]) * 1000.0);
            if held.len() >= SAMPLE {
                break 'outer;
            }
        }
    }
    if held.is_empty() {
        return 0.0;
    }
    held.sort_by(|left, right| left.total_cmp(right));
    held[held.len() / 2]
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
    precision: f32,
    report: &mut dyn FnMut() -> bool,
) -> Result<Option<Frc>, String> {
    let side = span[0].min(span[1]);
    let preferred = precision * 0.0005;
    let grid = ((side / preferred).ceil() as usize)
        .next_power_of_two()
        .clamp(FRC_MINIMUM, FRC_LIMIT);
    let pixel = side / grid as f32;
    let corner = [
        origin[0] + 0.5 * (span[0] - side),
        origin[1] + 0.5 * (span[1] - side),
    ];
    let mut first = Grid::empty([grid as u32; 2], pixel, corner);
    let mut second = Grid::empty([grid as u32; 2], pixel, corner);
    let mut index = 0u64;
    for frame in frames {
        for (spot_index, spot) in frame.spots.iter().enumerate() {
            if !frame.visible(spot_index) {
                continue;
            }
            let center = spot.center();
            if split(index) {
                first.add([center[0], center[1]], 1.0);
            } else {
                second.add([center[0], center[1]], 1.0);
            }
            index += 1;
        }
        if !report() {
            return Ok(None);
        }
    }
    Ok(Some(frc::measure(&first, &second)))
}

fn split(index: u64) -> bool {
    let mut value = index.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (value ^ (value >> 31)) & 1 == 1
}
