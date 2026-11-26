use std::path::PathBuf;
use std::sync::Arc;

use grana_gpu::{Consumer, Ledger};
use grana_segment::{Plan, Segmenter, Settings, Volume};

const SIDE: u32 = 256;

const DEPTHS: [u32; 8] = [8, 7, 6, 5, 4, 3, 2, 1];

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

struct Bench {
    segmenter: Segmenter,
    weights: PathBuf,
    ledger: Arc<Ledger>,
}

fn bench() -> Option<Bench> {
    let headless = grana_gpu::Headless::shared().ok()?;
    let ledger = Ledger::of(headless.memory());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let weights = grana_weight::weights(&cancel, &mut |_, _| {}).ok()?;
    let segmenter = Segmenter::open(&weights, Settings::for_depth(DEPTHS[0])).ok()?;
    Some(Bench {
        segmenter,
        weights,
        ledger,
    })
}

impl Bench {
    fn afford(&self) -> Option<(u32, Plan, Plan)> {
        let spare = self.ledger.spare();
        for depth in DEPTHS {
            let Ok(wide) = self.segmenter.plan(depth, spare) else {
                continue;
            };
            let mut tight = wide;
            let mut budget = wide.bytes() / 4 * 3;
            while budget > 0 {
                if let Ok(held) = self.segmenter.plan(depth, budget) {
                    tight = held;
                }
                budget /= 2;
            }
            if tight.bytes() < wide.bytes() {
                return Some((depth, wide, tight));
            }
        }
        None
    }

    fn run(
        &self,
        segmenter: &Segmenter,
        depth: u32,
        plan: Plan,
        progress: &mut dyn FnMut(usize),
    ) -> Volume {
        let planes = planes(SIDE, SIDE, depth);
        let claim = self
            .ledger
            .claim(Consumer::Inference, plan.bytes())
            .expect("a planned decode claims the graphics memory it declares");
        let volume = segmenter
            .segment(&planes, SIDE, SIDE, plan, progress)
            .expect("the planned decode segments the volume");
        drop(claim);
        volume
    }
}

#[test]
fn splitting_the_decode_keeps_every_label() {
    let Some(bench) = bench() else {
        println!("no graphics device or weights, skipping");
        return;
    };
    let Some((depth, wide, tight)) = bench.afford() else {
        println!(
            "the graphics device holds {:.1} GiB, less than two decodes of the model need, skipping",
            bench.ledger.spare() as f64 / (1u64 << 30) as f64
        );
        return;
    };
    assert!(
        tight.bytes() < wide.bytes(),
        "a tighter budget splits the decode further than {:.1} GiB",
        wide.bytes() as f64 / (1u64 << 30) as f64
    );
    let segmenter = Segmenter::open(&bench.weights, Settings::for_depth(depth))
        .expect("the weights stay resident");
    let idle = segmenter
        .graphics_memory()
        .expect("the decoder reports the pages it holds");
    let mut progress = |_| {};
    let split = bench.run(&segmenter, depth, tight, &mut progress);
    let split_footprint = segmenter
        .graphics_memory()
        .expect("the decoder reports the pages it holds")
        .saturating_sub(idle);
    assert!(
        split_footprint <= tight.bytes(),
        "the plan claims {:.1} GiB and the allocator reserved {:.1} GiB more",
        tight.bytes() as f64 / (1u64 << 30) as f64,
        split_footprint as f64 / (1u64 << 30) as f64
    );
    let whole = bench.run(&segmenter, depth, wide, &mut progress);
    assert_eq!(whole.width(), split.width());
    assert_eq!(whole.height(), split.height());
    assert_eq!(whole.depth(), split.depth());
    assert_eq!(
        whole.labels(),
        split.labels(),
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
    assert_eq!(flat.minimum_size(), 50);
    let volume = Settings::for_depth(8);
    assert_eq!(volume.density_threshold(), 80.0);
    assert_eq!(volume.minimum_size(), 500);
    assert!(Settings::new(1.5, 30.0, 50).is_err());
    assert!(Settings::new(0.5, 30.0, 10_001).is_err());
}
