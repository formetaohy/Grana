use grana_analysis::{SpotParams, SurfaceParams};
use grana_volume::{Channel, Extent, Movie, Spacing, Volume, VoxelData};

use crate::objects::{ObjectKind, ObjectParams, ObjectSetId};
use crate::scene::Scene;

const FRAMES: u32 = 6;
const FRAME_INTERVAL: f32 = 1.0;
const EXTENT: Extent = Extent::new(256, 256, 48);
const CELL_COUNT: usize = 12;
const PUNCTUM_COUNT: usize = 26;
const MARGIN: f32 = 36.0;
const NUCLEUS_THRESHOLD: f32 = 0.25;
const MEMBRANE_OPACITY: f32 = 0.35;
const MEMBRANE_RADIAL: f32 = 1.24;
const MEMBRANE_AXIAL: f32 = 1.35;
const SURFACE_VOLUME: f32 = 2.0;
const SPOT_DIAMETER: f32 = 0.45;
const SPOT_CONTRAST: f32 = 0.1;

pub const SURFACE_CHANNEL: usize = 0;
pub const SPOT_CHANNEL: usize = 2;
const SPOT_COLOR: [f32; 3] = [0.42, 0.74, 1.00];
const SURFACE_COLOR: [f32; 3] = [1.00, 0.72, 0.28];

pub struct Staged {
    pub spots: ObjectSetId,
    pub surfaces: ObjectSetId,
}

struct Cell {
    centre: [f32; 3],
    drift: [f32; 3],
    radii: [f32; 3],
    brightness: f32,
    membrane: f32,
    phase: f32,
}

struct Punctum {
    centre: [f32; 3],
    drift: [f32; 3],
    radius: f32,
}

pub fn stage(scene: &mut Scene) -> Staged {
    scene.channels[1].opacity = MEMBRANE_OPACITY;
    scene.select_channel(SPOT_CHANNEL);
    let spots = scene.add_object(ObjectKind::Spots, SPOT_CHANNEL);
    let surfaces = scene.add_object(ObjectKind::Surfaces, SURFACE_CHANNEL);
    let threshold = scene
        .channel(SURFACE_CHANNEL)
        .intensity()
        .value(NUCLEUS_THRESHOLD);
    if let Some(set) = scene.objects.get_mut(spots) {
        set.set_color(SPOT_COLOR);
        set.set_params(ObjectParams::Spots(SpotParams::new(
            SPOT_DIAMETER,
            SPOT_CONTRAST,
        )));
    }
    if let Some(set) = scene.objects.get_mut(surfaces) {
        set.set_color(SURFACE_COLOR);
        set.set_params(ObjectParams::Surfaces(SurfaceParams::new(
            threshold,
            SURFACE_VOLUME,
        )));
    }
    Staged { spots, surfaces }
}

pub fn movie() -> Movie {
    let spacing = spacing();
    let mut random = Random::new(0x9E37_79B9_7F4A_7C15);
    let cells = (0..CELL_COUNT)
        .map(|_| {
            let radius_x = 10.0 + random.unit() * 11.0;
            let radius_y = radius_x * (0.8 + random.unit() * 0.4);
            Cell {
                centre: [
                    MARGIN + random.unit() * (EXTENT.x() as f32 - 2.0 * MARGIN),
                    MARGIN + random.unit() * (EXTENT.y() as f32 - 2.0 * MARGIN),
                    8.0 + random.unit() * (EXTENT.z() as f32 - 16.0),
                ],
                drift: [
                    (random.unit() - 0.5) * 2.0,
                    (random.unit() - 0.5) * 2.0,
                    (random.unit() - 0.5) * 0.6,
                ],
                radii: [radius_x, radius_y, (radius_x * 0.32).max(3.0)],
                brightness: 26000.0 + random.unit() * 36000.0,
                membrane: 15000.0 + random.unit() * 12000.0,
                phase: random.unit() * std::f32::consts::TAU,
            }
        })
        .collect::<Vec<_>>();
    let puncta = (0..PUNCTUM_COUNT)
        .map(|_| Punctum {
            centre: [
                18.0 + random.unit() * (EXTENT.x() as f32 - 36.0),
                18.0 + random.unit() * (EXTENT.y() as f32 - 36.0),
                2.0 + random.unit() * (EXTENT.z() as f32 - 4.0),
            ],
            drift: [
                (random.unit() - 0.5) * 4.0,
                (random.unit() - 0.5) * 4.0,
                (random.unit() - 0.5) * 1.2,
            ],
            radius: 1.2 + random.unit() * 1.4,
        })
        .collect::<Vec<_>>();

    let count = EXTENT.voxel_count() as usize;
    let mut frames = Vec::with_capacity(FRAMES as usize);
    for frame in 0..FRAMES {
        let time = frame as f32;
        let mut nuclei = background(frame);
        let mut membrane = vec![0u16; count];
        let mut puncta_data = vec![0u16; count];
        for cell in &cells {
            let centre = moved(cell.centre, cell.drift, time);
            paint_nucleus(
                &mut nuclei,
                centre,
                cell.radii,
                cell.brightness,
                cell.phase + 0.3 * time,
            );
            paint_membrane(
                &mut membrane,
                centre,
                [
                    cell.radii[0] * MEMBRANE_RADIAL,
                    cell.radii[1] * MEMBRANE_RADIAL,
                    cell.radii[2] * MEMBRANE_AXIAL,
                ],
                cell.membrane,
            );
        }
        for punctum in &puncta {
            let centre = moved(punctum.centre, punctum.drift, time);
            paint_nucleus(
                &mut puncta_data,
                centre,
                [punctum.radius, punctum.radius, punctum.radius * 0.7],
                58000.0,
                0.7 * time + punctum.radius,
            );
        }
        frames.push(Volume::new(
            EXTENT,
            spacing,
            vec![
                Channel::new("Nuclei", [0.28, 0.45, 1.00], VoxelData::U16(nuclei)),
                Channel::new("Membrane", [0.25, 1.00, 0.40], VoxelData::U16(membrane)),
                Channel::new("Puncta", [1.00, 0.30, 0.65], VoxelData::U16(puncta_data)),
            ],
        ));
    }
    Movie::new(frames, FRAME_INTERVAL)
}

