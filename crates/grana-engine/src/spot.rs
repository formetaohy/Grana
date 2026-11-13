use glam::Vec3;
use grana_display::srgb_to_linear;
use grana_object::{Spot, SpotSet};
use grana_render::{SplatGeometry, SplatStyle};

const RAW_PIXELS: f32 = 3.0;
const CELL_PIXELS: f32 = 4.0;
const CELL_BUDGET: u64 = 1 << 15;
const AXIS_LIMIT: f32 = 32.0;
const SAMPLE_LIMIT: usize = 1024;
const AGGREGATE_FLOOR: usize = 512;
const LADDER: u8 = 4;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const EMPTY: u32 = u32::MAX;

pub(crate) const STANDALONE: u32 = u32::MAX;

pub(crate) const LEVEL_COUNT: u8 = LADDER + 1;

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
}

pub(crate) struct Ladder {
    diameter: f32,
    cells: [f32; LADDER as usize],
}

impl Ladder {
    pub(crate) fn new(spots: &[Spot], world: Vec3) -> Option<Self> {
        let diameter = diameter(spots)?;
        let mut cells = [0.0f32; LADDER as usize];
        for (slot, exponent) in cells.iter_mut().zip(1..=LADDER) {
            *slot = cell_size(diameter * 2f32.powi(i32::from(exponent)), world);
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

pub(crate) fn aggregated(spots: &SpotSet) -> bool {
    spots.len() >= AGGREGATE_FLOOR
}

pub(crate) fn choose(spots: &SpotSet, world: Vec3, world_per_pixel: f32) -> Choice {
    if !aggregated(spots) || !(world_per_pixel.is_finite() && world_per_pixel > 0.0) {
        return Choice::Full;
    }
    let Some(ladder) = Ladder::new(spots.spots(), world) else {
        return Choice::Full;
    };
    let pixels = ladder.diameter() / world_per_pixel;
    if !pixels.is_finite() || pixels >= RAW_PIXELS {
        return Choice::Full;
    }
    let wanted = CELL_PIXELS * world_per_pixel;
    for level in 1..=LADDER {
        if ladder.cell(level) >= wanted {
            return Choice::Level(level);
        }
    }
    Choice::Level(LADDER)
}

pub(crate) fn detections<F>(
    spots: &SpotSet,
    choice: Choice,
    world: Vec3,
    opacity: f32,
    style: F,
) -> Detections
where
    F: Fn(usize) -> Appearance,
{
    let entries = spots.spots();
    let Some(ladder) = Ladder::new(entries, world) else {
        return Detections::default();
    };
    let level = match choice {
        Choice::Full => return raw(entries, opacity, &style),
        Choice::Level(level) => level,
    };
    let Some(mut grid) = Grid::new(world, ladder.cell(level)) else {
        return raw(entries, opacity, &style);
    };
    let mut palette = Palette::default();
    for (index, spot) in entries.iter().enumerate() {
        let look = style(index);
        if !look.visible {
            continue;
        }
        grid.add(index, spot, palette.radiance(look.color));
    }
    let mut detections = Detections {
        geometry: Vec::with_capacity(grid.len().min(entries.len())),
        style: Vec::with_capacity(grid.len().min(entries.len())),
        members: Vec::with_capacity(grid.len().min(entries.len())),
    };
    grid.finish(opacity, &mut detections);
    detections
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

fn raw<F>(entries: &[Spot], opacity: f32, style: &F) -> Detections
where
    F: Fn(usize) -> Appearance,
{
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

fn diameter(entries: &[Spot]) -> Option<f32> {
    if entries.is_empty() {
        return None;
    }
    let stride = entries.len().div_ceil(SAMPLE_LIMIT).max(1);
    let mut radii = entries
        .iter()
        .step_by(stride)
        .map(|spot| spot.radius())
        .collect::<Vec<_>>();
    radii.sort_unstable_by(f32::total_cmp);
    Some(2.0 * radii[radii.len() / 2])
}

fn cell_size(cell: f32, world: Vec3) -> f32 {
    let mut cell = cell.max(world.max_element() / AXIS_LIMIT);
    while cells(world, cell) > CELL_BUDGET {
        cell *= 2.0;
    }
    cell
}

fn dims(world: Vec3, cell: f32) -> [u32; 3] {
    [world.x, world.y, world.z]
        .map(|span| ((span.max(0.0) / cell).ceil() as u64).clamp(1, AXIS_LIMIT as u64) as u32)
}

fn cells(world: Vec3, cell: f32) -> u64 {
    let dims = dims(world, cell);
    u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2])
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
    origin: Vec3,
    cell: f32,
    dims: [u32; 3],
    cells: Vec<Cell>,
}

impl Grid {
    fn new(world: Vec3, cell: f32) -> Option<Self> {
        let dims = dims(world, cell);
        let count = u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2]);
        if count > CELL_BUDGET {
            return None;
        }
        Some(Self {
            origin: -0.5 * world,
            cell,
            dims,
            cells: vec![Cell::EMPTY; count as usize],
        })
    }

    fn len(&self) -> usize {
        self.cells.len()
    }

    fn slot(&self, point: [f32; 3]) -> ([usize; 3], usize) {
        let mut at = [0usize; 3];
        for axis in 0..3 {
            let cell = ((point[axis] - self.origin[axis]) / self.cell).floor();
            let limit = self.dims[axis].saturating_sub(1) as f32;
            at[axis] = cell.clamp(0.0, limit) as usize;
        }
        let slot = at[0] + self.dims[0] as usize * (at[1] + self.dims[1] as usize * at[2]);
        (at, slot)
    }

    fn add(&mut self, index: usize, spot: &Spot, radiance: [f32; 3]) {
        let center = spot.center();
        let (at, slot) = self.slot(center);
        let reach = [0, 1, 2]
            .iter()
            .map(|axis| {
                let offset = f64::from(center[*axis])
                    - f64::from(self.origin[*axis])
                    - (at[*axis] as f64 + 0.5) * f64::from(self.cell);
                offset * offset
            })
            .sum::<f64>();
        let sigma = spot.sigma();
        let cell = &mut self.cells[slot];
        if reach < cell.reach {
            cell.reach = reach;
            cell.member = index as u32;
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
        for cell in &self.cells {
            if cell.count == 0 {
                continue;
            }
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
