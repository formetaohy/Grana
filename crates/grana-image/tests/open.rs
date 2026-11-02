use std::path::PathBuf;

use grana_image::{Format, Image, Source, is_image};
use grana_volume::{Extent, VoxelData};

#[test]
fn opens_a_tiff_file_through_the_source_layer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plane.tif");
    assert!(is_image(&path));
    let image = Image::open(&Source::file(path)).expect("the tiff opens");
    assert_eq!(image.format(), Some(Format::Tiff));
    assert_eq!(image.name(), "plane");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let data = image
        .read_region(0, 0, 0, &[0..4, 0..3, 0..1])
        .expect("the plane reads");
    let VoxelData::U8(values) = data else {
        panic!("the fixture stores bytes");
    };
    for y in 0..3u32 {
        for x in 0..4u32 {
            assert_eq!(values[(y * 4 + x) as usize], (x + 10 * y) as u8);
        }
    }
}
