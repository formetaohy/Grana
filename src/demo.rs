use grana_analysis::{SpotParams, SurfaceParams};
use grana_volume::{Channel, Extent, Spacing, Volume, VoxelData};

use crate::scene::Scene;

const CELL_COUNT: usize = 30;
const PUNCTUM_COUNT: usize = 34;
const MARGIN: f32 = 42.0;
const NUCLEUS_THRESHOLD: f32 = 0.25;
const MEMBRANE_OPACITY: f32 = 0.35;
const SURFACE_VOLUME: f32 = 2.0;

pub const SURFACE_CHANNEL: usize = 0;

pub fn stage(scene: &mut Scene) {
    scene.channels[1].opacity = MEMBRANE_OPACITY;
    scene.select_channel(2);
    scene.spot_params = SpotParams::new(0.45, 0.1);
    scene.surface_params = SurfaceParams::new(
        scene
            .channel(SURFACE_CHANNEL)
            .intensity()
            .value(NUCLEUS_THRESHOLD),
        SURFACE_VOLUME,
    );
}

pub fn volume() -> Volume {
    let extent = Extent::new(512, 512, 64);
    let spacing = Spacing::new(0.1, 0.1, 0.35);
    let count = extent.voxel_count() as usize;
    let mut nuclei = background(extent);
    let mut membrane = vec![0u16; count];
    let mut puncta = vec![0u16; count];

    let mut random = Random::new(0x9E37_79B9_7F4A_7C15);
    for _ in 0..CELL_COUNT {
        let centre = [
            MARGIN + random.unit() * (extent.x() as f32 - 2.0 * MARGIN),
            MARGIN + random.unit() * (extent.y() as f32 - 2.0 * MARGIN),
            MARGIN * 0.2 + random.unit() * (extent.z() as f32 - 2.0 * MARGIN * 0.2),
        ];
        let radius_x = 14.0 + random.unit() * 16.0;
        let radius_y = radius_x * (0.8 + random.unit() * 0.4);
        let radius_z = (radius_x * 0.32).max(3.0);
        let brightness = 26000.0 + random.unit() * 36000.0;
        let phase = random.unit() * std::f32::consts::TAU;
        paint_nucleus(
            &mut nuclei,
            extent,
            centre,
            [radius_x, radius_y, radius_z],
            brightness,
            phase,
        );
        paint_membrane(
            &mut membrane,
            extent,
            centre,
            [radius_x * 1.24, radius_y * 1.24, radius_z * 1.35],
            15000.0 + random.unit() * 12000.0,
        );
    }

    for _ in 0..PUNCTUM_COUNT {
        let centre = [
            MARGIN * 0.5 + random.unit() * (extent.x() as f32 - MARGIN),
            MARGIN * 0.5 + random.unit() * (extent.y() as f32 - MARGIN),
            2.0 + random.unit() * (extent.z() as f32 - 4.0),
        ];
        let radius = 1.6 + random.unit() * 2.6;
        paint_nucleus(
            &mut puncta,
            extent,
            centre,
            [radius, radius, radius * 0.7],
            58000.0,
            random.unit() * std::f32::consts::TAU,
        );
    }

    Volume::new(
        extent,
        spacing,
        vec![
            Channel::new("Nuclei", [0.28, 0.45, 1.00], VoxelData::U16(nuclei)),
            Channel::new("Membrane", [0.25, 1.00, 0.40], VoxelData::U16(membrane)),
            Channel::new("Puncta", [1.00, 0.30, 0.65], VoxelData::U16(puncta)),
        ],
    )
}

fn background(extent: Extent) -> Vec<u16> {
    let mut random = Random::new(0x2545_F491_4F6C_DD1D);
    let mut voxels = vec![0u16; extent.voxel_count() as usize];
    for voxel in &mut voxels {
        *voxel = (random.unit() * 70.0) as u16;
    }
    voxels
}

fn paint_nucleus(
    voxels: &mut [u16],
    extent: Extent,
    centre: [f32; 3],
    radii: [f32; 3],
    brightness: f32,
    phase: f32,
) {
    for_each_voxel(extent, centre, radii, 2.4, |index, squared| {
        let profile = (-squared * 1.7).exp();
        let texture = 0.84 + 0.16 * (squared * 9.0 + phase).sin();
        let value = brightness * profile * texture;
        let voxel = &mut voxels[index];
        *voxel = (*voxel as f32 + value).min(u16::MAX as f32) as u16;
    });
}

fn paint_membrane(
    voxels: &mut [u16],
    extent: Extent,
    centre: [f32; 3],
    radii: [f32; 3],
    brightness: f32,
) {
    for_each_voxel(extent, centre, radii, 2.0, |index, squared| {
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
    extent: Extent,
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
    let (low_x, high_x) = bounds(0, extent.x());
    let (low_y, high_y) = bounds(1, extent.y());
    let (low_z, high_z) = bounds(2, extent.z());
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
                visit(extent.index(x, y, z) as usize, squared);
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