fn spacing() -> Spacing {
    Spacing::new(0.1, 0.1, 0.35)
}

fn background(frame: u32) -> Vec<u16> {
    let mut random = Random::new(0x2545_F491_4F6C_DD1D ^ u64::from(frame));
    let mut voxels = vec![0u16; EXTENT.voxel_count() as usize];
    for voxel in &mut voxels {
        *voxel = (random.unit() * 70.0) as u16;
    }
    voxels
}

fn moved(centre: [f32; 3], drift: [f32; 3], time: f32) -> [f32; 3] {
    [0, 1, 2].map(|axis| centre[axis] + drift[axis] * time)
}

fn paint_nucleus(
    voxels: &mut [u16],
    centre: [f32; 3],
    radii: [f32; 3],
    brightness: f32,
    phase: f32,
) {
    for_each_voxel(centre, radii, 2.4, |index, squared| {
        let profile = (-squared * 1.7).exp();
        let texture = 0.84 + 0.16 * (squared * 9.0 + phase).sin();
        let value = brightness * profile * texture;
        let voxel = &mut voxels[index];
        *voxel = (*voxel as f32 + value).min(u16::MAX as f32) as u16;
    });
}

fn paint_membrane(voxels: &mut [u16], centre: [f32; 3], radii: [f32; 3], brightness: f32) {
    for_each_voxel(centre, radii, 2.0, |index, squared| {
        let distance = squared.sqrt();
        let shell = (-((distance - 1.0) / 0.16).powi(2)).exp();
        if shell < 0.02 {
            return;
        }
        let voxel = &mut voxels[index];
        *voxel = (*voxel as f32 + brightness * shell).min(u16::MAX as f32) as u16;
    });
}

fn for_each_voxel(
    centre: [f32; 3],
    radii: [f32; 3],
    reach: f32,
    mut visit: impl FnMut(usize, f32),
) {
    let bounds = |axis: usize, size: u32| {
        let extent = radii[axis] * reach;
        let low = ((centre[axis] - extent).floor().max(0.0)) as u32;
        let high = (((centre[axis] + extent).ceil()) as u32).min(size - 1);
        (low, high)
    };
    let (low_x, high_x) = bounds(0, EXTENT.x());
    let (low_y, high_y) = bounds(1, EXTENT.y());
    let (low_z, high_z) = bounds(2, EXTENT.z());
    for z in low_z..=high_z {
        let nz = (z as f32 - centre[2]) / radii[2];
        for y in low_y..=high_y {
            let ny = (y as f32 - centre[1]) / radii[1];
            for x in low_x..=high_x {
                let nx = (x as f32 - centre[0]) / radii[0];
                let squared = nx * nx + ny * ny + nz * nz;
                if squared > reach * reach {
                    continue;
                }
                visit(EXTENT.index(x, y, z) as usize, squared);
            }
        }
    }
}

struct Random {
    state: u64,
}

impl Random {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        (self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    fn unit(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}
