use std::sync::atomic::AtomicBool;

use grana_analysis::TrackLinker;
use grana_evaluation::{Evaluation, Phantom, Spec};
use grana_object::{TrackEvents, TrackParams, TrackSet};

fn params() -> TrackParams {
    TrackParams::new(1.5, 1, TrackEvents::new(true, 4.5, true, 1.5, 0.0))
}

fn link(phantom: &Phantom) -> TrackSet {
    let frames = phantom
        .detections()
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    TrackLinker::new(params())
        .link(&frames, 1.0, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the tracker solves the phantom")
}

#[test]
#[ignore = "a measurement harness run explicitly with --ignored"]
fn sweeps_division_and_dropout_configurations() {
    let divisions = [0.06, 0.1, 0.2, 0.3, 0.4];
    let misses = [0.03, 0.1, 0.2, 0.3];
    let mut means = [0.0f64; 4];
    let mut count = 0usize;
    for division in divisions {
        let mut row = String::new();
        for miss in misses {
            let phantom = Phantom::of(
                Spec::new(24, 7)
                    .with_cells(10)
                    .with_step(0.2)
                    .with_division(division)
                    .with_miss(miss)
                    .with_false_alarm(0.2),
            );
            let evaluation = Evaluation::of(phantom.truth(), &link(&phantom), 1.0);
            let divisions_branch = evaluation.divisions();
            row.push_str(&format!(
                " | d{division:.2}/m{miss:.2} TRA {:.4} DET {:.4} link {:.4} divF1 {:.4} (+{}/{}/{})",
                evaluation.tracking(),
                evaluation.detection(),
                evaluation.linkage(),
                divisions_branch.f1(),
                evaluation.cost().added(),
                evaluation.cost().dropped(),
                evaluation.cost().altered(),
            ));
            means[0] += evaluation.tracking();
            means[1] += evaluation.detection();
            means[2] += evaluation.linkage();
            means[3] += divisions_branch.f1();
            count += 1;
        }
        println!("{row}");
    }
    let count = count as f64;
    println!(
        "MEAN TRA {:.4} DET {:.4} linkage {:.4} divF1 {:.4} over {count} configurations",
        means[0] / count,
        means[1] / count,
        means[2] / count,
        means[3] / count,
    );
}
