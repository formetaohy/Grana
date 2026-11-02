use grana_volume::Level;

const SCALE: f32 = 16.0;
const RADIUS_LIMIT: f32 = 8.0;
const CAP: u32 = u32::MAX / 4;

pub(crate) struct Element {
    offsets: Vec<([isize; 3], u32)>,
    radius: u32,
    reach: u32,
}

impl Element {
    pub(crate) fn new(level: Level, radius: f32) -> Option<Self> {
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let narrowest = spacing.iter().copied().fold(f32::INFINITY, f32::min);
        let radius = radius.min(RADIUS_LIMIT * narrowest);
        let weights = spacing.map(|value| (value / narrowest * SCALE).round().max(1.0) as i64);
        let mut offsets = Vec::with_capacity(13);
        for z in -1isize..=1 {
            for y in -1isize..=1 {
                for x in -1isize..=1 {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }
                    if z > 0 || (z == 0 && (y > 0 || (y == 0 && x > 0))) {
                        continue;
                    }
                    let squared = (x as i64 * weights[0]).pow(2)
                        + (y as i64 * weights[1]).pow(2)
                        + (z as i64 * weights[2]).pow(2);
                    offsets.push(([x, y, z], (squared as f64).sqrt().round().max(1.0) as u32));
                }
            }
        }
        Some(Self {
            offsets,
            radius: (radius / narrowest * SCALE).round() as u32,
            reach: (radius / narrowest).ceil() as u32,
        })
    }

    pub(crate) fn reach(&self) -> u32 {
        self.reach
    }
}

pub(crate) fn open(
    mask: &mut [bool],
    scratch: &mut [bool],
    distance: &mut [u32],
    dims: [usize; 3],
    element: &Element,
) {
    erode(mask, scratch, distance, dims, element);
    dilate(scratch, mask, distance, dims, element);
}

pub(crate) fn close(
    mask: &mut [bool],
    scratch: &mut [bool],
    distance: &mut [u32],
    dims: [usize; 3],
    element: &Element,
) {
    dilate(mask, scratch, distance, dims, element);
    erode(scratch, mask, distance, dims, element);
}

fn erode(
    input: &[bool],
    output: &mut [bool],
    distance: &mut [u32],
    dims: [usize; 3],
    element: &Element,
) {
    measure(distance, dims, input, false, element);
    let radius = element.radius;
    for (slot, value) in output.iter_mut().enumerate() {
        *value = distance[slot] > radius;
    }
}

fn dilate(
    input: &[bool],
    output: &mut [bool],
    distance: &mut [u32],
    dims: [usize; 3],
    element: &Element,
) {
    measure(distance, dims, input, true, element);
    let radius = element.radius;
    for (slot, value) in output.iter_mut().enumerate() {
        *value = input[slot] || distance[slot] <= radius;
    }
}

fn measure(distance: &mut [u32], dims: [usize; 3], mask: &[bool], target: bool, element: &Element) {
    for (slot, value) in distance.iter_mut().enumerate() {
        *value = if mask[slot] == target { 0 } else { CAP };
    }
    sweep(distance, dims, element, false);
    sweep(distance, dims, element, true);
}

fn sweep(distance: &mut [u32], dims: [usize; 3], element: &Element, reverse: bool) {
    let strides = [1usize, dims[0], dims[0] * dims[1]];
    for step_z in 0..dims[2] {
        let z = if reverse {
            dims[2] - 1 - step_z
        } else {
            step_z
        };
        for step_y in 0..dims[1] {
            let y = if reverse {
                dims[1] - 1 - step_y
            } else {
                step_y
            };
            for step_x in 0..dims[0] {
                let x = if reverse {
                    dims[0] - 1 - step_x
                } else {
                    step_x
                };
                let slot = x + strides[1] * y + strides[2] * z;
                if distance[slot] == 0 {
                    continue;
                }
                let mut best = distance[slot];
                for (offset, weight) in &element.offsets {
                    let sign = if reverse { -1 } else { 1 };
                    let reached = [
                        x as i64 + sign * offset[0] as i64,
                        y as i64 + sign * offset[1] as i64,
                        z as i64 + sign * offset[2] as i64,
                    ];
                    if reached[0] < 0
                        || reached[1] < 0
                        || reached[2] < 0
                        || reached[0] >= dims[0] as i64
                        || reached[1] >= dims[1] as i64
                        || reached[2] >= dims[2] as i64
                    {
                        continue;
                    }
                    let neighbour = reached[0] as usize
                        + strides[1] * reached[1] as usize
                        + strides[2] * reached[2] as usize;
                    let candidate = distance[neighbour].saturating_add(*weight);
                    if candidate < best {
                        best = candidate;
                    }
                }
                distance[slot] = best;
            }
        }
    }
}
