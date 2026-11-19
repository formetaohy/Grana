use grana_segment::assemble;

fn two_objects() -> grana_segment::Volume {
    let width = 4u32;
    let height = 2u32;
    let first = vec![1, 1, 0, 2, 1, 1, 0, 2];
    let second = vec![3, 3, 0, 0, 3, 3, 0, 0];
    assemble(&mut [first, second], width, height, 0.5, 0)
        .expect("the slices assemble into two objects")
}

#[test]
fn assembles_slices_by_overlap() {
    let width = 4u32;
    let height = 2u32;
    let mut first = vec![0u32; 8];
    let mut second = vec![0u32; 8];
    for index in [0, 1, 4] {
        first[index] = 1;
        second[index] = 1;
    }
    second[2] = 2;
    let volume = grana_segment::assemble(&mut [first, second], width, height, 0.5, 0)
        .expect("the slices assemble");
    assert_eq!(volume.count(), 2, "the overlap joins one object only");
    assert_eq!(
        volume.labels()[0],
        volume.labels()[8],
        "the overlapping object keeps one label across slices"
    );
    assert_ne!(
        volume.labels()[0],
        volume.labels()[10],
        "the unmatched object keeps its own label"
    );
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
    assert_eq!(components[0].center(), [3.0, 0.5, 0.0]);
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
