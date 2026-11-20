use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_archive::{Channel as WriteChannel, Fill, ImageSpec};
use grana_image::{Image, Source};
use grana_interop::{Detection, Edge, Lineage, geff};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_pipeline::Channels;
use grana_pipeline::export::{ChannelLayer, DatasetSpec, LabelLayer, cropped_levels};
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_volume::{
    Basis, Calibration, Channel, Extent, Intensity, Level, Region, Spacing, Volume, VolumeSeries,
    VoxelData, VoxelSemantics,
};
use grana_zarr::write_image;

use crate::support;
use crate::support::Temp;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn reads_a_tiled_slide_with_shared_jpeg_tables() {
    let image = grana_tiff::Image::open(&fixture("slide.tif"), 0).expect("the slide opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.level_count(), 3);
    assert_eq!(pyramid.channel_count(), 3);
    assert_eq!(pyramid.level(0).extent(), Extent::new(100, 70, 1));
    assert_eq!(pyramid.level(1).extent(), Extent::new(50, 35, 1));
    assert_eq!(pyramid.level(2).extent(), Extent::new(25, 18, 1));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 0.5));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 0.5));
    assert_eq!(
        pyramid.level(2).spacing(),
        Spacing::new(2.0, 2.0, 0.5),
        "a chained level keeps the whole power of its step where rounding shortened its extent"
    );
    let read = |level: usize, width: u32, height: u32| {
        let VoxelData::U8(values) = image
            .read_region(0, level, 0, 0..width, 0..height, 0..1)
            .expect("a slide level reads")
        else {
            panic!("a JPEG page stores bytes");
        };
        values
    };
    let base = read(0, 100, 70);
    for (x, y) in [
        (3u32, 3u32),
        (63, 0),
        (64, 0),
        (99, 63),
        (0, 63),
        (5, 68),
        (62, 69),
    ] {
        let index = (y * 100 + x) as usize;
        let wanted = (x * 2) % 251;
        let held = u32::from(base[index]);
        assert!(
            held.abs_diff(wanted) <= 12,
            "the tile at {x},{y} reads {held} instead of {wanted}"
        );
    }
    assert_eq!(
        base[(66 * 100 + 80) as usize],
        0,
        "a zero length tile reads blank instead of failing the slide"
    );
    let second = read(1, 50, 35);
    let held = u32::from(second[(10 * 50 + 20) as usize]);
    assert!(held.abs_diff(48) <= 12, "the chained level reads {held}");
    let third = read(2, 25, 18);
    let held = u32::from(third[(4 * 25 + 6) as usize]);
    assert!(held.abs_diff(28) <= 12, "the coarsest level reads {held}");
}

#[test]
fn exports_a_tiled_pyramid_ome_tiff() {
    let temp = Temp::new("suite-ome-tiff-archive");
    let levels = [
        Level::new(Extent::new(300, 270, 3), Spacing::new(0.2, 0.2, 0.5)),
        Level::new(Extent::new(150, 135, 3), Spacing::new(0.4, 0.4, 0.5)),
        Level::new(Extent::new(75, 68, 3), Spacing::new(0.8, 0.8, 0.5)),
    ];
    let channels = [WriteChannel {
        name: "signal",
        tint: [0.2, 0.7, 1.0],
        data: Intensity::new(0.0, 4096.0),
        window: Intensity::new(0.0, 4096.0),
    }];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        let scale = 1u32 << block.level;
        let origin = block.origin;
        let extent = block.extent;
        let mut values = vec![0.0f32; block.voxels()];
        for z in 0..extent[2] {
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let index = x as usize
                        + extent[0] as usize * (y as usize + extent[1] as usize * z as usize);
                    values[index] = ((origin[0] + x) * scale
                        + 1000 * (origin[1] + y) * scale
                        + 1000000 * (origin[2] + z)) as f32;
                }
            }
        }
        Ok(VoxelData::F32(values))
    };
    let mut progress = |_, _| {};
    let path = temp.path().join("pyramid.ome.tif");
    let written = grana_tiff::write_archive(
        &path,
        &mut ImageSpec {
            name: "pyramid",
            channels: &channels,
            levels: &levels,
            placement: [1.5, -2.0, 3.0],
            frames: 2,
            interval: Some(0.5),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Assumed,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the pyramid archive writes");
    assert_eq!(written.channels(), 1);
    assert_eq!(written.frames(), 2);
    assert_eq!(written.levels(), 3);
    assert_eq!(
        std::fs::metadata(&path).expect("the archive exists").len(),
        written.bytes(),
        "the archive reports the bytes it wrote"
    );
    let image = grana_tiff::Image::open(&path, 0).expect("the archive opens");
    let bytes = std::fs::read(&path).expect("the archive reads back");
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 20)]).into_owned();
    let data = text.find("<TiffData").expect("the archive lists its tiles");
    let plane = text.find("<Plane ").expect("the archive lists its planes");
    assert!(
        data < plane,
        "the OME-XML lists every TiffData before the first Plane"
    );
    assert_eq!(image.pyramid().level_count(), 3);
    assert_eq!(
        image.pyramid().calibration(),
        Calibration::Assumed,
        "the archive keeps the calibration it was given"
    );
    assert_eq!(
        image.pyramid().channels()[0].intensity(),
        Intensity::new(0.0, 4096.0),
        "the archive keeps the intensity range it was given"
    );
    assert_eq!(
        image.pyramid().channels()[0].window(),
        Some(Intensity::new(0.0, 4096.0)),
        "the archive keeps the display window it was given"
    );
    for (axis, value) in image.pyramid().placement().into_iter().enumerate() {
        let expected = [1.5, -2.0, 3.0][axis];
        assert!(
            (value - expected).abs() <= 1.0e-5,
            "the archive keeps its world placement on axis {axis}: {value} against {expected}"
        );
        assert!(
            image.pyramid().level(0).center()[axis].abs() <= 1.0e-5,
            "the archive centres its levels and records the placement once"
        );
    }
    for (index, level) in levels.iter().enumerate() {
        assert_eq!(
            image.pyramid().level(index).extent(),
            level.extent(),
            "level {index} keeps its extent"
        );
        let held = image.pyramid().level(index).spacing();
        let wanted = level.spacing();
        assert!(
            (held.x() - wanted.x()).abs() <= 1.0e-5
                && (held.y() - wanted.y()).abs() <= 1.0e-5
                && (held.z() - wanted.z()).abs() <= 1.0e-5,
            "level {index} keeps its spacing: {held:?} against {wanted:?}"
        );
    }
    let coarse = image
        .read_region(1, 2, 0, 0..75, 0..68, 0..3)
        .expect("the coarsest level reads");
    let VoxelData::F32(values) = coarse else {
        panic!("the archive stores float voxels");
    };
    assert_eq!(values[0], 0.0, "the first voxel of the coarsest level");
    assert_eq!(
        values[5 + 75 * 4],
        (5 * 4 + 1000 * 4 * 4) as f32,
        "a coarsest level voxel keeps its frame and scale"
    );

    let extent = Extent::new(24, 16, 8);
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 1.0, 1.0],
            VoxelData::U16(vec![7u16; extent.voxel_count() as usize]),
        )],
    );
    let image = Arc::new(
        Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
            .expect("the image opens"),
    );
    let mut progress = |_, _| {};
    let summary = grana_pipeline::write_dataset(
        DatasetSpec {
            path: temp.path().join("export.ome.tif"),
            name: "export".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 255.0),
                window: Intensity::new(0.0, 255.0),
            }],
            levels: image.pyramid().levels().to_vec(),
            origins: vec![[0, 0, 0]; image.pyramid().level_count()],
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: Vec::new(),
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("the dataset exports as an archive");
    assert_eq!(summary.channels(), 1);
    assert_eq!(summary.frames(), 1);
    let exported = grana_tiff::Image::open(summary.path(), 0).expect("the exported archive opens");
    let data = exported
        .read_region(0, 0, 0, 0..24, 0..16, 0..8)
        .expect("the exported channel reads");
    let VoxelData::U16(values) = data else {
        panic!("the exported channel keeps the voxel type of its dataset");
    };
    assert!(
        values.iter().all(|value| *value == 7),
        "the exported channel keeps its voxels"
    );

    let source = image.pyramid().level(0);
    let cropped = cropped_levels(
        Region::axis_aligned(
            Vec3::from(source.corner([4.0, 2.0, 1.0])),
            Vec3::from(source.corner([12.0, 10.0, 5.0])),
        ),
        image.pyramid().levels(),
    )
    .expect("the region crops the levels");
    let mut progress = |_, _| {};
    let summary = grana_pipeline::write_dataset(
        DatasetSpec {
            path: temp.path().join("crop.ome.tif"),
            name: "crop".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 255.0),
                window: Intensity::new(0.0, 255.0),
            }],
            levels: cropped.levels,
            origins: cropped.origins,
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: Vec::new(),
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("the region exports as an archive");
    let reopened = grana_tiff::Image::open(summary.path(), 0).expect("the cropped archive opens");
    let expected = [0, 1, 2].map(|axis| {
        0.5 * (source.corner([4.0, 2.0, 1.0])[axis] + source.corner([12.0, 10.0, 5.0])[axis])
    });
    let placement = reopened.pyramid().placement();
    for (axis, value) in reopened.pyramid().level(0).center().into_iter().enumerate() {
        let world = value + placement[axis];
        assert!(
            (world - expected[axis]).abs() <= 1.0e-3,
            "the cropped archive keeps the world centre of its box on axis {axis}: {world} against {}",
            expected[axis]
        );
    }

    let flipped = Level::placed(
        Extent::new(24, 16, 8),
        Basis::new([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
        [0.0; 3],
    );
    let mut progress = |_, _| {};
    let mirrored = grana_pipeline::write_dataset(
        DatasetSpec {
            path: temp.path().join("flipped.ome.tif"),
            name: "flipped".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 255.0),
                window: Intensity::new(0.0, 255.0),
            }],
            levels: vec![flipped],
            origins: vec![[0, 0, 0]],
            placement: [0.0; 3],
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: Vec::new(),
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("a mirrored basis writes an OME-TIFF");
    let mirrored = grana_tiff::Image::open(mirrored.path(), 0).expect("the mirrored archive opens");
    assert!(
        mirrored.pyramid().level(0).basis().column(0)[0] < 0.0,
        "an OME-TIFF carries a mirrored axis in its grana annotation"
    );

    let shallow = [
        Level::new(Extent::new(24, 16, 8), Spacing::new(1.0, 1.0, 1.0)),
        Level::new(Extent::new(12, 8, 4), Spacing::new(2.0, 2.0, 1.0)),
    ];
    let mut progress = |_, _| {};
    let refused = grana_pipeline::write_dataset(
        DatasetSpec {
            path: temp.path().join("shallow.ome.tif"),
            name: "shallow".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 255.0),
                window: Intensity::new(0.0, 255.0),
            }],
            levels: shallow.to_vec(),
            origins: vec![[0, 0, 0]; 2],
            placement: [0.0; 3],
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: Vec::new(),
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    );
    assert!(
        refused.is_err_and(|detail| detail.contains("one depth")),
        "an OME-TIFF refuses a pyramid whose coarse level stores fewer planes"
    );

    let wide = Extent::new(16, 8, 4);
    let values = (0..wide.voxel_count())
        .map(|index| 16_777_217u32 + index as u32)
        .collect::<Vec<_>>();
    let volume = Volume::new(
        wide,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 1.0, 1.0],
            VoxelData::U32(values.clone()),
        )],
    );
    let wide_image = Arc::new(
        Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
            .expect("the wide image opens"),
    );
    for name in ["wide.ome.tif", "wide.ome.zarr"] {
        let mut progress = |_, _| {};
        let summary = grana_pipeline::write_dataset(
            DatasetSpec {
                path: temp.path().join(name),
                name: "wide".to_string(),
                channels: vec![ChannelLayer {
                    index: 0,
                    name: "signal".to_string(),
                    tint: [1.0, 1.0, 1.0],
                    data: Intensity::new(0.0, 20_000_000.0),
                    window: Intensity::new(0.0, 20_000_000.0),
                }],
                levels: wide_image.pyramid().levels().to_vec(),
                origins: vec![[0, 0, 0]; wide_image.pyramid().level_count()],
                placement: [0.0; 3],
                frames: 1,
                interval: 1.0,
                calibration: grana_volume::Calibration::Recorded,
                drift: Vec::new(),
                labels: Vec::new(),
            },
            &Channels::stored(Arc::clone(&wide_image)),
            &AtomicBool::new(false),
            &mut progress,
        )
        .expect("the wide dataset exports");
        let read = if name.ends_with(".zarr") {
            grana_zarr::Image::open(summary.path())
                .expect("the wide archive opens")
                .read_region(0, 0, 0, 0..16, 0..8, 0..4)
                .expect("the wide channel reads")
        } else {
            grana_tiff::Image::open(summary.path(), 0)
                .expect("the wide picture opens")
                .read_region(0, 0, 0, 0..16, 0..8, 0..4)
                .expect("the wide plane reads")
        };
        let VoxelData::U32(stored) = read else {
            panic!("{name} keeps the voxel type of its dataset");
        };
        assert_eq!(
            stored, values,
            "{name} keeps samples beyond the float mantissa"
        );
    }
}

