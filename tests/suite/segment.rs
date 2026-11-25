use grana_segment::{Settings, Volume};

fn two_objects() -> Volume {
    let labels = vec![1, 1, 0, 2, 1, 1, 0, 2, 1, 1, 0, 2, 1, 1, 0, 2];
    Volume::new(4, 2, 2, labels)
}

#[test]
fn keeps_the_components_that_hold_the_seeds() {
    let mut volume = two_objects();
    assert_eq!(volume.count(), 2);
    let components = volume.retain_containing(&[[0, 0, 0]]);
    assert_eq!(components.len(), 1, "the seed keeps one object");
    assert_eq!(volume.count(), 1, "the other object is dropped");
    assert_eq!(components[0].center(), [0.5, 0.5, 0.5]);
    let held = volume
        .labels()
        .iter()
        .enumerate()
        .filter(|(_, label)| **label > 0)
        .collect::<Vec<_>>();
    assert_eq!(held.len(), 8, "the kept object spans both slices");
    assert!(held.iter().all(|(_, label)| **label == 1));

    let mut volume = two_objects();
    let components = volume.retain_containing(&[[3, 0, 0]]);
    assert_eq!(
        components.len(),
        1,
        "the second seed keeps the other object"
    );
    assert_eq!(components[0].center(), [3.0, 0.5, 0.5]);
    assert_eq!(volume.labels()[3], 1, "the kept object is renumbered");
    assert_eq!(volume.labels()[0], 0, "the dropped object is cleared");

    let mut volume = two_objects();
    assert!(
        volume.retain_containing(&[[2, 1, 1]]).is_empty(),
        "a seed in the background keeps no object"
    );
    assert_eq!(volume.count(), 0);
    assert!(volume.labels().iter().all(|label| *label == 0));
}

#[test]
fn defaults_follow_the_volume_dimensionality() {
    let flat = Settings::for_depth(1);
    assert_eq!(flat.density_threshold(), 30.0);
    assert_eq!(flat.minimum_size(), 50);
    let volume = Settings::for_depth(8);
    assert_eq!(volume.density_threshold(), 80.0);
    assert_eq!(volume.minimum_size(), 500);
    assert!(Settings::new(1.5, 30.0, 50).is_err());
    assert!(Settings::new(0.5, 30.0, 10_001).is_err());
}
