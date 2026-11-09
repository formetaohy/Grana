use grana_weight::{License, ModelVersion, license};

#[test]
fn licenses_every_model_asset() {
    let expected = if cfg!(feature = "commercial") {
        None
    } else {
        Some(License::NonCommercial)
    };
    assert_eq!(license(ModelVersion::Cpsam), expected);
    assert_eq!(license(ModelVersion::CpsamV2), expected);
}
