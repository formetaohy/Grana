use std::path::PathBuf;

use grana_readout::{ImportedFrame, ImportedKind, imported};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/imaris.ims")
}

#[test]
fn reads_an_imaris_scene() {
    let imported = imported(&fixture()).expect("the Imaris fixture opens");
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
