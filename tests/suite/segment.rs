use std::path::PathBuf;
use std::sync::Arc;

use grana_gpu::Ledger;
use grana_segment::{Plan, Segmenter, Settings, Volume};

const SIDE: u32 = 256;

fn planes(width: u32, height: u32, depth: u32) -> Vec<Vec<f32>> {
    let voxels = (width * height) as usize;
    (0..depth)
        .map(|z| {
            let mut noise = noise(z);
            (0..voxels)
                .map(|index| {
                    let x = (index % width as usize) as f32 / width as f32;
                    let y = (index / width as usize) as f32 / height as f32;
                    let drift = (z as f32 * 0.02).sin() * 0.1;
                    let blob = ((x - 0.5 - drift).powi(2) + (y - 0.5).powi(2)).sqrt();
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

struct Bench {
    weights: PathBuf,
    ledger: Arc<Ledger>,
}

fn bench() -> Option<Bench> {
    let headless = grana_gpu::Headless::shared().ok()?;
    let ledger = Ledger::of(headless.memory());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let weights = grana_weight::weights(&cancel, &mut |_, _| {}).ok()?;
    if Segmenter::open(&weights, Settings::for_depth(8)).is_err() {
        return None;
    }
    Some(Bench { weights, ledger })
}

impl Bench {
    fn run(&self, segmenter: &Segmenter, depth: u32, plan: Plan) -> Volume {
        let planes = planes(SIDE, SIDE, depth);
        let mut progress = |_| {};
        segmenter
            .segment(
                &planes,
                SIDE,
                SIDE,
                cell_measure(depth),
                plan,
                &mut progress,
            )
            .expect("the planned decode segments the volume")
    }

    fn opened(&self, depth: u32) -> Segmenter {
        Segmenter::open(&self.weights, Settings::for_depth(depth))
            .expect("the weights stay resident")
    }
}

fn cell_measure(depth: u32) -> f64 {
    if depth > 1 { 0.1 } else { 0.5 }
}

#[test]
fn shallow_volumes_keep_their_labels() {
    let Some(bench) = bench() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let mut failures = Vec::new();
    for depth in [1u32, 4, 8] {
        let segmenter = bench.opened(depth);
        let spare = bench.ledger.spare();
        let plan = segmenter
            .plan(depth, spare)
            .expect("the planner fits the shallow volume inside the budget");
        let volume = bench.run(&segmenter, depth, plan);
        println!(
            "depth {depth}: splits {} labels {}",
            plan.splits(),
            volume.count()
        );
        if volume.count() < 1 {
            failures.push(depth);
        }
    }
    let depth = 2;
    let settings = Settings::new(0.5, 30.0, 0.3).expect("the shallow density fits two slices");
    let segmenter = Segmenter::open(&bench.weights, settings).expect("the weights stay resident");
    let spare = bench.ledger.spare();
    let plan = segmenter
        .plan(depth, spare)
        .expect("the planner fits the shallow volume inside the budget");
    let volume = bench.run(&segmenter, depth, plan);
    println!(
        "depth {depth} with low density: splits {} labels {}",
        plan.splits(),
        volume.count()
    );
    if volume.count() < 1 {
        failures.push(depth);
    }
    assert!(
        failures.is_empty(),
        "the drifting object survives every shallow decode, missing at {failures:?}"
    );
}

#[test]
fn splitting_the_decode_keeps_every_label() {
    let Some(bench) = bench() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let depth = 16;
    let segmenter = bench.opened(depth);
    let wide = segmenter.forced_plan(depth, 8);
    let tight = segmenter.forced_plan(depth, 16);
    let whole = bench.run(&segmenter, depth, wide);
    let split = bench.run(&segmenter, depth, tight);
    assert_eq!(whole.width(), split.width());
    assert_eq!(whole.height(), split.height());
    assert_eq!(whole.depth(), split.depth());
    assert_eq!(
        whole.labels(),
        split.labels(),
        "the stripes decode every voxel exactly like the whole surface"
    );
}

#[test]
fn deep_volumes_segment_inside_the_budget() {
    let Some(bench) = bench() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let spare = bench.ledger.spare();
    for depth in [16u32, 32, 64] {
        let segmenter = bench.opened(depth);
        let Ok(plan) = segmenter.plan(depth, spare) else {
            println!("depth {depth}: the planner finds no split inside the budget");
            continue;
        };
        let volume = bench.run(&segmenter, depth, plan);
        assert_eq!(volume.depth(), depth);
        println!(
            "depth {depth}: splits {} plan {:.2} GiB labels {}",
            plan.splits(),
            plan.bytes() as f64 / (1u64 << 30) as f64,
            volume.count()
        );
    }
}

#[test]
fn tiled_volumes_hold_together_across_the_seams() {
    let Some(bench) = bench() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let (width, height, depth) = (1280u32, 1280u32, 8u32);
    let voxels = (width * height) as usize;
    let radius = 60.0f32;
    let centers = [(768.0f32, 768.0f32), (300.0f32, 300.0f32)];
    let planes: Vec<Vec<f32>> = (0..depth)
        .map(|z| {
            let mut noise = noise(z + 1000);
            (0..voxels)
                .map(|index| {
                    let x = (index % width as usize) as f32;
                    let y = (index / width as usize) as f32;
                    let blob = centers
                        .iter()
                        .map(|(cx, cy)| {
                            let distance = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                            if distance <= radius * 0.8 {
                                1.0
                            } else {
                                ((radius - distance) / (radius * 0.2)).max(0.0)
                            }
                        })
                        .fold(0f32, f32::max);
                    blob + noise()
                })
                .collect()
        })
        .collect();
    let segmenter = bench.opened(depth);
    let spare = bench.ledger.spare();
    let Ok(plan) = segmenter.plan(depth, spare) else {
        println!("the planner finds no split inside the budget");
        return;
    };
    let mut progress = |_| {};
    let volume = segmenter
        .segment(
            &planes,
            width,
            height,
            cell_measure(depth),
            plan,
            &mut progress,
        )
        .expect("the tiled decode segments the volume");
    let at = |x: usize, y: usize, z: usize| {
        volume.labels()[x + width as usize * (y + height as usize * z)]
    };
    let mid = depth as usize / 2;
    let center = at(768, 768, mid);
    let west = at(728, 768, mid);
    let east = at(808, 768, mid);
    let north = at(768, 728, mid);
    let south = at(768, 808, mid);
    println!(
        "tiled {}x{}x{}: labels {} seam center {} west {} east {} north {} south {}",
        width,
        height,
        depth,
        volume.count(),
        center,
        west,
        east,
        north,
        south
    );
    assert!(
        center > 0,
        "the compact object centered on the tile seams is detected"
    );
    assert_eq!(
        west, center,
        "the object keeps one label west across the vertical seam"
    );
    assert_eq!(
        east, center,
        "the object keeps one label east across the vertical seam"
    );
    assert_eq!(
        north, center,
        "the object keeps one label north across the horizontal seam"
    );
    assert_eq!(
        south, center,
        "the object keeps one label south across the horizontal seam"
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
    assert_eq!(components.len(), 1);
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
    assert_eq!(flat.minimum_volume(), 0.0011);
    let volume = Settings::for_depth(8);
    assert_eq!(volume.density_threshold(), 80.0);
    assert_eq!(volume.minimum_volume(), 0.01);
    assert!(Settings::new(1.5, 30.0, 0.0011).is_err());
    assert!(Settings::new(0.5, 30.0, 1_001.0).is_err());
}

#[test]
fn the_minimum_volume_holds_for_every_sampling() {
    for step in [0.4f64, 0.29, 0.2, 0.15, 0.1] {
        let floor = Settings::for_depth(3).minimum_voxel_count(step.powi(3));
        let mut sampled = Volume::new(8, 8, 8, big_and_small());
        sampled.retain_above(floor);
        let [cell, speckle] = counts(&sampled);
        assert_eq!(
            cell, 176,
            "the {step} µm sampling keeps every cell above the physical floor"
        );
        if floor > 8 {
            assert_eq!(
                speckle, 0,
                "the {step} µm sampling drops the speckle that holds less than the floor"
            );
        }
    }
}

fn big_and_small() -> Vec<u32> {
    let mut labels = vec![0u32; 8 * 8 * 8];
    for cell in labels.iter_mut().take(176) {
        *cell = 1;
    }
    for cell in labels.iter_mut().skip(400).take(8) {
        *cell = 2;
    }
    labels
}

fn counts(volume: &Volume) -> [usize; 2] {
    let labels = volume.labels();
    [
        labels.iter().filter(|label| **label == 1).count(),
        labels.iter().filter(|label| **label == 2).count(),
    ]
}
