use super::*;

use glam::Vec3;

const PICK_SIDE: u32 = 128;
const PICK_FLOOR: f32 = 0.1;
const PICK_STRIDE: f32 = 0.25;

impl Session {
    pub fn seed_peak(&self, channel: usize, origin: Vec3, direction: Vec3) -> Option<[f32; 3]> {
        let scene = self.scene.as_ref()?;
        let channels = self.source.as_ref().and_then(Loader::channels)?;
        if channel >= channels.image().pyramid().channel_count() {
            return None;
        }
        let pyramid = channels.pyramid();
        let level = pick_level(pyramid);
        let info = pyramid.level(level);
        let extent = info.extent();
        let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
        let time = scene.time();
        let data = channels
            .read_region(time, level, channel, &ranges, scene.drift().offset(time))
            .ok()?;
        let mut values = vec![0.0f32; data.voxel_count()];
        data.write_values(&mut values);
        let intensity = pyramid.channels()[channel].intensity();
        let low = Vec3::from(info.origin());
        let high = low + Vec3::from(info.world_extent());
        let (entry, exit) = ray_box(origin, direction, low, high)?;
        let voxel = info.voxel();
        let step = (PICK_STRIDE
            * voxel
                .iter()
                .fold(f32::INFINITY, |low, value| low.min(*value)))
        .max(1.0e-6);
        let limits = [extent.x() as i64, extent.y() as i64, extent.z() as i64];
        let width = extent.x() as usize;
        let height = extent.y() as usize;
        let mut best: Option<(f32, [u32; 3])> = None;
        let mut t = entry.max(0.0);
        while t <= exit {
            let point = origin + direction * t;
            let at = info.voxel_of(point.to_array());
            let cell =
                [0, 1, 2].map(|axis| (at[axis].floor() as i64).clamp(0, limits[axis] - 1) as u32);
            let index = cell[0] as usize + width * (cell[1] as usize + height * cell[2] as usize);
            let unit = intensity.unit(values[index]);
            if best.is_none_or(|(value, _)| unit > value) {
                best = Some((unit, cell));
            }
            t += step;
        }
        let (unit, cell) = best?;
        if unit < PICK_FLOOR {
            return None;
        }
        Some(info.voxel_center(cell.map(|value| value as f32)))
    }
}

fn pick_level(pyramid: &grana_volume::Pyramid) -> usize {
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        if pyramid.level(index).extent().max_dimension() >= PICK_SIDE {
            level = index;
        }
    }
    level
}

fn ray_box(origin: Vec3, direction: Vec3, low: Vec3, high: Vec3) -> Option<(f32, f32)> {
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 1.0e-9 {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return None;
            }
            continue;
        }
        let inverse = 1.0 / direction[axis];
        let first = (low[axis] - origin[axis]) * inverse;
        let second = (high[axis] - origin[axis]) * inverse;
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    (entry <= exit && exit >= 0.0).then_some((entry, exit))
}