#[test]
fn writes_a_sixteen_bit_ome_tiff() {
    let temp = Temp::new("suite-tiff-write");
    let path = temp.path().join("picture.ome.tif");
    let size = [5u32, 4];
    let mut pixels = vec![0u16; size[0] as usize * size[1] as usize * 3];
    for (index, value) in pixels.iter_mut().enumerate() {
        *value = (index as u16).wrapping_mul(1237).wrapping_add(3);
    }
    let levels = [Level::new(
        Extent::new(size[0], size[1], 1),
        Spacing::new(0.5, 0.5, 0.5),
    )];
    let channels = ["Red", "Green", "Blue"].map(|name| WriteChannel {
        name,
        tint: match name {
            "Red" => [1.0, 0.0, 0.0],
            "Green" => [0.0, 1.0, 0.0],
            _ => [0.0, 0.0, 1.0],
        },
        data: Intensity::new(0.0, 1.0),
        window: Intensity::new(0.0, 1.0),
    });
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        let origin = block.origin;
        let length = block.extent;
        let mut values = vec![0u16; block.voxels()];
        for y in 0..length[1] {
            for x in 0..length[0] {
                let index = (origin[0] + x) as usize + size[0] as usize * (origin[1] + y) as usize;
                values[(x + length[0] * y) as usize] = pixels[index * 3 + block.channel];
            }
        }
        Ok(VoxelData::U16(values))
    };
    let mut progress = |_, _| {};
    grana_tiff::write_archive(
        &path,
        &mut ImageSpec {
            name: "figure & frame",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(2.0),
            sample: grana_volume::VoxelFormat::U16,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the picture writes");
    let bytes = std::fs::read(&path).expect("the picture reads as bytes");
    let metadata = String::from_utf8_lossy(&bytes);
    assert!(
        metadata.contains("PhysicalSizeX=\"0.5\"") && metadata.contains("PhysicalSizeY=\"0.5\""),
        "the picture records its physical pixel size"
    );
    assert!(
        metadata.contains("TimeIncrement=\"2\""),
        "the picture records its frame interval"
    );
    assert!(
        metadata.contains("Type=\"uint16\""),
        "the picture declares its sixteen bit samples"
    );
    let image = grana_tiff::Image::open(&path, 0).expect("the written picture opens");
    assert_eq!(image.name(), "figure & frame");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 3);
    assert_eq!(pyramid.frame_count(), 1);
    assert_eq!(pyramid.level(0).extent(), Extent::new(5, 4, 1));
    assert_eq!(pyramid.channels()[0].name(), "Red");
    assert_eq!(pyramid.channels()[1].tint(), [0.0, 1.0, 0.0]);
    for channel in 0..3 {
        let data = image
            .read_region(0, 0, channel, 0..5, 0..4, 0..1)
            .expect("the written plane reads");
        let VoxelData::U16(values) = data else {
            panic!("the written picture stores sixteen bit voxels");
        };
        for y in 0..4u32 {
            for x in 0..5u32 {
                let index = (x + 5 * y) as usize;
                assert_eq!(
                    values[index],
                    pixels[index * 3 + channel],
                    "texel {x},{y} of channel {channel}"
                );
            }
        }
    }
}

