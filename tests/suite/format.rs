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
    Channel, Extent, Intensity, Level, Region, Spacing, Volume, VolumeSeries, VoxelData,
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
        range: Intensity::new(0.0, 4096.0),
    }];
    let fill = |level, _frame, _channel, origin: [u32; 3], extent: [u32; 3], data: &mut [f32]| {
        let scale = 1u32 << level;
        for z in 0..extent[2] {
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let index = x as usize
                        + extent[0] as usize * (y as usize + extent[1] as usize * z as usize);
                    data[index] = ((origin[0] + x) * scale
                        + 1000 * (origin[1] + y) * scale
                        + 1000000 * (origin[2] + z)) as f32;
                }
            }
        }
        Ok(())
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
            sample: grana_archive::Sample::Float,
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
    let image = grana_tiff::Image::open(&path).expect("the archive opens");
    assert_eq!(image.pyramid().level_count(), 3);
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
        assert!(
            (image.pyramid().level(index).spacing().x() - level.spacing().x()).abs() <= 1.0e-5,
            "level {index} keeps its spacing"
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
                range: Intensity::new(0.0, 255.0),
            }],
            levels: image.pyramid().levels().to_vec(),
            origins: vec![[0, 0, 0]; image.pyramid().level_count()],
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
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
    let exported = grana_tiff::Image::open(summary.path()).expect("the exported archive opens");
    let data = exported
        .read_region(0, 0, 0, 0..24, 0..16, 0..8)
        .expect("the exported channel reads");
    let VoxelData::F32(values) = data else {
        panic!("the exported channel stores float voxels");
    };
    assert!(
        values.iter().all(|value| *value == 7.0),
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
                range: Intensity::new(0.0, 255.0),
            }],
            levels: cropped.levels,
            origins: cropped.origins,
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            drift: Vec::new(),
            labels: Vec::new(),
        },
        &Channels::stored(Arc::clone(&image)),
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("the region exports as an archive");
    let reopened = grana_tiff::Image::open(summary.path()).expect("the cropped archive opens");
    let expected = [0, 1, 2].map(|axis| {
        0.5 * (source.corner([4.0, 2.0, 1.0])[axis] + source.corner([12.0, 10.0, 5.0])[axis])
    });
    for (axis, value) in reopened.pyramid().placement().into_iter().enumerate() {
        assert!(
            (value - expected[axis]).abs() <= 1.0e-3,
            "the cropped archive keeps the world centre of its box on axis {axis}: {value} against {}",
            expected[axis]
        );
    }
    for (axis, value) in reopened.pyramid().level(0).center().into_iter().enumerate() {
        assert!(
            value.abs() <= 1.0e-3,
            "the cropped archive places its centre at the world origin on axis {axis}: {value}"
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
        range: Intensity::new(0.0, 1.0),
    });
    let fill = |_level: usize,
                _frame: u32,
                channel: usize,
                origin: [u32; 3],
                length: [u32; 3],
                data: &mut [f32]| {
        for y in 0..length[1] {
            for x in 0..length[0] {
                let index = (origin[0] + x) as usize + size[0] as usize * (origin[1] + y) as usize;
                data[(x + length[0] * y) as usize] =
                    f32::from(pixels[index * 3 + channel]) / f32::from(u16::MAX);
            }
        }
        Ok(())
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
            sample: grana_archive::Sample::Unsigned16,
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
    let image = grana_tiff::Image::open(&path).expect("the written picture opens");
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
        range: Intensity::new(0.0, 1.0),
    }];
    let fill = |_level, _frame, _channel, origin: [u32; 3], length: [u32; 3], data: &mut [f32]| {
        for z in 0..length[2] {
            for y in 0..length[1] {
                for x in 0..length[0] {
                    let index = x as usize
                        + length[0] as usize * (y as usize + length[1] as usize * z as usize);
                    data[index] = (origin[0] + x) as f32
                        + 1000.0 * (origin[1] + y) as f32
                        + 1_000_000.0 * (origin[2] + z) as f32;
                }
            }
        }
        Ok(())
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
            sample: grana_archive::Sample::Float,
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
            range: Intensity::new(0.0, 2048.0),
        },
        WriteChannel {
            name: "GFP",
            tint: [0.0, 1.0, 0.0],
            range: Intensity::new(0.0, 1024.0),
        },
    ];
    let fill = |level, frame, channel, origin: [u32; 3], extent: [u32; 3], data: &mut [f32]| {
        let scale = 1u32 << level;
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
                    data[index] = sample(channel, frame, position) as f32;
                }
            }
        }
        Ok(())
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
            sample: grana_archive::Sample::Float,
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
fn writes_the_released_ome_zarr_schema() {
    let temp = Temp::new("suite-zarr-schema");
    let path = temp.path().join("released.ome.zarr");
    write_zarr(&path);
    let metadata = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(path.join("zarr.json")).expect("the group metadata reads"),
    )
    .expect("the group metadata parses");
    let ome = &metadata["attributes"]["ome"];
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
}

