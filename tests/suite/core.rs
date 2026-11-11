use grana_text::{Contour, Curve};
use grana_volume::{
    BrickCoord, BrickGrid, Channel, Extent, Intensity, PADDED_SIDE, Spacing, Volume, VoxelData,
};

#[test]
fn writes_deterministic_vector_pages() {
    let mut page = grana_vector::Page::new(320.0, 200.0);
    page.push(grana_vector::Shape::new(
        grana_vector::rounded_rect(
            grana_vector::Rect::new([8.0, 8.0], [312.0, 192.0]),
            [12.0, 12.0, 12.0, 12.0],
        ),
        grana_vector::Paint::Stroke {
            width: 1.5,
            color: grana_vector::Rgba::new(0.9, 0.2, 0.1, 0.5),
            cap: grana_vector::Cap::Round,
        },
    ));
    page.push(grana_vector::Shape::new(
        grana_vector::segment([20.0, 120.0], [300.0, 120.0]),
        grana_vector::Paint::Gradient(
            grana_vector::Rgba::new(0.1, 0.4, 0.9, 1.0),
            grana_vector::Rgba::new(0.9, 0.4, 0.1, 1.0),
        ),
    ));
    let mut contour = Contour::new([40.0, 40.0]);
    contour.push(Curve::Line([80.0, 40.0]));
    contour.push(Curve::Quad([90.0, 60.0], [80.0, 80.0]));
    contour.push(Curve::Cubic([60.0, 90.0], [50.0, 50.0], [40.0, 40.0]));
    page.push(grana_vector::Shape::new(
        vec![contour],
        grana_vector::Paint::Fill(grana_vector::Rgba::new(0.2, 0.8, 0.3, 1.0)),
    ));
    page.push(grana_vector::Shape::new(
        grana_vector::arc([160.0, 120.0], 40.0, 0.0, 1.5),
        grana_vector::Paint::Stroke {
            width: 2.0,
            color: grana_vector::Rgba::new(0.8, 0.8, 0.8, 1.0),
            cap: grana_vector::Cap::Butt,
        },
    ));
    let svg = grana_vector::write_svg(&page);
    assert_eq!(svg, grana_vector::write_svg(&page));
    assert!(svg.contains("<path d=\"M 40 40 L 80 40 Q 90 60 80 80 C 60 90 50 50 40 40 Z\""));
    assert!(svg.contains("stroke-linecap=\"round\""));
    assert!(svg.contains("stroke-linecap=\"butt\""));
    let arc = svg.split("M 200 120").nth(1).expect("the arc is written");
    let arc = &arc[..arc.find('"').expect("the arc path closes")];
    assert!(
        !arc.contains('Z'),
        "an open arc is stroked without a closing chord"
    );
    let mut document = grana_vector::Document::of(page);
    let pdf = grana_vector::write_pdf(&document);
    assert_eq!(pdf, grana_vector::write_pdf(&document));
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/ShadingType 2"));
    assert!(text.contains("/Count 1"));
    assert!(text.contains("startxref"));
    document.push(grana_vector::Page::new(320.0, 200.0));
    let text = String::from_utf8_lossy(&grana_vector::write_pdf(&document)).into_owned();
    assert!(text.contains("/Count 2"));
    assert_eq!(text.matches("/Type /Page ").count(), 2);
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
    assert_eq!(pyramid.channels()[1].window(), Intensity::new(0.0, 1.0));

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