#[test]
fn writes_a_sharded_ome_zarr_level() {
    let temp = Temp::new("suite-zarr-shard");
    let path = temp.path().join("sharded.ome.zarr");
    let extent = Extent::new(300, 270, 40);
    let levels = [Level::new(extent, Spacing::new(0.5, 0.5, 1.0))];
    let channels = [WriteChannel {
        name: "signal",
        tint: [1.0, 1.0, 1.0],
        data: Intensity::new(0.0, 1.0),
        window: Intensity::new(0.0, 1.0),
    }];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        let origin = block.origin;
        let length = block.extent;
        let mut values = vec![0.0f32; block.voxels()];
        for z in 0..length[2] {
            for y in 0..length[1] {
                for x in 0..length[0] {
                    let index = x as usize
                        + length[0] as usize * (y as usize + length[1] as usize * z as usize);
                    values[index] = (origin[0] + x) as f32
                        + 1000.0 * (origin[1] + y) as f32
                        + 1_000_000.0 * (origin[2] + z) as f32;
                }
            }
        }
        Ok(VoxelData::F32(values))
    };
    let mut progress = |_, _| {};
    write_image(
        &path,
        &mut ImageSpec {
            name: "sharded",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the sharded image writes");
    let metadata = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(path.join("0").join("zarr.json")).expect("the level metadata reads"),
    )
    .expect("the level metadata parses");
    assert_eq!(
        metadata["chunk_grid"]["configuration"]["chunk_shape"],
        serde_json::json!([1, 1, 32, 256, 256]),
        "a level stores shards of four by four by two subchunks"
    );
    assert_eq!(
        metadata["codecs"][0]["name"],
        serde_json::json!("sharding_indexed"),
        "a level indexes its subchunks through a shard"
    );
    assert_eq!(
        metadata["codecs"][0]["configuration"]["chunk_shape"],
        serde_json::json!([1, 1, 16, 64, 64]),
        "a shard holds subchunks of sixty four by sixty four by sixteen voxels"
    );
    let shards = files(&path.join("0")) - 1;
    assert_eq!(
        shards, 8,
        "a level writes one file per shard, not per chunk"
    );
    let image = grana_zarr::Image::open(&path).expect("the sharded image opens");
    let data = image
        .read_region(0, 0, 0, 250..262, 250..258, 30..34)
        .expect("a region across every shard boundary reads");
    let VoxelData::F32(values) = data else {
        panic!("the sharded image stores float voxels");
    };
    for z in 30..34u32 {
        for y in 250..258u32 {
            for x in 250..262u32 {
                let index = (x - 250 + 12 * (y - 250 + 8 * (z - 30))) as usize;
                assert_eq!(
                    values[index],
                    x as f32 + 1000.0 * y as f32 + 1_000_000.0 * z as f32,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }
}

fn files(directory: &std::path::Path) -> usize {
    std::fs::read_dir(directory)
        .expect("a stored directory lists")
        .flatten()
        .map(|entry| match entry.path().is_dir() {
            true => files(&entry.path()),
            false => 1,
        })
        .sum()
}

fn write_zarr(root: &std::path::Path) {
    let levels = [
        Level::new(Extent::new(24, 16, 8), Spacing::new(0.5, 0.5, 1.0)),
        Level::new(Extent::new(12, 8, 4), Spacing::new(1.0, 1.0, 2.0)),
    ];
    let channels = [
        WriteChannel {
            name: "DAPI",
            tint: [0.0, 0.0, 1.0],
            data: Intensity::new(0.0, 4096.0),
            window: Intensity::new(0.0, 2048.0),
        },
        WriteChannel {
            name: "GFP",
            tint: [0.0, 1.0, 0.0],
            data: Intensity::new(0.0, 1024.0),
            window: Intensity::new(64.0, 768.0),
        },
    ];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        let scale = 1u32 << block.level;
        let origin = block.origin;
        let extent = block.extent;
        let mut values = vec![0.0f32; block.voxels()];
        for z in 0..extent[2] {
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let index = x as usize
                        + extent[0] as usize * (y as usize + extent[1] as usize * z as usize);
                    let position = [
                        (origin[0] + x) * scale,
                        (origin[1] + y) * scale,
                        (origin[2] + z) * scale,
                    ];
                    values[index] = sample(block.channel, block.frame, position) as f32;
                }
            }
        }
        Ok(VoxelData::F32(values))
    };
    let mut progress = |_, _| {};
    write_image(
        root,
        &mut ImageSpec {
            name: "fixture",
            channels: &channels,
            levels: &levels,
            placement: [1.5, -2.0, 3.0],
            frames: 2,
            interval: Some(1.5),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Assumed,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the tiny image writes");
}

fn sample(channel: usize, frame: u32, position: [u32; 3]) -> u32 {
    channel as u32 * 100_000 + frame * 10_000 + position[2] * 500 + position[1] * 10 + position[0]
}

#[test]
fn writes_and_reads_a_tiny_ome_zarr() {
    let temp = Temp::new("suite-zarr");
    let path = temp.path().join("tiny.ome.zarr");
    write_zarr(&path);
    let metadata = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(path.join("zarr.json")).expect("the group metadata reads"),
    )
    .expect("the group metadata parses");
    let ome = &metadata["attributes"]["ome"];
    assert_eq!(
        ome["omero"]["channels"][1]["window"],
        serde_json::json!({
            "min": 0.0,
            "max": 1024.0,
            "start": 64.0,
            "end": 768.0,
        }),
        "the writer keeps the intensity range and the display window apart"
    );
    assert_eq!(
        ome["version"],
        serde_json::json!("0.5"),
        "an axis aligned image declares the released schema"
    );
    let multiscales = &ome["multiscales"][0];
    assert!(
        multiscales.get("coordinateSystems").is_none(),
        "the released schema names its axes on the multiscales entry"
    );
    assert_eq!(
        multiscales["axes"],
        serde_json::json!([
            { "name": "t", "type": "time", "unit": "second" },
            { "name": "c", "type": "channel" },
            { "name": "z", "type": "space", "unit": "micrometer" },
            { "name": "y", "type": "space", "unit": "micrometer" },
            { "name": "x", "type": "space", "unit": "micrometer" },
        ])
    );
    for (level, dataset) in multiscales["datasets"]
        .as_array()
        .expect("the levels are listed")
        .iter()
        .enumerate()
    {
        assert_eq!(dataset["path"], serde_json::json!(level.to_string()));
        let transforms = dataset["coordinateTransformations"]
            .as_array()
            .expect("a level lists its transforms");
        assert_eq!(
            transforms
                .iter()
                .map(|transform| transform["type"].as_str())
                .collect::<Vec<_>>(),
            [Some("scale"), Some("translation")],
            "the released schema places a level with a scale and a translation"
        );
        assert!(transforms.iter().all(|transform| {
            transform["scale"]
                .as_array()
                .or(transform["translation"].as_array())
                .is_some_and(|values| values.len() == 5)
        }));
    }

    let image = grana_zarr::Image::open(&path).expect("the written image opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 2);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(
        pyramid.channels()[0].intensity(),
        Intensity::new(0.0, 4096.0)
    );
    assert_eq!(
        pyramid.channels()[1].window(),
        Some(Intensity::new(64.0, 768.0)),
        "the display window reads back beside the intensity range it was written with"
    );
    assert_eq!(
        pyramid.channels()[1].intensity(),
        Intensity::new(0.0, 1024.0)
    );
    assert_eq!(pyramid.level(0).extent(), Extent::new(24, 16, 8));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 1.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(12, 8, 4));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 2.0));
    assert_eq!(
        pyramid.calibration(),
        Calibration::Assumed,
        "the written image keeps the calibration it was given"
    );

    let data = image
        .read_region(1, 0, 1, 2..6, 1..4, 0..2)
        .expect("the written region reads");
    let VoxelData::F32(values) = data else {
        panic!("the written image stores float voxels");
    };
    assert_eq!(values.len(), 4 * 3 * 2);
    for z in 0..2u32 {
        for y in 1..4u32 {
            for x in 2..6u32 {
                let index = ((z * 3 + (y - 1)) * 4 + (x - 2)) as usize;
                assert_eq!(
                    values[index],
                    sample(1, 1, [x, y, z]) as f32,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }

    let identity = image
        .identity()
        .expect("a local dataset names the metadata it was read from");
    assert_eq!(
        grana_zarr::Image::open(&path)
            .expect("the written image reopens")
            .identity(),
        Some(identity),
        "a stored dataset keeps its identity while its levels stay put"
    );
    let stamps = image
        .stamps(0, 0, 0, 0..24, 0..16, 0..8)
        .expect("the chunk stamps read");
    assert!(stamps.is_some(), "a local dataset stamps its chunks");
    let chunks = chunk_files(&path);
    assert!(!chunks.is_empty(), "a written level stores its chunks");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    for chunk in &chunks {
        std::fs::File::options()
            .write(true)
            .open(chunk)
            .expect("the stored chunk opens for rewriting")
            .set_modified(later)
            .expect("the stored chunk restamps");
    }
    let reopened = grana_zarr::Image::open(&path).expect("the rewritten image reopens");
    assert_eq!(
        reopened.identity(),
        Some(identity),
        "the metadata digest cannot see a chunk rewritten in place"
    );
    assert_ne!(
        reopened
            .stamps(0, 0, 0, 0..24, 0..16, 0..8)
            .expect("the chunk stamps reread"),
        stamps,
        "a rewritten chunk restamps every tile composed from it"
    );

    let frames = temp.path().join("frames.ome.zarr");
    let extent = Extent::new(64, 64, 4);
    let levels = [Level::new(extent, Spacing::new(0.5, 0.5, 1.0))];
    let channels = [WriteChannel {
        name: "signal",
        tint: [1.0, 1.0, 1.0],
        data: Intensity::new(0.0, 255.0),
        window: Intensity::new(0.0, 255.0),
    }];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        Ok(VoxelData::F32(vec![
            block.level as f32
                + block.frame as f32 * 100.0;
            block.voxels()
        ]))
    };
    let mut progress = |_, _| {};
    write_image(
        &frames,
        &mut ImageSpec {
            name: "frames",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 2,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the frame fixture writes");
    let derived = Image::open_with_cache(
        &Source::file_series(frames, 0),
        Some(temp.path().join("derived")),
    )
    .expect("the derived pyramid opens");
    assert_eq!(derived.pyramid().level_count(), 2);
    let composed = |frame: u32| {
        derived
            .read_region(
                frame,
                1,
                0,
                &[0..32, 0..32, 0..2],
                &grana_warp::Warp::IDENTITY,
            )
            .expect("the derived level composes")
            .value(0)
    };
    assert_eq!(
        composed(0),
        0.0,
        "the first derived voxel averages the first frame"
    );
    assert_eq!(
        composed(1),
        100.0,
        "the derived level composes the frame it is asked for"
    );
}

#[test]
fn round_trips_a_rotated_basis_through_ome_tiff() {
    let temp = Temp::new("suite-ome-tiff-basis");
    let basis = Basis::new([[0.0, 0.5, 0.0], [0.25, 0.0, 0.0], [0.0, 0.0, 1.5]]);
    let coarse = Basis::new([[0.0, 1.0, 0.0], [0.5, 0.0, 0.0], [0.0, 0.0, 3.0]]);
    let levels = [
        Level::placed(Extent::new(32, 24, 4), basis, [-2.0, 1.0, -3.0]),
        Level::placed(Extent::new(16, 12, 4), coarse, [-1.5, 0.5, -2.5]),
    ];
    let channels = [WriteChannel {
        name: "signal",
        tint: [1.0, 1.0, 1.0],
        data: Intensity::new(0.0, 1.0),
        window: Intensity::new(0.0, 1.0),
    }];
    let fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        Ok(VoxelData::F32(vec![block.level as f32; block.voxels()]))
    };
    let mut progress = |_, _| {};
    let path = temp.path().join("rotated.ome.tif");
    grana_tiff::write_archive(
        &path,
        &mut ImageSpec {
            name: "rotated",
            channels: &channels,
            levels: &levels,
            placement: [1.5, -2.0, 3.0],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: Calibration::Inferred,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("a rotated basis writes an OME-TIFF");
    let image = grana_tiff::Image::open(&path, 0).expect("the rotated archive opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.level_count(), 2);
    for (axis, value) in pyramid.placement().into_iter().enumerate() {
        assert!(
            (value - [1.5, -2.0, 3.0][axis]).abs() <= 1.0e-5,
            "the archive keeps its placement on axis {axis}"
        );
    }
    for (index, level) in levels.iter().enumerate() {
        let held = pyramid.level(index);
        assert_eq!(
            held.extent(),
            level.extent(),
            "level {index} keeps its extent"
        );
        assert!(
            held.basis().close(level.basis()),
            "level {index} keeps its full basis: {:?} against {:?}",
            held.basis().matrix(),
            level.basis().matrix()
        );
        let (read, written) = (held.origin(), level.origin());
        assert!(
            (0..3).all(|axis| (read[axis] - written[axis]).abs() <= 1.0e-5),
            "level {index} keeps its origin: {read:?} against {written:?}"
        );
    }

    let plain = temp.path().join("uncalibrated.ome.tif");
    let uncalibrated = [Level::new(
        Extent::new(8, 6, 4),
        Spacing::new(1.0, 1.0, 1.0),
    )];
    let mut progress = |_, _| {};
    grana_tiff::write_archive(
        &plain,
        &mut ImageSpec {
            name: "plain",
            channels: &channels,
            levels: &uncalibrated,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: Calibration::Uncalibrated,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("an uncalibrated basis writes an OME-TIFF");
    let bytes = std::fs::read(&plain).expect("the uncalibrated archive reads back");
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 16)]).into_owned();
    assert!(
        !text.contains("PhysicalSizeX") && !text.contains("PositionX"),
        "an uncalibrated archive writes no physical size and no plane position"
    );
    let reopened = grana_tiff::Image::open(&plain, 0).expect("the uncalibrated archive opens");
    assert_eq!(
        reopened.pyramid().calibration(),
        Calibration::Uncalibrated,
        "an uncalibrated archive reads back uncalibrated"
    );

    let unmeasured = temp.path().join("uncalibrated.ome.zarr");
    let mut progress = |_, _| {};
    write_image(
        &unmeasured,
        &mut ImageSpec {
            name: "plain",
            channels: &channels,
            levels: &uncalibrated,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: Calibration::Uncalibrated,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("an uncalibrated basis writes an OME-Zarr");
    let attributes = std::fs::read_to_string(unmeasured.join("zarr.json"))
        .expect("the uncalibrated group metadata reads back");
    let metadata: serde_json::Value =
        serde_json::from_str(&attributes).expect("the group metadata is json");
    let text = metadata.to_string();
    assert!(
        !text.contains("micrometer"),
        "an uncalibrated archive declares no physical unit"
    );
    let scale =
        metadata["attributes"]["ome"]["multiscales"][0]["datasets"][0]["coordinateTransformations"]
            [0]["scale"]
            .as_array()
            .expect("the level declares its scale");
    assert_eq!(
        scale[2].as_f64(),
        Some(1.0),
        "an uncalibrated archive records the downsample factor of its finest level"
    );
}

fn write_derived_zarr(root: &std::path::Path, value: f32) {
    let levels = [Level::new(
        Extent::new(128, 128, 16),
        Spacing::new(1.0, 1.0, 1.0),
    )];
    let channels = [WriteChannel {
        name: "signal",
        tint: [1.0, 1.0, 1.0],
        data: Intensity::new(0.0, 255.0),
        window: Intensity::new(0.0, 255.0),
    }];
    let fill = move |block: grana_archive::Block| -> Result<VoxelData, String> {
        let mut values = Vec::with_capacity(block.voxels());
        for z in 0..block.extent[2] {
            for y in 0..block.extent[1] {
                for x in 0..block.extent[0] {
                    values.push(gradient(
                        value,
                        [
                            block.origin[0] + x,
                            block.origin[1] + y,
                            block.origin[2] + z,
                        ],
                    ));
                }
            }
        }
        Ok(VoxelData::F32(values))
    };
    let mut progress = |_, _| {};
    write_image(
        root,
        &mut ImageSpec {
            name: "remote",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the remote fixture writes");
}

fn derived_values(image: &Image, ranges: &[std::ops::Range<u32>; 3]) -> Vec<f32> {
    derived_values_at(image, 1, ranges)
}

fn derived_values_at(image: &Image, level: usize, ranges: &[std::ops::Range<u32>; 3]) -> Vec<f32> {
    let data = image
        .read_region(0, level, 0, ranges, &grana_warp::Warp::IDENTITY)
        .expect("the derived level composes");
    let VoxelData::F32(values) = data else {
        panic!("the fixture stores float voxels");
    };
    values
}

fn gradient(value: f32, index: [u32; 3]) -> f32 {
    value + (index[0] * 3 + index[1] * 5 + index[2] * 7) as f32
}

fn derived_gradient(value: f32, index: [u32; 3]) -> f32 {
    value + (index[0] * 6 + index[1] * 10 + index[2] * 14) as f32 + 7.5
}

#[test]
fn refuses_a_range_that_the_server_answers_with_the_whole_object() {
    let temp = Temp::new("suite-range");
    let payload = vec![7u8; 4096];
    std::fs::write(temp.path().join("data.bin"), &payload).expect("the payload writes");
    let server = support::Server::serve(temp.path());
    let url = format!("{}data.bin", server.url());
    let client = grana_net::Client::new();
    let error = client
        .get(&url, Some(grana_net::Span::Between(16, 31)))
        .expect_err("a whole response never passes for a byte range");
    assert!(
        matches!(error, grana_net::Error::Range { .. }),
        "the client reports the unusable range: {error}"
    );
    let whole = client
        .get(&url, None)
        .expect("the whole object reads")
        .expect("the object is present");
    assert_eq!(whole, payload, "the whole read keeps every byte");
}

#[test]
fn caches_the_derived_levels_of_a_remote_ome_zarr() {
    let temp = Temp::new("suite-zarr-remote");
    let path = temp.path().join("remote.ome.zarr");
    let cache = temp.path().join("cache");
    let ranges = [0..32u32, 0..32, 0..8];
    write_derived_zarr(&path, 64.0);
    let server = support::Server::serve(&path);
    let source = Source::remote(server.url().to_string());
    let first =
        Image::open_with_cache(&source, Some(cache.clone())).expect("the remote image opens");
    assert_derived(&first, &ranges, 64.0);
    assert!(
        server.downloads() > 0,
        "the first read downloads the stored shards"
    );
    drop(first);
    server.reset();
    let cached =
        Image::open_with_cache(&source, Some(cache.clone())).expect("the remote image reopens");
    assert_derived(&cached, &ranges, 64.0);
    assert_eq!(
        server.downloads(),
        0,
        "a reopened image reads its derived level from the disk cache"
    );
    drop(cached);
    write_derived_zarr(&path, 200.0);
    server.reset();
    let rewritten =
        Image::open_with_cache(&source, Some(cache)).expect("the rewritten image opens");
    assert_derived(&rewritten, &ranges, 200.0);
    assert!(
        server.downloads() > 0,
        "a rewritten shard invalidates the disk cache"
    );
}

fn assert_derived(image: &Image, ranges: &[std::ops::Range<u32>; 3], value: f32) {
    let read = derived_values(image, ranges);
    let width = ranges[0].end - ranges[0].start;
    let height = ranges[1].end - ranges[1].start;
    assert_eq!(
        read.len(),
        (width * height * (ranges[2].end - ranges[2].start)) as usize,
        "the derived level holds its request"
    );
    for z in ranges[2].clone() {
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let index = ((x - ranges[0].start)
                    + width * ((y - ranges[1].start) + height * (z - ranges[2].start)))
                    as usize;
                let expected = derived_gradient(value, [x, y, z]);
                assert!(
                    (read[index] - expected).abs() < 1.0e-3,
                    "the derived voxel {x} {y} {z} averages its block: {} against {expected}",
                    read[index]
                );
            }
        }
    }
    let ranges = [0..4u32, 0..4, 0..2];
    let read = derived_values_at(image, 2, &ranges);
    for z in ranges[2].clone() {
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let index = (x + 4 * (y + 4 * z)) as usize;
                let expected = value + (x * 12 + y * 20 + z * 28) as f32 + 22.5;
                assert!(
                    (read[index] - expected).abs() < 1.0e-3,
                    "the derived level 2 voxel {x} {y} {z} averages its child block: {} against {expected}",
                    read[index]
                );
            }
        }
    }
}

fn chunk_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut folders = vec![root.join("0")];
    let mut chunks = Vec::new();
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("a stored level lists") {
            let entry = entry.expect("a stored entry reads");
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = entry.file_name();
            if name != "zarr.json" && name != ".zarray" && name != ".zgroup" {
                chunks.push(path);
            }
        }
    }
    chunks
}

#[test]
fn exports_a_region_of_the_channels_and_labels() {
    let temp = Temp::new("suite-zarr-crop");
    let path = temp.path().join("crop.ome.zarr");
    let extent = Extent::new(16, 12, 8);
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                values[extent.index(x, y, z) as usize] = (x + 10 * y + 100 * z) as u16;
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 1.0, 1.0],
            VoxelData::U16(values),
        )],
    );
    let image = Arc::new(
        Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
            .expect("the image opens"),
    );
    let source_level = image.pyramid().level(0);
    let levels = image.pyramid().levels().to_vec();
    let region = Region::axis_aligned(
        Vec3::from(source_level.corner([4.0, 2.0, 1.0])),
        Vec3::from(source_level.corner([12.0, 10.0, 5.0])),
    );
    let cropped = cropped_levels(region, &levels).expect("the region crops the levels");
    assert_eq!(cropped.levels.len(), 1);
    assert_eq!(cropped.levels[0].extent(), Extent::new(8, 8, 4));
    assert_eq!(cropped.origins[0], [4, 2, 1]);
    let whole = cropped_levels(Region::none(), &levels).expect("no region keeps the levels");
    assert_eq!(whole.levels[0].extent(), extent);
    assert_eq!(whole.origins[0], [0, 0, 0]);

    let center = source_level.voxel_center([5.0, 3.0, 2.0]);
    let spots = SpotSet::new(
        0,
        vec![Spot::new(center, 0.9, 1.0)],
        IntensityTable::new(1, vec![SampleStatistics::new(1.0, 1.0, 1.0, 0.0, 1.0)]),
    );
    let mut label_scene = Scene::new(Dataset::synthetic("crop", image.pyramid().clone()));
    let label_set = label_scene.add_object(ObjectKind::Spots, 0);
    label_scene.install(label_set, 0, ObjectFrame::from_spots(spots));
    label_scene
        .objects
        .get_mut(label_set)
        .expect("the label set exists")
        .set_name("spot");
    let label = LabelLayer::of(
        label_scene
            .objects
            .get(label_set)
            .expect("the label set exists"),
        &cropped.levels,
    )
    .expect("the label layer builds")
    .expect("the label set holds labelled objects");
    let channels = Channels::stored(Arc::clone(&image));
    let mut progress = |_, _| {};
    grana_pipeline::write_dataset(
        DatasetSpec {
            path: path.clone(),
            name: "crop".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 1000.0),
                window: Intensity::new(0.0, 1000.0),
            }],
            levels: cropped.levels,
            origins: cropped.origins,
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: vec![label],
        },
        &channels,
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("the region exports");
    let written = grana_zarr::Image::open(&path).expect("the cropped image opens");
    let info = &written.pyramid().channels()[0];
    assert_eq!(
        (info.intensity().min(), info.intensity().max()),
        (0.0, 1000.0),
        "the exported channel keeps the intensities its voxels span"
    );
    assert!(
        info.intensity().span() > 0.0,
        "the exported channel keeps a display window with a positive span"
    );
    let level = written.pyramid().level(0);
    assert_eq!(level.extent(), Extent::new(8, 8, 4));
    let expected = [0, 1, 2].map(|axis| {
        0.5 * (source_level.corner([4.0, 2.0, 1.0])[axis]
            + source_level.corner([12.0, 10.0, 5.0])[axis])
            + image.pyramid().placement()[axis]
    });
    for (axis, value) in written.pyramid().placement().into_iter().enumerate() {
        assert!(
            (value - expected[axis]).abs() <= 1.0e-4,
            "the cropped image keeps the world centre of its box on axis {axis}: {value} against {}",
            expected[axis]
        );
    }
    let data = written
        .read_region(0, 0, 0, 0..8, 0..8, 0..4)
        .expect("the cropped channel reads");
    let VoxelData::U16(data) = data else {
        panic!("the cropped channel keeps the voxel type of its dataset");
    };
    assert_eq!(data[0], 124, "the crop starts at voxel 4,2,1");
    let labels = written.labels().expect("the cropped labels read");
    let label = labels.get("spot").expect("the cropped label exists");
    let crop = label
        .read(0, 0, &[0..8, 0..8, 0..4], [0, 0, 0])
        .expect("the cropped label reads");
    assert_eq!(crop[1 + 8 * (1 + 8)], 1, "the spot keeps its identity");
    assert_eq!(
        label.level(0).expect("the label level exists").extent(),
        Extent::new(8, 8, 4)
    );
}
#[test]
fn reads_ome_tiff_metadata_and_voxels() {
    let image = grana_tiff::Image::open(&fixture("ome.tif"), 0).expect("the ome tiff opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 1);
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level(0).extent(), Extent::new(32, 32, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 1.0));
    assert_eq!(pyramid.channels()[0].name(), "GFP");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 1.0, 0.0]);
    assert_eq!(image.series().len(), 1);
    let data = image
        .read_region(0, 0, 0, 0..32, 0..32, 0..2)
        .expect("the ome volume reads");
    let VoxelData::U8(values) = data else {
        panic!("the ome tiff stores bytes");
    };
    for z in 0..2u32 {
        for y in 0..32u32 {
            for x in 0..32u32 {
                let index = ((z * 32 + y) * 32 + x) as usize;
                assert_eq!(
                    u32::from(values[index]),
                    (z * 97 + y * 5 + x * 3) % 256,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }

    let temp = Temp::new("suite-ome-tiff-series");
    let path = temp.path().join("series.ome.tif");
    write_two_series(&path);
    assert_eq!(
        grana_tiff::series_count(&path).expect("the series count reads"),
        2
    );
    assert_eq!(
        grana_tiff::series_names(&path).expect("the series names read"),
        vec![Some("Overview".to_string()), Some("Detail".to_string())]
    );
    assert_eq!(
        grana_image::series_count(&path).expect("the image counts its series"),
        2
    );
    let overview = grana_tiff::Image::open(&path, 0).expect("the overview opens");
    assert_eq!(overview.name(), "Overview");
    assert_eq!(overview.pyramid().calibration(), Calibration::Recorded);
    let VoxelData::U8(values) = overview
        .read_region(0, 0, 0, 0..4, 0..4, 0..1)
        .expect("the overview reads")
    else {
        panic!("the overview stores bytes");
    };
    assert_eq!(values, (1..=16).collect::<Vec<u8>>());
    let detail = grana_tiff::Image::open(&path, 1).expect("the detail opens");
    assert_eq!(detail.name(), "Detail");
    assert_eq!(detail.pyramid().level(0).extent(), Extent::new(2, 2, 2));
    assert_eq!(
        detail.pyramid().level(0).spacing(),
        Spacing::new(0.5, 0.5, 1.5)
    );
    assert_eq!(detail.pyramid().calibration(), Calibration::Inferred);
    let VoxelData::U8(values) = detail
        .read_region(0, 0, 0, 0..2, 0..2, 0..2)
        .expect("the detail reads")
    else {
        panic!("the detail stores bytes");
    };
    assert_eq!(values, [7u8, 7, 7, 7, 9, 9, 9, 9]);
    assert!(
        grana_tiff::Image::open(&path, 2).is_err(),
        "a series outside the metadata is refused"
    );
    let unplaced = temp.path().join("unplaced.ome.tif");
    support::ome_tiff(&unplaced, UNPLACED, &[(2, 2, &[3u8; 4]), (2, 2, &[5u8; 4])]);
    let stack = grana_tiff::Image::open(&unplaced, 0).expect("the unplaced stack opens");
    assert_eq!(stack.pyramid().level(0).extent(), Extent::new(2, 2, 2));
    assert_eq!(
        stack.pyramid().level(0).spacing(),
        Spacing::new(0.5, 0.5, 0.5)
    );
    assert_eq!(stack.pyramid().calibration(), Calibration::Assumed);
    reads_a_chunky_series(&temp.path().join("chunky.ome.tif"));
}

const TWO_SERIES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n",
    " <Image Name=\"Overview\">\n",
    "  <Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"4\" SizeY=\"4\" SizeZ=\"1\" SizeC=\"1\" SizeT=\"1\" PhysicalSizeX=\"0.5\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"0.5\" PhysicalSizeYUnit=\"µm\">\n",
    "   <Channel ID=\"Channel:0:0\" Name=\"GFP\" SamplesPerPixel=\"1\"/>\n",
    "   <TiffData IFD=\"0\" PlaneCount=\"1\"/>\n",
    "  </Pixels>\n",
    " </Image>\n",
    " <Image Name=\"Detail\">\n",
    "  <Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"2\" SizeY=\"2\" SizeZ=\"2\" SizeC=\"1\" SizeT=\"1\" PhysicalSizeX=\"0.5\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"0.5\" PhysicalSizeYUnit=\"µm\">\n",
    "   <Channel ID=\"Channel:0:1\" Name=\"GFP\" SamplesPerPixel=\"1\"/>\n",
    "   <TiffData IFD=\"1\" PlaneCount=\"2\"/>\n",
    "   <Plane TheZ=\"0\" TheC=\"0\" TheT=\"0\" PositionZ=\"12.5\" PositionZUnit=\"µm\"/>\n",
    "   <Plane TheZ=\"1\" TheC=\"0\" TheT=\"0\" PositionZ=\"14\" PositionZUnit=\"µm\"/>\n",
    "  </Pixels>\n",
    " </Image>\n",
    "</OME>\n"
);

const UNPLACED: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n",
    " <Image Name=\"Stack\">\n",
    "  <Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"2\" SizeY=\"2\" SizeZ=\"2\" SizeC=\"1\" SizeT=\"1\" PhysicalSizeX=\"0.5\" PhysicalSizeXUnit=\"µm\" PhysicalSizeY=\"0.5\" PhysicalSizeYUnit=\"µm\">\n",
    "   <Channel ID=\"Channel:0:0\" Name=\"GFP\" SamplesPerPixel=\"1\"/>\n",
    "   <TiffData IFD=\"0\" PlaneCount=\"2\"/>\n",
    "  </Pixels>\n",
    " </Image>\n",
    "</OME>\n"
);