#[test]
fn writes_and_reads_a_tiny_ome_zarr() {
    let temp = Temp::new("suite-zarr");
    let path = temp.path().join("tiny.ome.zarr");
    write_zarr(&path);
    let image = grana_zarr::Image::open(&path).expect("the written image opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 2);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(pyramid.channels()[0].window(), Intensity::new(0.0, 2048.0));
    assert_eq!(pyramid.level(0).extent(), Extent::new(24, 16, 8));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 1.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(12, 8, 4));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 2.0));

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
    assert_eq!(
        image.identity(),
        Some(identity),
        "the metadata digest cannot see a chunk rewritten in place"
    );
    assert_ne!(
        image
            .stamps(0, 0, 0, 0..24, 0..16, 0..8)
            .expect("the chunk stamps reread"),
        stamps,
        "a rewritten chunk restamps every tile composed from it"
    );
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
                range: Intensity::new(0.0, 1000.0),
            }],
            levels: cropped.levels,
            origins: cropped.origins,
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
            drift: Vec::new(),
            labels: vec![label],
        },
        &channels,
        &AtomicBool::new(false),
        &mut progress,
    )
    .expect("the region exports");
    let written = grana_zarr::Image::open(&path).expect("the cropped image opens");
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
    let VoxelData::F32(data) = data else {
        panic!("the cropped channel stores float voxels");
    };
    assert_eq!(
        data[0],
        4.0 + 20.0 + 100.0,
        "the crop starts at voxel 4,2,1"
    );
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
    let image = grana_tiff::Image::open(&fixture("ome.tif")).expect("the ome tiff opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 1);
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level(0).extent(), Extent::new(32, 32, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 1.0));
    assert_eq!(pyramid.channels()[0].name(), "GFP");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 1.0, 0.0]);
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
    let image = grana_tiff::Image::open(&path).expect("the stacked tiff opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(1, 1, pages));
    let last = image
        .read_region(0, 0, 0, 0..1, 0..1, pages - 1..pages)
        .expect("the last plane reads");
    let VoxelData::U8(values) = last else {
        panic!("the stacked tiff stores bytes");
    };
    assert_eq!(values, vec![((pages - 1) % 256) as u8]);
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
        range: Intensity::new(0.0, 1.0),
    }];
    let image_fill = |_: usize, _: u32, _: usize, _: [u32; 3], _: [u32; 3], data: &mut [f32]| {
        data.fill(0.0);
        Ok(())
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
            sample: grana_archive::Sample::Float,
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
fn refuses_a_dataset_that_cannot_fit_beside_the_previous_one() {
    let temp = Temp::new("suite-zarr-space");
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
        grana_zarr::image_space(&levels, 2, 3) >= voxels * 2 * 3 * 4,
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
}

#[test]
fn keeps_the_previous_dataset_when_a_label_cannot_join_the_export() {
    let temp = Temp::new("suite-zarr-transaction");
    let path = temp.path().join("kept.ome.zarr");
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
                range: Intensity::new(0.0, 10.0),
            }],
            levels,
            origins: vec![[0, 0, 0]; image.pyramid().level_count()],
            placement: image.pyramid().placement(),
            frames: 1,
            interval: 1.0,
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
