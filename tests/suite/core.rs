use grana_gpu::{Consumer, Ledger};
use grana_image::{Image, Source};
use grana_volume::{
    BrickCoord, BrickData, BrickGrid, Calibration, Channel, ChannelInfo, ChannelSet, ColorTable,
    Extent, Intensity, Level, NoData, PADDED_SIDE, PackedBrick, Pyramid, Spacing, Volume,
    VolumeSeries, VoxelData, VoxelFormat, blank_brick, fill_is_background,
};
use grana_warp::Warp;

#[test]
fn a_ledger_never_lets_the_renderer_and_inference_oversubscribe_the_device() {
    let ledger = Ledger::new(100);
    let render = ledger
        .claim(Consumer::Render, 60)
        .expect("the renderer takes the first share");
    assert!(
        ledger.claim(Consumer::Inference, 50).is_none(),
        "inference above the spare allowance is refused"
    );
    let inference = ledger
        .claim(Consumer::Inference, 40)
        .expect("inference takes the rest");
    assert_eq!(ledger.spare(), 0, "both shares fill the allowance");
    let mut render = render;
    assert!(
        !render.resize(61),
        "the renderer cannot grow past the allowance"
    );
    assert_eq!(render.bytes(), 60, "a refused resize keeps the claim");
    drop(inference);
    assert_eq!(ledger.spare(), 40, "the released share returns");
    assert!(
        render.resize(100),
        "the renderer grows into the released share"
    );
    assert_eq!(ledger.claimed(Consumer::Render), 100);
}

#[test]
fn builds_padded_bricks_from_a_volume() {
    let extent = Extent::new(40, 36, 20);
    let count = extent.voxel_count() as usize;
    let mut first = Vec::with_capacity(count);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                first.push((7 + x + 10 * y + 100 * z) as u16);
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(0.2, 0.2, 0.5),
        vec![
            Channel::new("first", [0.2, 0.5, 1.0], VoxelData::U16(first)),
            Channel::new("second", [1.0, 0.5, 0.2], VoxelData::U16(vec![9; count])),
        ],
    );
    let pyramid = volume.pyramid();
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.level(0).extent(), extent);
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 0.5));
    assert_eq!(pyramid.channels().len(), 2);
    assert_eq!(pyramid.channels()[0].name(), "first");
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(7.0, 2296.0)
    );
    assert_eq!(pyramid.channels()[1].intensity(), Intensity::new(9.0, 9.0));

    let grid = BrickGrid::new(extent);
    assert_eq!(grid.counts(), [2, 2, 1]);
    assert_eq!(grid.count(), 4);
    let brick = volume.brick(&grid, BrickCoord::new(0, 0, 0));
    assert_eq!(brick.channels().len(), 2);
    let VoxelData::U16(values) = &brick.channels()[0] else {
        panic!("the brick keeps the voxel format");
    };
    assert_eq!(
        values.len(),
        (PADDED_SIDE * PADDED_SIDE * PADDED_SIDE) as usize
    );
    let side = PADDED_SIDE as usize;
    let padded = |x: usize, y: usize, z: usize| values[x + side * (y + side * z)];
    assert_eq!(padded(0, 0, 0), 7);
    assert_eq!(padded(1, 1, 1), 7);
    assert_eq!(padded(2, 2, 2), 7 + 1 + 10 + 100);
    assert_eq!(padded(3, 3, 3), 7 + 2 + 20 + 200);
    assert_eq!(padded(33, 33, 21), 7 + 32 + 320 + 1900);
    let VoxelData::U16(second) = &brick.channels()[1] else {
        panic!("the brick keeps the voxel format");
    };
    assert!(second.iter().all(|value| *value == 9));

    let region = volume.channels()[0].data().region(extent, 4..8, 2..5, 1..2);
    let VoxelData::U16(values) = region else {
        panic!("the region keeps the voxel format");
    };
    assert_eq!(values.len(), 4 * 3);
    assert_eq!(values[0], 7 + 4 + 20 + 100);
    assert_eq!(values[values.len() - 1], 7 + 7 + 40 + 100);
}