const OVERVIEW_BYTES: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

fn write_two_series(path: &std::path::Path) {
    support::ome_tiff(
        path,
        TWO_SERIES,
        &[
            (4, 4, &OVERVIEW_BYTES),
            (2, 2, &[7u8; 4]),
            (2, 2, &[9u8; 4]),
        ],
    );
}

#[test]
fn reads_a_tiff_stack_past_the_page_limit() {
    let temp = Temp::new("suite-tiff-page-chain");
    let path = temp.path().join("stack.tif");
    let pages = 4097u32;
    let entries = 11u32;
    let ifd = 2 + entries * 12 + 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    let mut origin = 8u32;
    for page in 0..pages {
        let next = if page + 1 == pages {
            0
        } else {
            origin + ifd + 1
        };
        bytes.extend_from_slice(&(entries as u16).to_le_bytes());
        let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        };
        entry(256, 4, 1, 1);
        entry(257, 4, 1, 1);
        entry(258, 3, 1, 8);
        entry(259, 3, 1, 1);
        entry(262, 3, 1, 1);
        entry(273, 4, 1, origin + ifd);
        entry(277, 3, 1, 1);
        entry(278, 4, 1, 1);
        entry(279, 4, 1, 1);
        entry(284, 3, 1, 1);
        entry(339, 3, 1, 1);
        bytes.extend_from_slice(&next.to_le_bytes());
        bytes.push((page % 256) as u8);
        origin = next;
    }
    std::fs::write(&path, &bytes).expect("the stacked tiff writes");
    let image = grana_tiff::Image::open(&path, 0).expect("the stacked tiff opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(1, 1, pages));
    assert_eq!(
        image.pyramid().level(0).spacing(),
        Spacing::new(1.0, 1.0, 1.0),
        "a plain stack carries no physical size"
    );
    assert_eq!(
        image.pyramid().calibration(),
        Calibration::Uncalibrated,
        "a plain stack is uncalibrated rather than pretending a recorded size"
    );
    let last = image
        .read_region(0, 0, 0, 0..1, 0..1, pages - 1..pages)
        .expect("the last plane reads");
    let VoxelData::U8(values) = last else {
        panic!("the stacked tiff stores bytes");
    };
    assert_eq!(values, vec![((pages - 1) % 256) as u8]);
    assert!(
        image.pyramid().channels()[0].intensity().max() > 0.0,
        "a stack reports the intensities it stores rather than the full byte domain"
    );
    assert_eq!(
        image.pyramid().channels()[0].semantics(),
        VoxelSemantics::Label,
        "a stack of few distinct values is a label volume"
    );
    let palette = temp.path().join("palette.tif");
    let pages = 105u32;
    let entries = 12u32;
    let record = 2 + entries * 12 + 4;
    let payload = 25u32;
    let map_at = 8u32 + pages * (record + payload);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    let mut origin = 8u32;
    for page in 0..pages {
        let next = if page + 1 == pages {
            0
        } else {
            origin + record + payload
        };
        bytes.extend_from_slice(&(entries as u16).to_le_bytes());
        let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        };
        entry(256, 4, 1, 5);
        entry(257, 4, 1, 5);
        entry(258, 3, 1, 8);
        entry(259, 3, 1, 1);
        entry(262, 3, 1, 3);
        entry(273, 4, 1, origin + record);
        entry(277, 3, 1, 1);
        entry(278, 4, 1, 5);
        entry(279, 4, 1, payload);
        entry(284, 3, 1, 1);
        entry(320, 3, 768, map_at);
        entry(339, 3, 1, 1);
        bytes.extend_from_slice(&next.to_le_bytes());
        for y in 0..5usize {
            for x in 0..5usize {
                bytes.push(u8::from(y == 2 && x < 3));
            }
        }
        origin = next;
    }
    let mut map = vec![0u8; 1536];
    for index in 0..256usize {
        let component: u16 = if index == 1 { 0xffff } else { index as u16 };
        for channel in 0..3usize {
            let at = (channel * 256 + index) * 2;
            map[at..at + 2].copy_from_slice(&component.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&map);
    std::fs::write(&palette, &bytes).expect("the palette stack writes");
    let indexed = grana_tiff::Image::open(&palette, 0).expect("the palette stack opens");
    assert_eq!(
        indexed.pyramid().channels()[0].semantics(),
        VoxelSemantics::Label,
        "a palette page carries labels even when its extent outruns the census budget"
    );
    assert_eq!(
        indexed.pyramid().channels()[0].intensity(),
        Intensity::new(0.0, 255.0),
        "a palette stack reports the intensities its colour map expands to"
    );
}

#[test]
fn browses_an_ome_zarr_plate() {
    let temp = Temp::new("suite-plate");
    let root = temp.path().join("assay.ome.zarr");
    support::plate(&root);
    assert!(grana_image::is_image(&root), "a plate folder is a dataset");
    assert_eq!(
        grana_image::series_count(&root).expect("the plate counts its fields"),
        2
    );
    let names = grana_image::series_names(&root).expect("the plate names its fields");
    assert_eq!(
        names,
        vec![Some("A1/0".to_string()), Some("A2/1".to_string())]
    );
    assert_eq!(
        grana_image::series_group(&root, 1).expect("the second field resolves"),
        Some("/A/2/1".to_string())
    );
    let zarr = grana_zarr::Root::local(&root).expect("the plate opens");
    let plate = grana_zarr::open_plate(&zarr).expect("the plate metadata parses");
    assert_eq!((plate.rows(), plate.columns()), (1, 2));
    assert_eq!(plate.row_name(0), Some("A"));
    assert_eq!(plate.column_name(1), Some("2"));
    let well = plate.well(0, 1).expect("the second column holds a well");
    assert_eq!(well.label(), "A2");
    assert_eq!(well.fields().len(), 1);
    assert_eq!(plate.fields()[1].series(), 1);
    assert_eq!(
        (plate.fields()[1].row(), plate.fields()[1].column()),
        (0, 1)
    );
    assert_eq!(plate.well_of(1).map(grana_zarr::Well::label), Some("A2"));
    let browse = grana_image::plate(&Source::file(root.clone()))
        .expect("the image layer reads the plate")
        .expect("the folder is a plate");
    assert_eq!(browse.len(), plate.len());
    assert_eq!(browse.wells().len(), plate.wells().len());
    let resolved = grana_image::resolve_series(&Source::file_series(root, 1))
        .expect("the second field resolves");
    assert_eq!(resolved.group(), Some("/A/2/1"));
    let image = Image::open(&resolved).expect("the second field opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(12, 10, 6));
}

#[test]
fn writes_and_reads_a_label_image() {
    let temp = Temp::new("suite-zarr-label-write");
    let path = temp.path().join("labelled.ome.zarr");
    let extent = Extent::new(300, 10, 6);
    let levels = [Level::new(extent, Spacing::new(1.0, 1.0, 1.0))];
    let channels = [WriteChannel {
        name: "DAPI",
        tint: [0.0, 0.0, 1.0],
        data: Intensity::new(0.0, 1.0),
        window: Intensity::new(0.0, 1.0),
    }];
    let image_fill = |block: grana_archive::Block| -> Result<VoxelData, String> {
        Ok(VoxelData::F32(vec![0.0; block.voxels()]))
    };
    let mut progress = |_, _| {};
    write_image(
        &path,
        &mut ImageSpec {
            name: "wide",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 2,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&image_fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the wide image writes");
    let label_of = |x: u32, y: u32, z: u32| 1 + (x + 3 * y + 5 * z) % 7;
    let label_fill = |_: usize,
                      _: u32,
                      origin: [u32; 3],
                      length: [u32; 3],
                      data: &mut [u32]|
     -> Result<(), String> {
        for z in 0..length[2] {
            for y in 0..length[1] {
                for x in 0..length[0] {
                    let index = x as usize
                        + length[0] as usize * (y as usize + length[1] as usize * z as usize);
                    data[index] = label_of(origin[0] + x, origin[1] + y, origin[2] + z);
                }
            }
        }
        Ok(())
    };
    grana_zarr::write_group_labels(
        &path,
        "/",
        grana_zarr::LabelSpec {
            name: "cells",
            frames: &[0, 1],
            labels: 7,
            color: [1.0, 0.0, 0.0],
            opacity: 1.0,
            properties: &[],
            fill: &label_fill,
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the labels write over a chunk boundary");
    let image = grana_zarr::Image::open(&path).expect("the labelled image opens");
    let labels = image.labels().expect("the labels read");
    let label = labels.get("cells").expect("the written label exists");
    assert_eq!(label.frames(), 2);
    let data = label
        .read(
            1,
            0,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
            [0, 0, 0],
        )
        .expect("the written label region reads");
    assert_eq!(data.len(), extent.voxel_count() as usize);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                assert_eq!(
                    data[extent.index(x, y, z) as usize],
                    label_of(x, y, z),
                    "the label at {x},{y},{z}"
                );
            }
        }
    }
    let labelled = Image::open_with_cache(
        &Source::file_group(path.clone(), 0, "/labels/cells".to_string()),
        None,
    )
    .expect("the label image opens as a dataset");
    assert_eq!(
        labelled.pyramid().channels()[0].semantics(),
        grana_volume::VoxelSemantics::Label,
        "a label image samples its nearest child instead of averaging its ids"
    );
    let coarse = Extent::new(
        extent.x().div_ceil(2),
        extent.y().div_ceil(2),
        extent.z().div_ceil(2),
    );
    let stored = labelled
        .read_region(
            0,
            0,
            0,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
            &grana_warp::Warp::IDENTITY,
        )
        .expect("the stored label level reads");
    let derived = labelled
        .read_region(
            0,
            1,
            0,
            &[0..coarse.x(), 0..coarse.y(), 0..coarse.z()],
            &grana_warp::Warp::IDENTITY,
        )
        .expect("the derived label level reads");
    let mut fine = vec![0.0f32; stored.voxel_count()];
    stored.write_values(&mut fine);
    let mut sampled = vec![0.0f32; derived.voxel_count()];
    derived.write_values(&mut sampled);
    for z in 0..coarse.z() {
        for y in 0..coarse.y() {
            for x in 0..coarse.x() {
                let child = extent.index(
                    (2 * x).min(extent.x() - 1),
                    (2 * y).min(extent.y() - 1),
                    (2 * z).min(extent.z() - 1),
                ) as usize;
                assert_eq!(
                    sampled[coarse.index(x, y, z) as usize],
                    fine[child],
                    "the derived label at {x},{y},{z} keeps the nearest id"
                );
            }
        }
    }
    grana_zarr::write_group_labels(
        &path,
        "/",
        grana_zarr::LabelSpec {
            name: "spots",
            frames: &[0],
            labels: 1,
            color: [0.0, 1.0, 0.0],
            opacity: 1.0,
            properties: &[],
            fill: &label_fill,
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("a second label image joins the labels group");
    let crashed = path.join("labels").join("cells");
    std::fs::rename(&crashed, path.join("labels").join(".cells.old"))
        .expect("the replacement leaves its backup behind");
    let staging = path.join("labels").join(".cells.100-0.part");
    std::fs::create_dir_all(&staging)
        .expect("the interrupted replacement leaves its staging behind");
    std::fs::write(staging.join("zarr.json"), "{}")
        .expect("the staging holds half written metadata");
    let image = grana_zarr::Image::open(&path).expect("the interrupted image opens");
    let labels = image.labels().expect("the interrupted labels recover");
    let mut names = labels.names().map(str::to_string).collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, vec!["cells".to_string(), "spots".to_string()]);
    let data = labels
        .get("cells")
        .expect("the recovered label exists")
        .read(
            1,
            0,
            &[0..extent.x(), 0..extent.y(), 0..extent.z()],
            [0, 0, 0],
        )
        .expect("the recovered label region reads");
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                assert_eq!(
                    data[extent.index(x, y, z) as usize],
                    label_of(x, y, z),
                    "the recovered label at {x},{y},{z}"
                );
            }
        }
    }
    assert!(
        !path.join("labels").join(".cells.old").exists(),
        "the backup is consumed by the recovery"
    );
    assert!(
        !staging.exists(),
        "the staging is reclaimed by the recovery"
    );
}

#[test]
fn rolls_back_an_unpublished_directory_replacement() {
    let temp = Temp::new("suite-file-rollback");
    let labels = temp.path().join("labels");
    let target = labels.join("cells");
    std::fs::create_dir_all(&target).expect("the previous label image exists");
    std::fs::write(target.join("zarr.json"), "previous").expect("the previous metadata writes");
    let atomic = grana_file::Atomic::directory(&target).expect("the replacement stages");
    std::fs::write(atomic.staging().join("zarr.json"), "replacement")
        .expect("the replacement writes");
    let replaced = atomic.commit_held().expect("the replacement publishes");
    assert_eq!(
        std::fs::read_to_string(target.join("zarr.json")).expect("the replacement reads"),
        "replacement"
    );
    replaced.rollback().expect("the replacement rolls back");
    assert_eq!(
        std::fs::read_to_string(target.join("zarr.json")).expect("the previous label reads"),
        "previous"
    );
    assert!(
        !labels.join(".cells.old").exists(),
        "the rollback restores the previous label image"
    );
    let staging = labels.join(".cells.100-0.part");
    std::fs::create_dir_all(&staging).expect("a writer stages a replacement");
    std::fs::write(staging.join("zarr.json"), "{}").expect("the staging holds metadata");
    let lease = grana_file::Lease::acquire(&target).expect("the target lease is held");
    grana_file::recover(&target).expect("recovery skips a leased target");
    assert!(
        staging.exists(),
        "recovery keeps the staging of a live writer"
    );
    drop(lease);
    grana_file::recover(&target).expect("recovery reclaims a released target");
    assert!(
        !staging.exists(),
        "recovery reclaims staging no writer holds"
    );
}

#[test]
fn round_trips_a_typed_geff_lineage() {
    let temp = Temp::new("suite-geff");
    let path = temp.path().join("graph.geff");
    let detections = vec![
        Detection::new(1, 0, [0.0, 0.0, 0.0], 0.8, 0.9),
        Detection::new(2, 0, [2.0, 0.0, 0.0], 0.8, 0.7),
        Detection::new(3, 1, [1.0, 0.0, 0.0], 1.0, 0.6),
        Detection::new(5, 2, [1.1, 0.0, 0.0], 1.0, 0.5),
        Detection::new(6, 3, [0.7, 0.0, 0.0], 0.9, 0.4),
        Detection::new(7, 3, [1.5, 0.0, 0.0], 0.9, 0.3),
        Detection::new(8, 4, [0.75, 0.0, 0.0], 0.9, 0.2),
        Detection::new(9, 6, [0.8, 0.0, 0.0], 0.9, 0.1),
    ];
    let edges = vec![
        Edge::new(1, 3),
        Edge::new(2, 3),
        Edge::new(3, 5),
        Edge::new(5, 6),
        Edge::new(5, 7),
        Edge::new(6, 8),
        Edge::new(8, 9),
    ];
    let lineage = Lineage::new(detections, edges).expect("the lineage is consistent");
    let axes = geff::Axes::new([0.25, 0.25, 0.5], 2.0);
    geff::write(&path, &lineage, &axes).expect("the graph writes");
    let read = geff::read(&path, 0.5).expect("the graph reads");
    assert_eq!(read.len(), 8);
    assert_eq!(read.edges().len(), 7);
    assert_eq!(read.frames(), vec![0, 1, 2, 3, 4, 6]);
    assert_eq!(read.detections()[2].id(), 3);
    assert_eq!(read.detections()[2].position(), [1.0, 0.0, 0.0]);
    assert!((read.detections()[2].radius() - 1.0).abs() < 1.0e-6);
    assert!((read.detections()[2].quality() - 0.6).abs() < 1.0e-6);
    let set = read.tracks(2.0).expect("the graph forms tracks");
    assert_eq!(set.merges(), 1);
    assert_eq!(set.divisions(), 1);
    assert_eq!(
        set.links()
            .iter()
            .filter(|link| link.kind() == grana_object::Link::Gap)
            .count(),
        1
    );
    assert_eq!(
        read.tracklets()
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        5
    );
    assert_eq!(
        read.lineages()
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1
    );
    let empty = Lineage::new(
        vec![Detection::new(1, 0, [0.0, 0.0, 0.0], 1.0, 0.0)],
        Vec::new(),
    )
    .expect("a single detection is a lineage");
    let path = temp.path().join("empty.geff");
    geff::write(&path, &empty, &axes).expect("an edgeless graph writes");
    let read = geff::read(&path, 0.5).expect("an edgeless graph reads");
    assert_eq!(read.len(), 1);
    assert!(read.edges().is_empty());
    let path = temp.path().join("mismatched.geff");
    geff::write(&path, &lineage, &axes).expect("the graph writes");
    let store = std::sync::Arc::new(
        zarrs::filesystem::FilesystemStore::new(&path).expect("the graph opens as a store"),
    );
    let kinds = zarrs::array::Array::open(store, "/edges/props/grana_kind/values")
        .expect("the edge kinds open");
    kinds
        .store_chunk(&[0], &[0u8; 7])
        .expect("the edge kinds overwrite");
    assert!(
        geff::read(&path, 0.5).is_err(),
        "the graph rejects edge kinds that contradict its topology"
    );
}

#[test]
fn keeps_the_previous_dataset_when_an_export_fails() {
    let temp = Temp::new("suite-zarr-transaction");
    let path = temp.path().join("kept.ome.zarr");
    write_zarr(&path);
    let levels = [
        Level::new(Extent::new(24, 16, 8), Spacing::new(0.5, 0.5, 1.0)),
        Level::new(Extent::new(12, 8, 4), Spacing::new(1.0, 1.0, 2.0)),
    ];
    let voxels = levels
        .iter()
        .map(|level| level.extent().voxel_count())
        .sum::<u64>();
    assert!(
        grana_zarr::image_space(&levels, 2, 3, grana_volume::VoxelFormat::F32)
            >= voxels * 2 * 3 * 4,
        "the planned image covers every stored float voxel"
    );
    assert!(
        grana_zarr::label_space(&levels, 2, 7) >= voxels * 2 * 2,
        "the planned label image covers every stored sixteen bit voxel"
    );
    let error = grana_zarr::Archive::create(&path, u64::MAX / 4)
        .err()
        .expect("the volume cannot hold the planned dataset beside its previous version");
    assert!(
        error.to_string().contains("GiB"),
        "the refusal names the volume sizes: {error}"
    );
    let kept = grana_zarr::Image::open(&path).expect("the previous dataset still opens");
    assert_eq!(kept.pyramid().level(0).extent(), Extent::new(24, 16, 8));
    let staging = std::fs::read_dir(temp.path())
        .expect("the temporary directory lists")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
        .count();
    assert_eq!(staging, 0, "a refused dataset leaves no staging behind");

    write_zarr(&path);
    let extent = Extent::new(8, 8, 4);
    let volume = || {
        Volume::new(
            extent,
            Spacing::new(1.0, 1.0, 1.0),
            vec![Channel::new(
                "signal",
                [1.0, 1.0, 1.0],
                VoxelData::U16(vec![3u16; extent.voxel_count() as usize]),
            )],
        )
    };
    let image = Arc::new(
        Image::open(&Source::memory(VolumeSeries::new(
            vec![volume(), volume()],
            1.0,
        )))
        .expect("the image opens"),
    );
    let levels = image.pyramid().levels().to_vec();
    let mut label_scene = Scene::new(Dataset::synthetic("labelled", image.pyramid().clone()));
    let label_set = label_scene.add_object(ObjectKind::Spots, 0);
    let centre = image.pyramid().level(0).voxel_center([3.0, 3.0, 2.0]);
    let spots = || {
        SpotSet::new(
            0,
            vec![Spot::new(centre, 0.9, 1.0)],
            IntensityTable::new(1, vec![SampleStatistics::new(1.0, 1.0, 1.0, 0.0, 1.0)]),
        )
    };
    label_scene.install(label_set, 0, ObjectFrame::from_spots(spots()));
    label_scene.install(label_set, 1, ObjectFrame::from_spots(spots()));
    let label = LabelLayer::of(
        label_scene
            .objects
            .get(label_set)
            .expect("the label set exists"),
        &levels,
    )
    .expect("the label layer builds")
    .expect("the label set holds labelled objects");
    let mut progress = |_, _| {};
    let outcome = grana_pipeline::write_dataset(
        DatasetSpec {
            path: path.clone(),
            name: "labelled".to_string(),
            channels: vec![ChannelLayer {
                index: 0,
                name: "signal".to_string(),
                tint: [1.0, 1.0, 1.0],
                data: Intensity::new(0.0, 10.0),
                window: Intensity::new(0.0, 10.0),
            }],
            levels,
            origins: vec![[0, 0, 0]; image.pyramid().level_count()],
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            calibration: grana_volume::Calibration::Recorded,
            drift: Vec::new(),
            labels: vec![label],
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    );
    assert!(
        outcome.is_err(),
        "a label image that names a frame the export omits fails the export"
    );
    let kept = grana_zarr::Image::open(&path).expect("the previous dataset still opens");
    assert_eq!(kept.pyramid().level(0).extent(), Extent::new(24, 16, 8));
    assert!(
        kept.labels()
            .expect("the previous labels read")
            .names()
            .next()
            .is_none(),
        "the failed export leaves the previous dataset unlabelled"
    );
}

#[test]
fn reads_picasso_localization_tables() {
    let temp = Temp::new("suite-picasso-table");
    let path = temp.path().join("localizations.hdf5");
    support::picasso(
        &path,
        100.0,
        &[
            support::Localization {
                frame: 0,
                position: [10.0, 5.0, 600.0],
                photons: 3000.0,
                precision: [0.2, 0.22, 45.0],
                group: 7,
            },
            support::Localization {
                frame: 1,
                position: [30.0, 5.0, 600.0],
                photons: 2000.0,
                precision: [0.25, 0.25, 45.0],
                group: 7,
            },
        ],
    );
    let level = Level::new(Extent::new(64, 32, 32), Spacing::new(0.1, 0.1, 0.1));
    let found = support::read_localizations(&path, level, 2);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].time(), 0);
    assert_eq!(found[1].time(), 1);
    assert_eq!(
        (
            found[0].center().to_vec(),
            found[0].sigma().to_vec(),
            found[0].quality(),
            found[0].group()
        ),
        (
            vec![-2.2, -1.1, -1.0],
            vec![0.02, 0.022, 0.045],
            3000.0,
            Some(7)
        ),
        "a Picasso localization lands on the dataset with its precision and group"
    );
}

