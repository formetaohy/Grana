use glam::Vec3;
use grana_analysis::{FilamentParams, FilamentScan};
use grana_geometry::GeometryStore;
use grana_volume::{Extent, Intensity, Level, Region, Spacing, VoxelData};

const EXTENT: Extent = Extent::new(72, 72, 32);
const SPACING: f32 = 0.25;
const PEAK: f32 = 40_000.0;
const BACKGROUND: f32 = 50.0;
const RADIUS: f32 = 2.0;
const INTENSITY_MAX: f32 = 65_535.0;
const THRESHOLD: f32 = 8_000.0;
const MIN_LENGTH: f32 = 2.5;

fn params() -> FilamentParams {
    FilamentParams::new(THRESHOLD / INTENSITY_MAX, MIN_LENGTH)
}

fn channel_intensity() -> Intensity {
    Intensity::new(0.0, INTENSITY_MAX)
}

#[test]
fn traces_a_dim_filament_on_a_coarse_grid() {
    let extent = Extent::new(40, 40, 30);
    let spacing = Spacing::new(0.3, 0.3, 0.1);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let offset = ((y as f32 - 20.0).powi(2) + (z as f32 - 15.0).powi(2)).sqrt();
                let intensity = 300.0 + 4_000.0 * (-0.5 * (offset / 0.9).powi(2)).exp();
                values[extent.index(x, y, z) as usize] = intensity as u16;
            }
        }
    }
    let data = VoxelData::U16(values);
    let level = Level::new(extent, spacing);
    let mut scan = FilamentScan::new(
        0,
        level,
        params(),
        1,
        0,
        Region::none(),
        channel_intensity(),
    );
    let set = scan
        .frame(&data, None, &GeometryStore::memory(), |_| Ok(data.clone()))
        .expect("the coarse filament scan traces the frame");
    assert_eq!(
        set.object_count(),
        1,
        "the dim filament survives the coarse working grid"
    );
    let length = set.object(0).expect("the filament exists").length();
    let expected = f64::from(extent.x()) * f64::from(spacing.x());
    assert!(
        length > 0.5 * expected,
        "the traced length {length:.2} µm keeps most of {expected:.2} µm"
    );
}

const SEGMENTS: [([f32; 3], [f32; 3]); 4] = [
    ([12.0, 12.0, 10.0], [40.0, 40.0, 10.0]),
    ([40.0, 40.0, 10.0], [58.0, 60.0, 18.0]),
    ([40.0, 40.0, 10.0], [62.0, 30.0, 22.0]),
    ([26.0, 26.0, 10.0], [26.0, 26.0, 13.0]),
];

#[test]
fn traces_branching_filaments() {
    let data = tubes(&SEGMENTS);
    let level = Level::new(EXTENT, Spacing::new(SPACING, SPACING, SPACING));
    let mut scan = FilamentScan::new(
        0,
        level,
        params(),
        1,
        0,
        Region::none(),
        channel_intensity(),
    );
    let set = scan
        .frame(&data, None, &GeometryStore::memory(), |_| Ok(data.clone()))
        .expect("the filament scan traces the frame");
    assert_eq!(set.object_count(), 1, "the spur merges into one filament");
    let statistics = set.object(0).expect("the filament exists");
    let expected = (distance(SEGMENTS[0]) + distance(SEGMENTS[1]) + distance(SEGMENTS[2]))
        * f64::from(SPACING);
    let length = statistics.length();
    assert!(
        (length - expected).abs() <= 0.2 * expected,
        "the traced length {length:.2} µm leaves the expected {expected:.2} µm"
    );
    assert_eq!(
        statistics.branches(),
        3,
        "the traced filament counts branches"
    );
    assert_eq!(
        statistics.terminals(),
        3,
        "the spur is pruned from the tips"
    );
    assert_eq!(statistics.junctions(), 1, "the traced filament splits once");
    let diameter = statistics.diameter();
    assert!(
        (1.0..=2.6).contains(&diameter),
        "the traced diameter {diameter:.2} µm follows the painted tube"
    );
    assert!(
        set.geometry().expect("the mesh loads").triangle_count() > 0,
        "the filament carries a mesh"
    );
    let intensity = set.intensities().object(0, 0).mean();
    assert!(
        intensity > THRESHOLD,
        "the filament is measured along its centerline: {intensity}"
    );

    let region = Region::new(Vec3::new(-9.0, -9.0, -4.0), Vec3::new(-2.0, 9.0, 4.0));
    let mut restricted = FilamentScan::new(0, level, params(), 1, 0, region, channel_intensity());
    let restricted = restricted
        .frame(&data, None, &GeometryStore::memory(), |_| Ok(data.clone()))
        .expect("the restricted filament scan traces the frame");
    assert_eq!(
        restricted.object_count(),
        1,
        "the region keeps the cut filament"
    );
    let cut = restricted
        .object(0)
        .expect("the cut filament exists")
        .length();
    assert!(
        cut > 0.0 && cut < 0.9 * length,
        "the region shortens the filament: {cut:.2} of {length:.2} µm"
    );
}

