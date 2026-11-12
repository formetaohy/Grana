use glam::Vec3;
use grana_display::srgb_to_linear;
use grana_object::{Spot, SpotSet};
use grana_render::{SIGMA_PER_RADIUS, Splat};

const RAW_PIXELS: f32 = 3.0;
const CELL_PIXELS: f32 = 4.0;
const CELL_BUDGET: u64 = 1 << 15;
const AXIS_LIMIT: f32 = 32.0;
const STEP_LIMIT: u8 = 24;
const SAMPLE_LIMIT: usize = 1024;
const AGGREGATE_FLOOR: usize = 512;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;
const EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Full,
    Grid(u8),
}

impl Choice {
    pub(crate) fn digest(self) -> u64 {
        match self {
            Self::Full => u64::MAX,
            Self::Grid(step) => u64::from(step),
        }
    }
}

pub(crate) struct Appearance {
    pub(crate) color: [f32; 3],
    pub(crate) visible: bool,
    pub(crate) selected: bool,
}

#[derive(Default)]
pub(crate) struct Detections {
    pub(crate) splats: Vec<Splat>,
    pub(crate) members: Vec<Option<usize>>,
}

impl Detections {
    fn push(&mut self, splat: Splat, member: Option<usize>) {
        self.splats.push(splat);
        self.members.push(member);
    }
}

pub(crate) fn choose(spots: &SpotSet, world_per_pixel: f32) -> Choice {
    if spots.len() < AGGREGATE_FLOOR || !(world_per_pixel.is_finite() && world_per_pixel > 0.0) {
        return Choice::Full;
    }
    let Some(diameter) = diameter(spots.spots()) else {
        return Choice::Full;
    };
    let pixels = diameter / world_per_pixel;
    if !pixels.is_finite() || pixels >= RAW_PIXELS {
        return Choice::Full;
    }
    let step = (2.0 * (CELL_PIXELS / pixels).log2())
        .ceil()
        .clamp(1.0, f32::from(STEP_LIMIT)) as u8;
    Choice::Grid(step)
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
    let step = match choice {
        Choice::Full => return raw(entries, opacity, &style),
        Choice::Grid(step) => step,
    };
    let Some(diameter) = diameter(entries) else {
        return Detections::default();
    };
    let cell = cell_size(step, diameter, world);
    let Some(mut grid) = Grid::new(world, cell) else {
        return raw(entries, opacity, &style);
    };
    let mut palette = Palette::default();
    let mut selected = Vec::new();
    for (index, spot) in entries.iter().enumerate() {
        let look = style(index);
        if !look.visible {
            continue;
        }
        if look.selected {
            selected.push((index, spot.center(), spot.radius()));
            continue;
        }
        grid.add(index, spot, palette.radiance(look.color));
    }
    let mut detections = Detections {
        splats: Vec::with_capacity(grid.len().min(entries.len()) + selected.len()),
        members: Vec::with_capacity(grid.len().min(entries.len()) + selected.len()),
    };
    grid.finish(opacity, &mut detections);
    let highlight = palette.radiance(SELECTED_COLOR);
    for (index, center, radius) in selected {
        detections.push(
            Splat::gaussian(
                center,
                [SELECTED_SCALE * radius * SIGMA_PER_RADIUS; 3],
                highlight,
                opacity,
            ),
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
        splats: Vec::with_capacity(entries.len()),
        members: Vec::with_capacity(entries.len()),
    };
    for (index, spot) in entries.iter().enumerate() {
        let look = style(index);
        if !look.visible {
            continue;
        }
        let (radius, color) = if look.selected {
            (SELECTED_SCALE * spot.radius(), SELECTED_COLOR)
        } else {
            (spot.radius(), look.color)
        };
        detections.push(
            Splat::gaussian(
                spot.center(),
                [radius * SIGMA_PER_RADIUS; 3],
                palette.radiance(color),
                opacity,
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

fn cell_size(step: u8, diameter: f32, world: Vec3) -> f32 {
    let mut cell = diameter * 2f32.powf(0.5 * f32::from(step));
    cell = cell.max(world.max_element() / AXIS_LIMIT);
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
    sigma: f64,
    radiance: [f64; 3],
    member: u32,
    reach: f64,
}

impl Cell {
    const EMPTY: Self = Self {
        count: 0,
        sum: [0.0; 3],
        square: [0.0; 3],
        sigma: 0.0,
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
        let sigma = f64::from(spot.radius() * SIGMA_PER_RADIUS).powi(2);
        let cell = &mut self.cells[slot];
        if reach < cell.reach {
            cell.reach = reach;
            cell.member = index as u32;
        }
        cell.count += 1;
        cell.sigma += sigma;
        for axis in 0..3 {
            let value = f64::from(center[axis]);
            cell.sum[axis] += value;
            cell.square[axis] += value * value;
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
                ((variance + cell.sigma / count).sqrt() as f32).max(f32::MIN_POSITIVE)
            });
            let radiance = cell.radiance.map(|sum| (sum / count) as f32);
            let alpha = (1.0 - (1.0 - opacity).powi(cell.count as i32)).clamp(0.0, 1.0);
            detections.push(
                Splat::gaussian(center, sigma, radiance, alpha),
                (cell.member != EMPTY).then_some(cell.member as usize),
            );
        }
    }
}
