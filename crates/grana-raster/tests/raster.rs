use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_raster::{Grid, Raster, composite, read, write};

#[test]
fn round_trips_decodes_and_tiles_rasters() {
    let root = scratch("grana-raster");
    let file = root.join("red.png");
    let red = Raster::solid(4, 2, [255, 0, 0, 255]);
    write(&file, &red).expect("the raster writes");
    assert_eq!(read(&file).expect("the raster reads"), red);

    let green = Raster::solid(4, 2, [0, 255, 0, 255]);
    let blue = Raster::solid(4, 2, [0, 0, 255, 255]);
    let grid = Grid::new([8, 8], 3);
    let sheet = grid.compose(&[red.clone(), green, blue]);
    assert_eq!(sheet.size(), [48, 48], "the sheet wraps three tiles twice");
    assert_eq!(sheet.pixel(16, 16), [255, 0, 0, 255]);
    assert_eq!(sheet.pixel(32, 16), [0, 255, 0, 255]);
    assert_eq!(sheet.pixel(16, 32), [0, 0, 255, 255]);
    assert_eq!(
        sheet.pixel(0, 0),
        [18, 20, 24, 255],
        "the margin stays empty"
    );

    let halved = Raster::new(
        2,
        2,
        vec![
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ],
    )
    .scaled(1, 1);
    assert_eq!(halved.pixel(0, 0), [128, 128, 128, 255]);

    let under = Raster::solid(2, 2, [0, 0, 0, 255]);
    let above = Raster::solid(2, 2, [255, 255, 255, 128]);
    let blended = composite(&under, &above);
    assert_eq!(blended.pixel(0, 0)[3], 255, "an opaque sheet stays opaque");
    assert!(
        (i32::from(blended.pixel(0, 0)[0]) - 128).abs() <= 1,
        "a half transparent layer blends"
    );
    std::fs::remove_dir_all(&root).expect("the scratch folder is released");
}

fn scratch(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&path).expect("the scratch folder is created");
    path
}

#[test]
fn decodes_a_grayscale_snapshot() {
    let root = scratch("grana-raster-gray");
    let file = root.join("gray.png");
    write_gray(&file, 2, 1, &[0, 255]);
    let raster = read(&file).expect("a grayscale image reads");
    assert_eq!(raster.size(), [2, 1]);
    assert_eq!(raster.pixel(0, 0), [0, 0, 0, 255]);
    assert_eq!(raster.pixel(1, 0), [255, 255, 255, 255]);
    std::fs::remove_dir_all(&root).expect("the scratch folder is released");
}

fn write_gray(path: &Path, width: u32, height: u32, samples: &[u8]) {
    let mut buffer = Vec::new();
    let mut encoder = png::Encoder::new(&mut buffer, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the header writes");
    writer.write_image_data(samples).expect("the pixels write");
    writer.finish().expect("the image finishes");
    std::fs::write(path, buffer).expect("the image is written");
}