fn reads_a_chunky_series(path: &std::path::Path) {
    write_chunky_series(path);
    let mono = grana_tiff::Image::open(path, 0).expect("the mono series opens");
    assert_eq!(mono.pyramid().channel_count(), 1);
    let chunky = grana_tiff::Image::open(path, 1).expect("the chunky series opens");
    assert_eq!(
        chunky.pyramid().channel_count(),
        3,
        "the chunky series carries the samples of its own pages"
    );
    assert_eq!(chunky.pyramid().level(0).extent(), Extent::new(2, 2, 1));
    let VoxelData::U8(values) = chunky
        .read_region(0, 0, 1, 0..2, 0..2, 0..1)
        .expect("the green sample reads")
    else {
        panic!("the chunky series stores bytes");
    };
    assert_eq!(values, [1u8, 2, 3, 4], "the green sample of every pixel");
}

fn write_chunky_series(path: &std::path::Path) {
    let xml = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<OME xmlns=\"http://www.openmicroscopy.org/Schemas/OME/2016-06\">\n",
        " <Image Name=\"Overview\">\n",
        "  <Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"4\" SizeY=\"4\" SizeZ=\"1\" SizeC=\"1\" SizeT=\"1\">\n",
        "   <Channel ID=\"Channel:0:0\" Name=\"GFP\" SamplesPerPixel=\"1\"/>\n",
        "   <TiffData IFD=\"0\" PlaneCount=\"1\"/>\n",
        "  </Pixels>\n",
        " </Image>\n",
        " <Image Name=\"Colour\">\n",
        "  <Pixels DimensionOrder=\"XYZCT\" Type=\"uint8\" SizeX=\"2\" SizeY=\"2\" SizeZ=\"1\" SizeC=\"3\" SizeT=\"1\">\n",
        "   <Channel ID=\"Channel:1:0\" Name=\"Red\" SamplesPerPixel=\"3\"/>\n",
        "   <Channel ID=\"Channel:1:1\" Name=\"Green\" SamplesPerPixel=\"3\"/>\n",
        "   <Channel ID=\"Channel:1:2\" Name=\"Blue\" SamplesPerPixel=\"3\"/>\n",
        "   <TiffData IFD=\"1\" PlaneCount=\"1\"/>\n",
        "  </Pixels>\n",
        " </Image>\n",
        "</OME>\n"
    );
    let entries = 12u32;
    let record = 2 + entries * 12 + 4;
    let header = 8u32;
    let xml_offset = header + 2 * record;
    let xml_bytes = xml.as_bytes().to_vec();
    let strips = xml_offset + xml_bytes.len() as u32;
    let pages = [(4u32, 4u32, 16u32, 1u32, 1u32), (2, 2, 12, 3, 2)];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&header.to_le_bytes());
    let mut strip = strips;
    for (index, (width, height, length, samples, photometric)) in pages.into_iter().enumerate() {
        let next = if index + 1 == pages.len() {
            0
        } else {
            header + (index as u32 + 1) * record
        };
        bytes.extend_from_slice(&(entries as u16).to_le_bytes());
        let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        };
        entry(256, 4, 1, width);
        entry(257, 4, 1, height);
        entry(258, 3, 1, 8);
        entry(259, 3, 1, 1);
        entry(262, 3, 1, photometric);
        entry(270, 2, xml_bytes.len() as u32, xml_offset);
        entry(273, 4, 1, strip);
        entry(277, 3, 1, samples);
        entry(278, 4, 1, height);
        entry(279, 4, 1, length);
        entry(284, 3, 1, 1);
        entry(339, 3, 1, 1);
        bytes.extend_from_slice(&next.to_le_bytes());
        strip += length;
    }
    bytes.extend_from_slice(&xml_bytes);
    bytes.extend(1..=16u8);
    for (red, green, blue) in [(10u8, 1u8, 20u8), (11, 2, 21), (12, 3, 22), (13, 4, 23)] {
        bytes.extend([red, green, blue]);
    }
    std::fs::write(path, &bytes).expect("the sample tiff writes");
}

