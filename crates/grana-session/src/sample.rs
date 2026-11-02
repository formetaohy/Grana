use glam::Vec3;
use grana_analysis::{FilamentParams, SpotParams, SurfaceParams};
use grana_scene::{ObjectKind, ObjectParams, ObjectSetId, Scene};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

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
const SURFACE_GRAIN: f32 = 0.15;
const SURFACE_SEED: f32 = 1.2;
const SPOT_DIAMETER: f32 = 0.45;
const SPOT_CONTRAST: f32 = 0.1;
const VESSEL_OPACITY: f32 = 0.18;
const VESSEL_TREES: usize = 2;
const VESSEL_DEPTH: u32 = 3;
const VESSEL_BRIGHTNESS: f32 = 38_000.0;
const VESSEL_THRESHOLD: f32 = 0.35;
const VESSEL_MIN_LENGTH: f32 = 3.0;
const VESSEL_ROOT_RADIUS: f32 = 1.3;
const VESSEL_TAPER: f32 = 0.8;
const VESSEL_REACH: f32 = 2.2;
const VESSEL_FLOOR: f32 = 30.0;

pub const SURFACE_CHANNEL: usize = 0;
pub const SPOT_CHANNEL: usize = 2;
const VESSEL_CHANNEL: usize = 3;
const SPOT_COLOR: [f32; 3] = [0.42, 0.74, 1.00];
const SURFACE_COLOR: [f32; 3] = [1.00, 0.72, 0.28];
const VESSEL_COLOR: [f32; 3] = [0.45, 0.95, 0.70];

pub struct Staged {
    pub spots: ObjectSetId,
    pub surfaces: ObjectSetId,
    pub filaments: ObjectSetId,
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
    scene.channels[VESSEL_CHANNEL].opacity = VESSEL_OPACITY;
    scene.select_channel(SPOT_CHANNEL);
    let spots = scene.add_object(ObjectKind::Spots, SPOT_CHANNEL);
    let surfaces = scene.add_object(ObjectKind::Surfaces, SURFACE_CHANNEL);
    let filaments = scene.add_object(ObjectKind::Filaments, VESSEL_CHANNEL);
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
            NUCLEUS_THRESHOLD,
            SURFACE_GRAIN,
            SURFACE_SEED,
            SURFACE_VOLUME,
        )));
    }
    if let Some(set) = scene.objects.get_mut(filaments) {
        set.set_color(VESSEL_COLOR);
        set.set_params(ObjectParams::Filaments(FilamentParams::new(
            VESSEL_THRESHOLD,
            VESSEL_MIN_LENGTH,
        )));
    }
    Staged {
        spots,
        surfaces,
        filaments,
    }
}

pub fn series() -> VolumeSeries {
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
    let vessels = vessels(&mut random);
    let mut frames = Vec::with_capacity(FRAMES as usize);
    for frame in 0..FRAMES {
        let time = frame as f32;
        let mut nuclei = background(frame);
        let mut membrane = vec![0u16; count];
        let mut puncta_data = vec![0u16; count];
        let mut vessel_data = vec![0u16; count];
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
        paint_vessels(&mut vessel_data, &vessels);
        frames.push(Volume::new(
            EXTENT,
            spacing,
            vec![
                Channel::new("Nuclei", [0.28, 0.45, 1.00], VoxelData::U16(nuclei)),
                Channel::new("Membrane", [0.25, 1.00, 0.40], VoxelData::U16(membrane)),
                Channel::new("Puncta", [1.00, 0.30, 0.65], VoxelData::U16(puncta_data)),
                Channel::new("Vessels", VESSEL_COLOR, VoxelData::U16(vessel_data)),
            ],
        ));
    }
    VolumeSeries::new(frames, FRAME_INTERVAL)
}

struct Vessel {
    start: [f32; 3],
    end: [f32; 3],
    start_radius: f32,
    end_radius: f32,
}