#[test]
fn grades_missing_voxels_out_of_statistics_and_derived_levels() {
    let extent = Extent::new(64, 64, 64);
    let absent = NoData::F32(-1.0);
    let mut values = (0..extent.voxel_count())
        .map(|index| 2.0 + (index % 1000) as f32 * 1.0e-6)
        .collect::<Vec<f32>>();
    values[0] = -1.0;
    let channel =
        Channel::new("signal", [1.0, 1.0, 1.0], VoxelData::F32(values)).with_no_data(absent);
    let volume = Volume::new(extent, Spacing::new(1.0, 1.0, 1.0), vec![channel]);
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    let info = &image.pyramid().channels()[0];
    assert_eq!(info.no_data(), Some(absent));
    assert_eq!(
        info.statistics().intensity().min(),
        2.0,
        "the census leaves the declared no-data value out"
    );
    assert!(info.statistics().intensity().max() < 2.01);

    let ranges = [60..68, 0..2, 0..2];
    let data = image
        .read_region(0, 0, 0, &ranges, &Warp::IDENTITY)
        .expect("a region across the volume edge reads");
    let VoxelData::F32(read) = data else {
        panic!("the read keeps the voxel format");
    };
    for z in 0..2u32 {
        for y in 0..2u32 {
            for x in 0..8u32 {
                let index = (x + 8 * (y + 2 * z)) as usize;
                let value = read[index];
                if x < 4 {
                    let voxel = (x + 60) as usize + 64 * (y as usize + 64 * z as usize);
                    let expected = 2.0 + (voxel % 1000) as f32 * 1.0e-6;
                    assert_eq!(value, expected, "inside voxel {x},{y},{z}");
                } else {
                    assert_eq!(value, -1.0, "the region outside the extent holds the fill");
                }
            }
        }
    }

    let derived = image
        .read_region(0, 1, 0, &[0..4, 0..4, 0..4], &Warp::IDENTITY)
        .expect("a derived level reads");
    let VoxelData::F32(level) = derived else {
        panic!("the derived level keeps the voxel format");
    };
    assert!(
        (level[0] - 2.0).abs() < 1.0e-3,
        "a reduced voxel averages only the voxels that hold data"
    );
    assert!(
        level.iter().all(|value| (value - 2.0).abs() < 1.0e-3),
        "every reduced voxel leaves the no-data value out"
    );
    assert!(NoData::F32(f32::NAN).matches_sample(f32::NAN));
}

#[test]
fn keeps_a_shifted_region_at_its_intensity_across_the_volume_edge() {
    let extent = Extent::new(32, 32, 16);
    let values = (0..extent.voxel_count())
        .map(|index| 2.0 + (index % 1000) as f32 * 1.0e-6)
        .collect::<Vec<f32>>();
    let channel = Channel::new("signal", [1.0, 1.0, 1.0], VoxelData::F32(values));
    let volume = Volume::new(extent, Spacing::new(1.0, 1.0, 1.0), vec![channel]);
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    let warp = Warp::shift([3.5, 0.0, 0.0]);
    let shifted = image
        .read_region(0, 0, 0, &[28..29, 8..9, 4..5], &warp)
        .expect("a warped voxel reads");
    let source = image
        .read_region(0, 0, 0, &[31..32, 8..9, 4..5], &Warp::IDENTITY)
        .expect("the source voxel reads");
    let (VoxelData::F32(shifted), VoxelData::F32(source)) = (shifted, source) else {
        panic!("the reads keep the voxel format");
    };
    assert_eq!(
        shifted[0], source[0],
        "a half covered tap renormalises instead of dimming the edge voxel"
    );
    let beyond = image
        .read_region(0, 0, 0, &[29..32, 8..9, 4..5], &warp)
        .expect("a region past the source edge reads");
    let VoxelData::F32(beyond) = beyond else {
        panic!("the read keeps the voxel format");
    };
    assert!(
        beyond.iter().all(|value| *value == 0.0),
        "a voxel whose taps all leave the volume holds the fill"
    );
}