fn electron_stack(path: &std::path::Path, extent: [u32; 3]) {
    let mut bytes = vec![0u8; 1024];
    let mut put = |at: usize, value: i32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    put(0, extent[0] as i32);
    put(4, extent[1] as i32);
    put(8, extent[2] as i32);
    put(12, 1);
    put(28, extent[0] as i32);
    put(32, extent[1] as i32);
    put(36, extent[2] as i32);
    put(64, 1);
    put(68, 2);
    put(72, 3);
    bytes[208..212].copy_from_slice(b"MAP ");
    bytes[212..216].copy_from_slice(&[0x44, 0x44, 0, 0]);
    for z in 0..extent[2] {
        for y in 0..extent[1] {
            for x in 0..extent[0] {
                let value = (x + 10 * y + 100 * z) as i16;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    std::fs::write(path, &bytes).expect("the electron stack writes");
}

#[test]
fn reads_an_electron_microscopy_stack_through_the_shared_registry() {
    let temp = Temp::new("suite-mrc");
    let path = temp.path().join("tilt.mrc");
    electron_stack(&path, [8, 6, 4]);
    assert!(
        grana_image::is_image(&path),
        "the stack is not recognised as an image"
    );
    assert_eq!(
        grana_image::series_count(&path).expect("the stack counts its images"),
        1
    );
    let image = Image::open(&Source::file(path.clone())).expect("the stack opens");
    assert_eq!(image.format(), Some(grana_image::Format::Readout));
    assert_eq!(
        image.family(),
        Some(grana_readout::Family::Electron),
        "the stack does not declare its vendor family"
    );
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(8, 6, 1));
    assert_eq!(image.pyramid().frame_count(), 4);
    assert_eq!(image.pyramid().channel_count(), 1);
    let plane = image
        .read_region(1, 0, 0, &[1..4, 2..3, 0..1], &grana_warp::Warp::IDENTITY)
        .expect("a tilt section reads");
    let expected = [121i16, 122, 123];
    let VoxelData::I16(values) = plane else {
        panic!("a signed sixteen bit electron stack keeps its native samples");
    };
    assert_eq!(values, expected.to_vec(), "the plane values are wrong");
}

struct SamplePage<'a> {
    width: u32,
    height: u32,
    bits: u16,
    format: u16,
    fill_order: u16,
    tile: Option<(u32, u32)>,
    payload: &'a [u8],
}

fn sample_tiff(path: &std::path::Path, page: SamplePage<'_>) {
    let SamplePage {
        width,
        height,
        bits,
        format,
        fill_order,
        tile,
        payload,
    } = page;
    let entries = if tile.is_some() { 12u32 } else { 11 };
    let record = 2 + entries * 12 + 4;
    let header = 8u32;
    let data = header + record;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&header.to_le_bytes());
    bytes.extend_from_slice(&(entries as u16).to_le_bytes());
    let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    };
    entry(256, 4, 1, width);
    entry(257, 4, 1, height);
    entry(258, 3, 1, u32::from(bits));
    entry(259, 3, 1, 1);
    entry(262, 3, 1, 1);
    entry(266, 3, 1, u32::from(fill_order));
    entry(277, 3, 1, 1);
    match tile {
        Some((tile_width, tile_length)) => {
            entry(322, 4, 1, tile_width);
            entry(323, 4, 1, tile_length);
            entry(324, 4, 1, data);
            entry(325, 4, 1, payload.len() as u32);
        }
        None => {
            entry(273, 4, 1, data);
            entry(278, 4, 1, height);
            entry(279, 4, 1, payload.len() as u32);
        }
    }
    entry(339, 3, 1, u32::from(format));
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(payload);
    std::fs::write(path, &bytes).expect("the packed tiff writes");
}