fn tubes(segments: &[([f32; 3], [f32; 3])]) -> VoxelData {
    let count = EXTENT.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let point = Vec3::new(x as f32, y as f32, z as f32);
                let closest = segments
                    .iter()
                    .map(|(start, end)| {
                        segment_distance(point, Vec3::from(*start), Vec3::from(*end))
                    })
                    .fold(f32::INFINITY, f32::min);
                let intensity = BACKGROUND + PEAK * (-0.5 * (closest / RADIUS).powi(2)).exp();
                values[EXTENT.index(x, y, z) as usize] =
                    (intensity.min(f32::from(u16::MAX))) as u16;
            }
        }
    }
    VoxelData::U16(values)
}

fn segment_distance(point: Vec3, start: Vec3, end: Vec3) -> f32 {
    let direction = end - start;
    let fraction = ((point - start).dot(direction) / direction.length_squared()).clamp(0.0, 1.0);
    point.distance(start + direction * fraction)
}

fn distance(segment: ([f32; 3], [f32; 3])) -> f64 {
    f64::from(Vec3::from(segment.1).distance(Vec3::from(segment.0)))
}

#[test]
fn traces_filaments_under_an_illumination_ramp() {
    let data = ramped(&SEGMENTS, 20_000.0);
    let level = Level::new(EXTENT, Spacing::new(SPACING, SPACING, SPACING));
    let mut scan = FilamentScan::new(
        0,
        level,
        params(),
        1,
        0,
        Region::none(),
        channel_intensity(),
    );
    let set = scan
        .frame(&data, None, &GeometryStore::memory(), |_| Ok(data.clone()))
        .expect("the filament scan traces the ramped frame");
    assert_eq!(
        set.object_count(),
        1,
        "the ramp does not flood the background"
    );
    let expected = (distance(SEGMENTS[0]) + distance(SEGMENTS[1]) + distance(SEGMENTS[2]))
        * f64::from(SPACING);
    let length = set.object(0).expect("the filament exists").length();
    assert!(
        (length - expected).abs() <= 0.2 * expected,
        "the ramped trace keeps {length:.2} µm of {expected:.2} µm"
    );
}

fn ramped(segments: &[([f32; 3], [f32; 3])], high: f32) -> VoxelData {
    let count = EXTENT.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let point = Vec3::new(x as f32, y as f32, z as f32);
                let closest = segments
                    .iter()
                    .map(|(start, end)| {
                        segment_distance(point, Vec3::from(*start), Vec3::from(*end))
                    })
                    .fold(f32::INFINITY, f32::min);
                let background = high * x as f32 / (EXTENT.x() - 1) as f32;
                let intensity = background + PEAK * (-0.5 * (closest / RADIUS).powi(2)).exp();
                values[EXTENT.index(x, y, z) as usize] =
                    (intensity.min(f32::from(u16::MAX))) as u16;
            }
        }
    }
    VoxelData::U16(values)
}
