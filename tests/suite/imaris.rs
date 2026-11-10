use std::path::PathBuf;

use grana_readout::{ImportedFrame, ImportedKind, imported};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn reads_imaris_filaments() {
    let imported = imported(&fixture("imaris-filaments.ims")).expect("the filament fixture opens");
    let objects = imported.objects();
    assert_eq!(objects.len(), 1, "the scene groups merge by identifier");
    let object = &objects[0];
    assert_eq!(object.name(), "Filaments 1");
    assert_eq!(object.kind(), ImportedKind::Filaments);
    let frames = object.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, 0);
    let ImportedFrame::Filaments(filaments) = &frames[0].1 else {
        panic!("the frame holds filaments");
    };
    assert_eq!(filaments.len(), 1);
    let filament = &filaments[0];
    assert_eq!(
        filament.positions(),
        &[[-2.0, -2.0, 0.0], [0.0, -2.0, 0.0], [2.0, -2.0, 0.0]]
    );
    assert_eq!(filament.radii(), &[0.5, 0.25, 0.25]);
    assert_eq!(filament.edges(), &[[0, 1], [1, 2]]);
    assert!(imported.diagnostics().is_empty());
}

#[test]
fn reads_imaris_points() {
    let imported = imported(&fixture("imaris-points.ims")).expect("the point fixture opens");
    let objects = imported.objects();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].kind(), ImportedKind::Spots);
    let ImportedFrame::Spots(spots) = &objects[0].frames()[0].1 else {
        panic!("the frame holds spots");
    };
    assert_eq!(spots.len(), 2);
    assert_eq!(spots[0].center(), [-2.0, -1.0, 0.0]);
    assert_eq!(spots[0].radius(), 0.5);
    assert_eq!(spots[1].center(), [2.0, -1.0, 0.0]);
    assert_eq!(spots[1].radius(), 0.75);
}

#[test]
fn reads_legacy_filaments_and_reports_a_block_model() {
    let imported = imported(&fixture("imaris-blocks.ims")).expect("the block fixture opens");
    let objects = imported.objects();
    assert_eq!(
        objects.len(),
        1,
        "the legacy graphs stand in for the object"
    );
    let object = &objects[0];
    assert_eq!(object.name(), "Filaments 2");
    let ImportedFrame::Filaments(filaments) = &object.frames()[0].1 else {
        panic!("the frame holds filaments");
    };
    assert_eq!(filaments[0].positions().len(), 2);
    assert_eq!(filaments[0].edges(), &[[0, 1]]);
    let diagnostics = imported.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].name(), "Surface 1");
    assert!(diagnostics[0].detail().contains("block model"));
}

#[test]
fn reads_an_imaris_scene() {
    let imported = imported(&fixture("imaris.ims")).expect("the Imaris fixture opens");
    assert_eq!(imported.center(), [2.0, 2.0, 2.0]);
    let objects = imported.objects();
    assert_eq!(objects.len(), 1);
    let object = &objects[0];
    assert_eq!(object.name(), "Nuclei");
    assert_eq!(object.kind(), ImportedKind::Surfaces);
    let frames = object.frames();
    assert_eq!(frames.len(), 2);
    let ImportedFrame::Surfaces(first) = &frames[0].1 else {
        panic!("the first frame holds surfaces");
    };
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].vertices().len(), 3);
    assert_eq!(first[0].vertices()[0], [-2.0, -2.0, -2.0]);
    assert_eq!(first[0].vertices()[1], [0.0, -2.0, -2.0]);
    assert_eq!(first[0].triangles()[0], [0, 1, 2]);
    assert_eq!(first[0].normals()[1], [0.0, 1.0, 0.0]);
    let intensities = first[0].intensities().expect("statistics are attached");
    assert_eq!(intensities.len(), 1);
    assert_eq!(intensities[0].mean(), 42.0);
    assert_eq!(intensities[0].minimum(), 10.0);
    assert_eq!(intensities[0].maximum(), 90.0);
    assert_eq!(intensities[0].deviation(), 3.0);
    assert_eq!(intensities[0].sum(), 420.0);
    let ImportedFrame::Surfaces(second) = &frames[1].1 else {
        panic!("the second frame holds surfaces");
    };
    assert_eq!(second[0].vertices().len(), 4);
    assert_eq!(second[0].triangles().len(), 2);
    assert_eq!(
        second[0].intensities().expect("statistics are attached")[0].mean(),
        24.0
    );
}
