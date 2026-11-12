use grana_image::{Image, Source};
use grana_stitch::{Edge, Offset, Plane, Request, Side, estimate, solve, stitch};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const TILE: usize = 160;
const OVERLAP: usize = 32;
const DEPTH: u32 = 4;

#[test]
fn recovers_the_offsets_of_a_tiled_image() {
    let width = 2 * TILE - OVERLAP;
    let values = (0..width * width)
        .map(|index| pattern(index % width, index / width))
        .collect::<Vec<_>>();
    let tile = |x: usize, y: usize| crop(&values, width, x, y);
    let first = tile(0, 0);
    let right = tile(TILE - OVERLAP, 0);
    let below = tile(0, TILE - OVERLAP);
    let corner = tile(TILE - OVERLAP, TILE - OVERLAP);
    let step = Offset::new((TILE - OVERLAP) as i64, 0);
    let down = Offset::new(0, (TILE - OVERLAP) as i64);
    assert_eq!(
        estimate(&first, &right, Side::Right).expect("the right tile registers"),
        step
    );
    assert_eq!(
        estimate(&first, &below, Side::Below).expect("the lower tile registers"),
        down
    );
    let edges = [
        Edge::new(0, 1, step),
        Edge::new(0, 2, down),
        Edge::new(
            1,
            3,
            estimate(&right, &corner, Side::Below).expect("the corner registers"),
        ),
        Edge::new(
            2,
            3,
            estimate(&below, &corner, Side::Right).expect("the corner registers"),
        ),
    ];
    let positions = solve(4, &edges).expect("the tile grid solves");
    assert_eq!(
        positions,
        vec![
            Offset::new(0, 0),
            step,
            down,
            Offset::new((TILE - OVERLAP) as i64, (TILE - OVERLAP) as i64),
        ]
    );
}

#[test]
fn writes_a_stitched_mosaic() {
    let width = 2 * TILE - OVERLAP;
    let canvas = (0..width * width * DEPTH as usize)
        .map(|index| pattern(index % width, (index / width) % width))
        .collect::<Vec<_>>();
    let tiles = [
        (0, 0),
        (TILE - OVERLAP, 0),
        (0, TILE - OVERLAP),
        (TILE - OVERLAP, TILE - OVERLAP),
    ]
    .into_iter()
    .map(|(x, y)| memory_tile(&canvas, width, x, y))
    .collect();
    let output = std::env::temp_dir().join(format!("grana-stitch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    let request = Request {
        tiles,
        output: output.clone(),
        rows: 2,
        columns: 2,
        channel: 0,
        series: 0,
    };
    let summary = stitch(&request).expect("the mosaic stitches");
    assert_eq!(
        (summary.width(), summary.height(), summary.depth()),
        (width as u32, width as u32, DEPTH)
    );
    assert!(summary.levels() > 1, "the mosaic carries a pyramid");
    assert_eq!(
        summary.offsets(),
        &[
            [0, 0],
            [(TILE - OVERLAP) as i64, 0],
            [0, (TILE - OVERLAP) as i64],
            [(TILE - OVERLAP) as i64, (TILE - OVERLAP) as i64],
        ]
    );
    let image = Image::open_with_cache(&Source::file(output.clone()), None)
        .expect("the stitched image opens");
    let extent = image.pyramid().level(0).extent();
    assert_eq!(
        (extent.x(), extent.y(), extent.z()),
        (width as u32, width as u32, DEPTH)
    );
    let ranges = [0..extent.x(), 0..extent.y(), 0..extent.z()];
    let data = image
        .read_region(0, 0, 0, &ranges, [0.0; 3])
        .expect("the stitched image reads");
    let side = width as u32;
    for (x, y) in [(4u32, 4u32), (150, 40), (200, 100), (side - 4, side - 4)] {
        let stitched = data.value((x + side * (y + side)) as usize);
        let expected = canvas[(x as usize) + width * (y as usize + width)];
        assert!(
            (stitched - expected).abs() <= 1.0e-5 * expected.abs().max(1.0),
            "the stitched voxel at {x},{y} holds {stitched} instead of {expected}"
        );
    }
    let coarse = image.pyramid().level(1).extent();
    let ranges = [0..coarse.x(), 0..coarse.y(), 0..coarse.z()];
    let reduced = image
        .read_region(0, 1, 0, &ranges, [0.0; 3])
        .expect("the coarse level reads");
    for (x, y, z) in [(1u32, 1u32, 0u32), (60, 20, 1), (100, 90, 1)] {
        let mut sum = 0.0f32;
        let mut count = 0.0f32;
        for dz in 0..2u32 {
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    let voxel = [
                        (2 * x + dx).min(side - 1),
                        (2 * y + dy).min(side - 1),
                        (2 * z + dz).min(DEPTH - 1),
                    ];
                    sum += data.value((voxel[0] + side * (voxel[1] + side * voxel[2])) as usize);
                    count += 1.0;
                }
            }
        }
        let held = reduced.value((x + coarse.x() * (y + coarse.y() * z)) as usize);
        assert!(
            (held - sum / count).abs() <= 1.0e-5 * (sum / count).abs().max(1.0),
            "the coarse voxel at {x},{y},{z} holds {held} instead of {}",
            sum / count
        );
    }
    let _ = std::fs::remove_dir_all(&output);
}

fn crop(values: &[f32], width: usize, x: usize, y: usize) -> Plane {
    Plane::new(
        TILE,
        TILE,
        (0..TILE * TILE)
            .map(|index| {
                let (column, row) = (index % TILE, index / TILE);
                values[(column + x) + width * (row + y)]
            })
            .collect(),
    )
}

fn memory_tile(values: &[f32], width: usize, x: usize, y: usize) -> Source {
    let mut data = Vec::with_capacity(TILE * TILE * DEPTH as usize);
    for z in 0..DEPTH as usize {
        for row in 0..TILE {
            for column in 0..TILE {
                data.push(values[(column + x) + width * (row + y) + width * width * z]);
            }
        }
    }
    let extent = Extent::new(TILE as u32, TILE as u32, DEPTH);
    let channel = Channel::new("structure", [0.2, 0.8, 0.4], VoxelData::F32(data));
    let volume = Volume::new(extent, Spacing::new(0.5, 0.5, 1.0), vec![channel]);
    Source::memory(VolumeSeries::new(vec![volume], 1.0))
}

fn pattern(x: usize, y: usize) -> f32 {
    noise(x as u64, 0x9E37_79B9_7F4A_7C15) + 0.5 * noise(y as u64, 0xC2B2_AE3D_27D4_EB4F)
}

fn noise(index: u64, salt: u64) -> f32 {
    let mut value = index.wrapping_mul(salt) ^ salt.rotate_left(17);
    value ^= value >> 29;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 32;
    (value & 0xffff) as f32
}
