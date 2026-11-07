use std::path::Path;

use grana_segment::Model;

#[test]
fn runs_a_tiny_onnx_model() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiny.onnx");
    let mut model = Model::open(&path).expect("the fixture model opens");
    assert_eq!(model.info().side(), 256);
    let flow = model
        .infer(&vec![0.5f32; 3 * 256 * 256])
        .expect("the fixture model runs");
    assert!((flow.dy()[0] - 0.25).abs() < 1.0e-6);
    assert!((flow.cellprob()[0] - 0.25).abs() < 1.0e-6);
}