#[test]
fn reads_packed_samples_in_the_declared_bit_order() {
    let temp = Temp::new("suite-tiff-fill-order");
    let row = |name: &str, fill_order: u16| -> Vec<u8> {
        let path = temp.path().join(name);
        sample_tiff(
            &path,
            SamplePage {
                width: 8,
                height: 1,
                bits: 1,
                format: 1,
                fill_order,
                tile: None,
                payload: &[0x01],
            },
        );
        let image = grana_tiff::Image::open(&path, 0).expect("the packed page opens");
        let data = image
            .read_region(0, 0, 0, 0..8, 0..1, 0..1)
            .expect("the packed row reads");
        let VoxelData::U8(values) = data else {
            panic!("a one bit page unpacks to bytes");
        };
        values
    };
    assert_eq!(
        row("msb.tif", 1),
        vec![0, 0, 0, 0, 0, 0, 0, 1],
        "a most significant bit first page starts at the leftmost pixel"
    );
    assert_eq!(
        row("lsb.tif", 2),
        vec![1, 0, 0, 0, 0, 0, 0, 0],
        "a least significant bit first page starts at the leftmost pixel"
    );
    let path = temp.path().join("half.tif");
    sample_tiff(
        &path,
        SamplePage {
            width: 8,
            height: 1,
            bits: 16,
            format: 3,
            fill_order: 1,
            tile: Some((8, 1)),
            payload: &[0u8; 16],
        },
    );
    assert!(
        matches!(
            grana_tiff::Image::open(&path, 0),
            Err(grana_tiff::Error::Unsupported { .. })
        ),
        "a sixteen bit floating point page is rejected instead of read as garbage"
    );
    let path = temp.path().join("packed24.tif");
    sample_tiff(
        &path,
        SamplePage {
            width: 4,
            height: 1,
            bits: 24,
            format: 1,
            fill_order: 1,
            tile: Some((4, 1)),
            payload: &(1..=12u8).collect::<Vec<u8>>(),
        },
    );
    assert!(
        matches!(
            grana_tiff::Image::open(&path, 0),
            Err(grana_tiff::Error::Unsupported { .. })
        ),
        "a twenty four bit page is rejected instead of read into a wider container"
    );
    let twelve = [0x0ABu16, 0x123, 0x456, 0x789, 0xABC, 0xDEF, 0x111, 0x222];
    let payload = twelve
        .chunks(2)
        .flat_map(|pair| {
            let window = u32::from(pair[0]) << 12 | u32::from(pair[1]);
            [(window >> 16) as u8, (window >> 8) as u8, window as u8]
        })
        .collect::<Vec<u8>>();
    let path = temp.path().join("twelve.tif");
    sample_tiff(
        &path,
        SamplePage {
            width: 4,
            height: 2,
            bits: 12,
            format: 1,
            fill_order: 1,
            tile: Some((4, 2)),
            payload: &payload,
        },
    );
    let image = grana_tiff::Image::open(&path, 0).expect("the twelve bit page opens");
    let data = image
        .read_region(0, 0, 0, 0..4, 0..2, 0..1)
        .expect("the twelve bit tile reads");
    let VoxelData::U16(values) = data else {
        panic!("a twelve bit page unpacks to a sixteen bit container");
    };
    assert_eq!(
        values,
        twelve.to_vec(),
        "a twelve bit tile keeps every value it stores"
    );
}