fn vessels(random: &mut Random) -> Vec<Vessel> {
    let spacing = spacing();
    let world = [
        EXTENT.x() as f32 * spacing.x(),
        EXTENT.y() as f32 * spacing.y(),
        EXTENT.z() as f32 * spacing.z(),
    ];
    let mut vessels = Vec::new();
    for tree in 0..VESSEL_TREES {
        let lane = (tree as f32 + 0.5) / VESSEL_TREES as f32;
        let start = [
            1.5,
            world[1] * (0.2 + 0.6 * lane),
            world[2] * (0.25 + 0.5 * random.unit()),
        ];
        let direction = Vec3::new(1.0, (random.unit() - 0.5) * 0.5, 0.0).normalize();
        grow(
            &mut vessels,
            random,
            start,
            direction,
            VESSEL_ROOT_RADIUS,
            VESSEL_DEPTH,
            world,
        );
    }
    vessels
}

fn grow(
    vessels: &mut Vec<Vessel>,
    random: &mut Random,
    start: [f32; 3],
    direction: Vec3,
    radius: f32,
    depth: u32,
    world: [f32; 3],
) {
    if depth == 0 {
        return;
    }
    let length = 4.0 + random.unit() * 3.0;
    let end = Vec3::from(start) + direction * length;
    if end.x > world[0] - 1.0
        || end.y < 1.0
        || end.y > world[1] - 1.0
        || end.z < 0.5
        || end.z > world[2] - 0.5
    {
        return;
    }
    let end_radius = radius * VESSEL_TAPER;
    vessels.push(Vessel {
        start,
        end: end.to_array(),
        start_radius: radius,
        end_radius,
    });
    let angle = direction.y.atan2(direction.x);
    for branch in 0..2 {
        let side = if branch == 0 { 1.0 } else { -1.0 };
        let turned = angle + side * (0.25 + random.unit() * 0.25);
        let tilt = (random.unit() - 0.5) * 0.12;
        let direction = Vec3::new(turned.cos(), turned.sin(), tilt).normalize();
        grow(
            vessels,
            random,
            end.to_array(),
            direction,
            end_radius,
            depth - 1,
            world,
        );
    }
}

fn paint_vessels(voxels: &mut [u16], vessels: &[Vessel]) {
    let spacing = spacing();
    let step = [spacing.x(), spacing.y(), spacing.z()];
    let size = [EXTENT.x(), EXTENT.y(), EXTENT.z()];
    for vessel in vessels {
        let mut low = [0u32; 3];
        let mut high = [0u32; 3];
        for axis in 0..3 {
            let reach = vessel.start_radius.max(vessel.end_radius) * VESSEL_REACH;
            let minimum = (vessel.start[axis].min(vessel.end[axis]) - reach) / step[axis];
            let maximum = (vessel.start[axis].max(vessel.end[axis]) + reach) / step[axis];
            low[axis] = minimum.floor().max(0.0) as u32;
            high[axis] = (maximum.ceil() as u32).min(size[axis] - 1);
        }
        let start = Vec3::from(vessel.start);
        let end = Vec3::from(vessel.end);
        let direction = end - start;
        let squared = direction.length_squared();
        for z in low[2]..=high[2] {
            for y in low[1]..=high[1] {
                for x in low[0]..=high[0] {
                    let point =
                        Vec3::new(x as f32 * step[0], y as f32 * step[1], z as f32 * step[2]);
                    let fraction = ((point - start).dot(direction) / squared).clamp(0.0, 1.0);
                    let radius =
                        vessel.start_radius + (vessel.end_radius - vessel.start_radius) * fraction;
                    let distance = point.distance(start + direction * fraction);
                    let value = VESSEL_BRIGHTNESS * (-0.5 * (distance / radius).powi(2)).exp();
                    if value < VESSEL_FLOOR {
                        continue;
                    }
                    let voxel = &mut voxels[EXTENT.index(x, y, z) as usize];
                    *voxel = (*voxel as f32 + value).min(u16::MAX as f32) as u16;
                }
            }
        }
    }
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
