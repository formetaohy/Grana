use std::sync::atomic::AtomicBool;

use grana_analysis::TrackLinker;
use grana_evaluation::{Evaluation, Phantom, Spec};
use grana_object::{TrackEvents, TrackParams};

fn evaluate(phantom: &Phantom, params: TrackParams) -> Evaluation {
    let frames = phantom
        .detections()
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let set = TrackLinker::new(params)
        .link(&frames, 1.0, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the tracker solves the phantom");
    Evaluation::of(phantom.truth(), &set, 1.0)
}

#[test]
fn scores_tracking_against_a_known_phantom() {
    let phantom = Phantom::of(
        Spec::new(24, 7)
            .with_cells(8)
            .with_step(0.2)
            .with_division(0.08)
            .with_miss(0.03)
            .with_false_alarm(0.2),
    );
    let params = TrackParams::new(1.5, 1, TrackEvents::new(true, 4.5, true, 1.5, 0.0));
    let evaluation = evaluate(&phantom, params);
    let cost = evaluation.cost();
    assert!(
        evaluation.detection() >= 0.95,
        "the phantom detects {:.3} of {} references with {} spurious and {} missing",
        evaluation.detection(),
        cost.vertices(),
        cost.extra(),
        cost.missing()
    );
    assert!(
        evaluation.tracking() >= 0.93,
        "the phantom tracks {:.3} of {} links with {} missing, {} spurious and {} altered",
        evaluation.tracking(),
        cost.edges(),
        cost.added(),
        cost.dropped(),
        cost.altered()
    );
    assert!(
        evaluation.linkage() >= 0.90,
        "the phantom links {:.3} of its lineage",
        evaluation.linkage()
    );
    let divisions = evaluation.divisions();
    assert!(
        divisions.true_positive() > 0,
        "the phantom recovers no division"
    );
    assert!(
        divisions.f1() >= 0.80,
        "the phantom divides at {:.3} with {} true, {} spurious and {} missed",
        divisions.f1(),
        divisions.true_positive(),
        divisions.false_positive(),
        divisions.false_negative()
    );
}