#[test]
fn a_brick_at_the_channel_background_packs_empty() {
    fn channel(name: &str, intensity: Intensity, no_data: Option<NoData>) -> ChannelInfo {
        let info = ChannelInfo::declared(name, [1.0, 1.0, 1.0], intensity, VoxelFormat::U16);
        match no_data {
            Some(no_data) => info.with_no_data(no_data),
            None => info,
        }
    }
    let fill = NoData::U16(30);
    let level = Level::new(Extent::new(64, 64, 64), Spacing::new(1.0, 1.0, 1.0));
    let buried = Pyramid::new(
        vec![level],
        1,
        vec![
            channel("absent at the fill", Intensity::new(0.0, 100.0), Some(fill)),
            channel("under the fill", Intensity::new(40.0, 100.0), None),
            channel("at the fill", Intensity::new(30.0, 100.0), None),
        ],
        1,
        1.0,
        Calibration::Recorded,
    );
    let blank = BrickData::new(
        buried
            .channels()
            .iter()
            .map(|_| blank_brick(fill))
            .collect::<Vec<_>>(),
    );
    assert!(
        PackedBrick::of(&blank, &buried, ChannelSet::ALL).empty(),
        "a brick every channel carries its background through holds nothing"
    );
    assert!(
        fill_is_background(&buried, fill, ChannelSet::ALL),
        "the storage layer and the pack agree on the brick they filled"
    );
    let shown = Pyramid::new(
        buried.levels().to_vec(),
        1,
        vec![
            channel("absent at the fill", Intensity::new(0.0, 100.0), Some(fill)),
            channel("over the fill", Intensity::new(0.0, 100.0), None),
        ],
        1,
        1.0,
        Calibration::Recorded,
    );
    assert!(
        !fill_is_background(&shown, fill, ChannelSet::ALL),
        "a channel that shows the fill value fills the brick with content"
    );
    let indexed = Pyramid::new(
        buried.levels().to_vec(),
        1,
        vec![
            ChannelInfo::declared(
                "label",
                [0.0, 1.0, 0.0],
                Intensity::new(0.0, 3.0),
                VoxelFormat::U8,
            )
            .with_color_table(ColorTable::new(vec![
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            ])),
        ],
        1,
        1.0,
        Calibration::Recorded,
    );
    let background = BrickData::new(vec![blank_brick(NoData::U8(0))]);
    assert!(
        PackedBrick::of(&background, &indexed, ChannelSet::ALL).empty(),
        "an indexed channel packs a brick of its lowest index empty, even though that index sits above the channel minimum"
    );
    let mut indices = vec![0u8; (PADDED_SIDE as usize).pow(3)];
    indices[7] = 1;
    let marked = BrickData::new(vec![VoxelData::U8(indices)]);
    assert!(
        !PackedBrick::of(&marked, &indexed, ChannelSet::ALL).empty(),
        "one voxel above the background label makes the brick content"
    );
}

#[test]
fn anchors_the_default_transfer_on_the_value_the_volume_mostly_holds() {
    fn scene(name: &str, values: Vec<u8>) -> grana_scene::Scene {
        let extent = Extent::new(16, 16, 8);
        let channel = Channel::new(name, [1.0, 1.0, 1.0], VoxelData::U8(values));
        let volume = Volume::new(extent, Spacing::new(1.0, 1.0, 1.0), vec![channel]);
        let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
            .expect("the synthetic series opens");
        grana_scene::Scene::new(grana_scene::Dataset::synthetic(
            name,
            image.pyramid().clone(),
        ))
    }
    let extent = Extent::new(16, 16, 8);
    let count = extent.voxel_count() as usize;
    let mut bright = vec![200u8; count];
    let mut dark = vec![0u8; count];
    for z in 2..6u32 {
        for y in 4..12u32 {
            for x in 4..12u32 {
                bright[extent.index(x, y, z) as usize] = 40;
                dark[extent.index(x, y, z) as usize] = 220;
            }
        }
    }
    let bright = scene("bright", bright);
    let background = bright.dataset.channels()[0].statistics().background();
    assert!(
        (background - 200.0).abs() < 0.5,
        "the background is the value the volume mostly holds: {background}"
    );
    let stops = bright.channels[0]
        .transfer()
        .stops()
        .expect("a channel ramp");
    assert_eq!(
        stops.last().map(|stop| stop.opacity()),
        Some(0.0),
        "a bright background stays transparent at the top of its ramp"
    );
    assert_eq!(
        stops.first().map(|stop| stop.opacity()),
        Some(1.0),
        "the objects gain opacity towards the dark end"
    );
    let dark = scene("dark", dark);
    let stops = dark.channels[0].transfer().stops().expect("a channel ramp");
    assert_eq!(
        stops.first().map(|stop| stop.opacity()),
        Some(0.0),
        "a dark background stays transparent at the bottom of its ramp"
    );
    assert_eq!(
        stops.last().map(|stop| stop.opacity()),
        Some(1.0),
        "the objects gain opacity towards the bright end"
    );
}
