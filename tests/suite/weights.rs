use grana_weight::{License, ModelVersion, license};

#[test]
fn licenses_every_model_asset() {
    for model in ModelVersion::ALL {
        assert_eq!(model.license(), License::NonCommercial);
        let expected = model.licensed().then_some(License::NonCommercial);
        assert_eq!(license(model), expected);
    }
    assert_eq!(
        ModelVersion::preferred().is_some(),
        !cfg!(feature = "commercial"),
        "a commercial build carries no commercially licensed weight yet"
    );
}
