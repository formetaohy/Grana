use std::collections::BTreeMap;

use grana_display::srgb_to_linear;
use grana_object::{Spot, SpotSet};
use grana_render::{SplatGeometry, SplatStyle};

const RAW_PIXELS: f32 = 3.0;
const CELL_PIXELS: f32 = 4.0;
const SAMPLE_LIMIT: usize = 1024;
const AGGREGATE_FLOOR: usize = 512;
const LADDER: u8 = 4;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const EMPTY: u32 = u32::MAX;

pub(crate) const STANDALONE: u32 = u32::MAX;

pub(crate) const LEVEL_COUNT: u8 = LADDER + 1;

#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub(crate) time: u32,
    pub(crate) spots: &'a SpotSet,
}

impl<'a> Frame<'a> {
    pub(crate) fn new(time: u32, spots: &'a SpotSet) -> Self {
        Self { time, spots }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Full,
    Level(u8),
}

impl Choice {
    pub(crate) fn level(self) -> u8 {
        match self {
            Self::Full => 0,
            Self::Level(level) => level,
        }
    }

    pub(crate) fn digest(self) -> u64 {
        u64::from(self.level())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Appearance {
    pub(crate) color: [f32; 3],
    pub(crate) visible: bool,
}

#[derive(Default)]
pub(crate) struct Detections {
    pub(crate) geometry: Vec<SplatGeometry>,
    pub(crate) style: Vec<SplatStyle>,
    pub(crate) members: Vec<u32>,
}

impl Detections {
    fn push(&mut self, geometry: SplatGeometry, style: SplatStyle, member: Option<usize>) {
        self.geometry.push(geometry);
        self.style.push(style);
        self.members.push(match member {
            Some(index) => index as u32,
            None => STANDALONE,
        });
    }

    pub(crate) fn len(&self) -> usize {
        self.geometry.len()
    }
}

pub(crate) struct Ladder {
    diameter: f32,
    cells: [f32; LADDER as usize],
}

impl Ladder {
    pub(crate) fn over(frames: &[Frame<'_>]) -> Option<Self> {
        let total = frames.iter().map(|frame| frame.spots.len()).sum::<usize>();
        if total == 0 {
            return None;
        }
        let stride = total.div_ceil(SAMPLE_LIMIT).max(1);
        let mut radii = Vec::with_capacity(total.div_ceil(stride) + frames.len());
        let mut seen = 0usize;
        for frame in frames {
            for spot in frame.spots.spots() {
                if seen.is_multiple_of(stride) {
                    radii.push(spot.radius());
                }
                seen += 1;
            }
        }
        radii.sort_unstable_by(f32::total_cmp);
        let diameter = 2.0 * radii[radii.len() / 2];
        if !(diameter.is_finite() && diameter > 0.0) {
            return None;
        }
        let mut cells = [0.0f32; LADDER as usize];
        for (slot, exponent) in cells.iter_mut().zip(1..=LADDER) {
            *slot = diameter * 2f32.powi(i32::from(exponent));
        }
        Some(Self { diameter, cells })
    }

    pub(crate) fn cell(&self, level: u8) -> f32 {
        assert!(
            (1..=LADDER).contains(&level),
            "a proxy level names one of the ladder rungs"
        );
        self.cells[level as usize - 1]
    }

    pub(crate) fn diameter(&self) -> f32 {
        self.diameter
    }
}

pub(crate) fn aggregated(frames: &[Frame<'_>]) -> bool {
    frames.iter().map(|frame| frame.spots.len()).sum::<usize>() >= AGGREGATE_FLOOR
}

pub(crate) fn choose(frames: &[Frame<'_>], world_per_pixel: f32) -> Choice {
    let projected = frames.len() > 1;
    let usable = world_per_pixel.is_finite() && world_per_pixel > 0.0;
    if !projected && (!aggregated(frames) || !usable) {
        return Choice::Full;
    }
    let Some(ladder) = Ladder::over(frames) else {
        return if projected {
            Choice::Level(1)
        } else {
            Choice::Full
        };
    };
    if !projected {
        let pixels = ladder.diameter() / world_per_pixel;
        if !pixels.is_finite() || pixels >= RAW_PIXELS {
            return Choice::Full;
        }
    }
    let wanted = if usable {
        CELL_PIXELS * world_per_pixel
    } else {
        ladder.diameter()
    };
    for level in 1..=LADDER {
        if ladder.cell(level) >= wanted {
            return Choice::Level(level);
        }
    }
    Choice::Level(LADDER)
}

pub(crate) fn detections<F>(
    frames: &[Frame<'_>],
    choice: Choice,
    opacity: f32,
    style: F,
) -> Detections
where
    F: Fn(u32, usize) -> Appearance,
{
    if frames.len() == 1 {
        let frame = frames[0];
        return match choice {
            Choice::Full => raw(frame.spots, opacity, &|index: usize| {
                style(frame.time, index)
            }),
            Choice::Level(level) => binned(frames, level, opacity, &style, true),
        };
    }
    let level = choice.level().max(1);
    binned(frames, level, opacity, &style, false)
}

pub(crate) fn highlights<F>(
    spots: &SpotSet,
    selected: &[u32],
    opacity: f32,
    visible: F,
) -> Detections
where
    F: Fn(usize) -> bool,
{
    let entries = spots.spots();
    let mut palette = Palette::default();
    let mut detections = Detections::default();
    let highlight = palette.radiance(SELECTED_COLOR);
    for index in selected {
        let index = *index as usize;
        let Some(spot) = entries.get(index) else {
            continue;
        };
        if !visible(index) {
            continue;
        }
        let sigma = spot.sigma().map(|value| SELECTED_SCALE * value);
        detections.push(
            SplatGeometry::gaussian(spot.center(), sigma),
            SplatStyle::new(highlight, opacity),
            Some(index),
        );
    }
    detections
}

fn raw<F>(spots: &SpotSet, opacity: f32, style: &F) -> Detections
where
    F: Fn(usize) -> Appearance,
{
    let entries = spots.spots();
    let mut palette = Palette::default();
    let mut detections = Detections {
        geometry: Vec::with_capacity(entries.len()),
        style: Vec::with_capacity(entries.len()),
        members: Vec::with_capacity(entries.len()),
    };
    for (index, spot) in entries.iter().enumerate() {
        let look = style(index);
        detections.push(
            SplatGeometry::gaussian(spot.center(), spot.sigma()),
            SplatStyle::new(
                palette.radiance(look.color),
                if look.visible { opacity } else { 0.0 },
            ),
            Some(index),
        );
    }
    detections
}

fn binned<F>(frames: &[Frame<'_>], level: u8, opacity: f32, style: &F, pickable: bool) -> Detections
where
    F: Fn(u32, usize) -> Appearance,
{
    let Some(ladder) = Ladder::over(frames) else {
        return Detections::default();
    };
    let mut grid = Grid::new(ladder.cell(level));
    let mut palette = Palette::default();
    for frame in frames {
        for (index, spot) in frame.spots.spots().iter().enumerate() {
            let look = style(frame.time, index);
            if !look.visible {
                continue;
            }
            let member = pickable.then_some(index);
            grid.add(member, spot, palette.radiance(look.color));
        }
    }
    let count = grid.len();
    let mut detections = Detections {
        geometry: Vec::with_capacity(count),
        style: Vec::with_capacity(count),
        members: Vec::with_capacity(count),
    };
    grid.finish(opacity, &mut detections);
    detections
}

#[derive(Default)]
struct Palette {
    cached: Option<([f32; 3], [f32; 3])>,
}

impl Palette {
    fn radiance(&mut self, color: [f32; 3]) -> [f32; 3] {
        match self.cached {
            Some((key, radiance)) if key == color => radiance,
            _ => {
                let radiance = srgb_to_linear(color);
                self.cached = Some((color, radiance));
                radiance
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Cell {
    count: u32,
    sum: [f64; 3],
    square: [f64; 3],
    sigma: [f64; 3],
    radiance: [f64; 3],
    member: u32,
    reach: f64,
}

impl Cell {
    const EMPTY: Self = Self {
        count: 0,
        sum: [0.0; 3],
        square: [0.0; 3],
        sigma: [0.0; 3],
        radiance: [0.0; 3],
        member: EMPTY,
        reach: f64::INFINITY,
    };
}

struct Grid {
    cell: f32,
    cells: BTreeMap<[i64; 3], Cell>,
}

impl Grid {
    fn new(cell: f32) -> Self {
        assert!(
            cell.is_finite() && cell > 0.0,
            "an aggregation cell spans a positive length"
        );
        Self {
            cell,
            cells: BTreeMap::new(),
        }
    }

    fn len(&self) -> usize {
        self.cells.len()
    }

    fn slot(&self, point: [f32; 3]) -> [i64; 3] {
        point.map(|value| (value / self.cell).floor() as i64)
    }

    fn add(&mut self, member: Option<usize>, spot: &Spot, radiance: [f32; 3]) {
        let center = spot.center();
        let at = self.slot(center);
        let sigma = spot.sigma();
        let cell = self.cells.entry(at).or_insert(Cell::EMPTY);
        if let Some(member) = member {
            let reach = [0, 1, 2]
                .map(|axis| {
                    let offset =
                        f64::from(center[axis]) - (at[axis] as f64 + 0.5) * f64::from(self.cell);
                    offset * offset
                })
                .into_iter()
                .sum::<f64>();
            if reach < cell.reach {
                cell.reach = reach;
                cell.member = member as u32;
            }
        }
        cell.count += 1;
        for axis in 0..3 {
            let value = f64::from(center[axis]);
            cell.sum[axis] += value;
            cell.square[axis] += value * value;
            cell.sigma[axis] += f64::from(sigma[axis]).powi(2);
            cell.radiance[axis] += f64::from(radiance[axis]);
        }
    }

    fn finish(&self, opacity: f32, detections: &mut Detections) {
        for cell in self.cells.values() {
            let count = f64::from(cell.count);
            let center = [0, 1, 2].map(|axis| (cell.sum[axis] / count) as f32);
            let sigma = [0, 1, 2].map(|axis| {
                let mean = cell.sum[axis] / count;
                let variance = (cell.square[axis] / count - mean * mean).max(0.0);
                ((variance + cell.sigma[axis] / count).sqrt() as f32).max(f32::MIN_POSITIVE)
            });
            let radiance = cell.radiance.map(|sum| (sum / count) as f32);
            let alpha = (1.0 - (1.0 - opacity).powi(cell.count as i32)).clamp(0.0, 1.0);
            detections.push(
                SplatGeometry::gaussian(center, sigma),
                SplatStyle::new(radiance, alpha),
                (cell.member != EMPTY).then_some(cell.member as usize),
            );
        }
    }
}
