use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_raster::{Cell, Group, Raster, Row, Sheet, Table, composite, read, write};
use grana_text::Fonts;

#[test]
fn round_trips_decodes_and_labels_rasters() {
    let root = scratch("grana-raster");
    let file = root.join("red.png");
    let red = Raster::solid(4, 2, [255, 0, 0, 255]);
    write(&file, &red).expect("the raster writes");
    assert_eq!(read(&file).expect("the raster reads"), red);

    let halved = Raster::new(
        2,
        2,
        vec![
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ],
    )
    .scaled(1, 1);
    assert_eq!(halved.pixel(0, 0), [188, 188, 188, 255]);

    let under = Raster::solid(2, 2, [0, 0, 0, 255]);
    let above = Raster::solid(2, 2, [255, 255, 255, 128]);
    let blended = composite(&under, &above);
    assert_eq!(blended.pixel(0, 0)[3], 255, "an opaque sheet stays opaque");
    assert!(
        (i32::from(blended.pixel(0, 0)[0]) - 188).abs() <= 1,
        "a half transparent layer blends in linear light"
    );
    let premultiplied = Raster::solid(2, 2, [128, 128, 128, 128]);
    let composited = grana_raster::composite_premultiplied(&under, &premultiplied);
    assert!(
        (i32::from(composited.pixel(0, 0)[0]) - 188).abs() <= 1,
        "a premultiplied layer blends in linear light"
    );

    let green = Raster::solid(8, 8, [0, 255, 0, 255]);
    let blue = Raster::solid(8, 8, [0, 0, 255, 255]);
    let mut fonts = Fonts::system();
    let group = Group {
        name: "group".to_string(),
        table: None,
        cells: vec![
            Cell {
                image: &green,
                title: "green",
                detail: "ok",
                accent: Some([255, 0, 0, 255]),
            },
            Cell {
                image: &blue,
                title: "blue",
                detail: "resumed",
                accent: None,
            },
        ],
    };
    let sheet = Sheet::new([8, 8], 2).compose(&mut fonts, &[group]);
    assert_eq!(sheet.size(), [76, 172], "the sheet lays out its bands");
    assert_eq!(
        sheet.pixel(16, 82),
        [255, 0, 0, 255],
        "a status accent lands beside its cell"
    );
    assert_eq!(
        sheet.pixel(28, 82),
        [0, 255, 0, 255],
        "the first tile lands"
    );
    assert_eq!(
        sheet.pixel(48, 82),
        [0, 0, 255, 255],
        "the second tile lands"
    );
    let ink = (24..62)
        .flat_map(|y| (24..72).map(move |x| (x, y)))
        .filter(|(x, y)| sheet.pixel(*x, *y) != [18, 20, 24, 255])
        .count();
    assert!(ink > 0, "the group heading draws its ink");
    std::fs::remove_dir_all(&root).expect("the scratch folder is released");
}

#[test]
fn lays_out_the_summary_band_of_a_group() {
    let image = Raster::solid(8, 8, [0, 255, 0, 255]);
    let table = Table {
        columns: vec!["Puncta".to_string(), "Nuclei".to_string()],
        rows: vec![
            Row {
                label: "datasets".to_string(),
                values: vec!["2".to_string(), "2".to_string()],
            },
            Row {
                label: "volume µm³".to_string(),
                values: vec!["3.214 ± 0.512".to_string(), "1.000".to_string()],
            },
        ],
    };
    let plain = Group {
        name: "control".to_string(),
        table: None,
        cells: vec![Cell {
            image: &image,
            title: "a",
            detail: "",
            accent: None,
        }],
    };
    let banded = Group {
        name: "control".to_string(),
        table: Some(&table),
        cells: vec![Cell {
            image: &image,
            title: "a",
            detail: "",
            accent: None,
        }],
    };
    let mut fonts = Fonts::system();
    let without = Sheet::new([160, 40], 2).compose(&mut fonts, std::slice::from_ref(&plain));
    let with = Sheet::new([160, 40], 2).compose(&mut fonts, std::slice::from_ref(&banded));
    assert_eq!(
        with.height() - without.height(),
        96,
        "the band adds its header, rows and gap"
    );
    let ink = (78..174)
        .flat_map(|y| (24..184).map(move |x| (x, y)))
        .filter(|(x, y)| with.pixel(*x, *y) != [18, 20, 24, 255])
        .count();
    assert!(ink > 0, "the summary band draws its table");
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
