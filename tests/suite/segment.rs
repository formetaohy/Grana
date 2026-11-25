use grana_segment::{Segmenter, Settings, Volume};

fn planes(width: u32, height: u32, depth: u32) -> Vec<Vec<f32>> {
    let voxels = (width * height) as usize;
    (0..depth)
        .map(|z| {
            let mut noise = noise(z);
            (0..voxels)
                .map(|index| {
                    let x = (index % width as usize) as f32 / width as f32;
                    let y = (index / width as usize) as f32 / height as f32;
                    let blob = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
                    (1.0 - blob).max(0.0) + noise()
                })
                .collect()
        })
        .collect()
}

fn noise(seed: u32) -> impl FnMut() -> f32 {
    let mut state = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / 16_777_216.0 * 0.2
    }
}

fn segmenter() -> Option<(Segmenter, std::path::PathBuf)> {
    grana_gpu::Headless::shared().ok()?;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let path = grana_weight::weights(&cancel, &mut |_, _| {}).ok()?;
    let segmenter = Segmenter::open(&path, Settings::for_depth(8)).ok()?;
    Some((segmenter, path))
}

#[test]
fn splitting_the_decode_keeps_every_label() {
    let Some((segmenter, _)) = segmenter() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let (width, height, depth) = (256, 256, 8);
    let planes = planes(width, height, depth);
    let whole = segmenter
        .plan(8, u64::MAX)
        .expect("the whole plan fits an unbounded budget");
    let split = segmenter
        .plan(8, 4 << 30)
        .expect("the split plan fits four gibibytes");
    assert!(
        split.bytes() < whole.bytes(),
        "a bounded budget splits the decode further than an unbounded one"
    );
    let mut progress = |_| {};
    let coarse = segmenter
        .segment(&planes, width, height, whole, &mut progress)
        .expect("the whole decode segments");
    let fine = segmenter
        .segment(&planes, width, height, split, &mut progress)
        .expect("the split decode segments");
    assert_eq!(coarse.width(), fine.width());
    assert_eq!(coarse.height(), fine.height());
    assert_eq!(coarse.depth(), fine.depth());
    assert_eq!(
        coarse.labels(),
        fine.labels(),
        "the stripes decode every voxel exactly like the whole surface"
    );
}

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
