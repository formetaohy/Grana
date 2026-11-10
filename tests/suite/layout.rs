use grana_display::{PANE_GAP, PaneLayout, Panes};

const STAGE: [f32; 4] = [16.0, 60.0, 1000.0, 700.0];

fn overlaps(left: [f32; 4], right: [f32; 4]) -> bool {
    left[0] < right[0] + right[2]
        && right[0] < left[0] + left[2]
        && left[1] < right[1] + right[3]
        && right[1] < left[1] + left[3]
}

#[test]
fn lays_out_one_two_and_four_views() {
    let mut panes = Panes::new();

    panes.set_layout(PaneLayout::Single);
    assert_eq!(panes.count(), 1);
    let rects = panes.rects(STAGE, PANE_GAP);
    assert_eq!(rects[0], STAGE);
    assert!(
        rects[1..]
            .iter()
            .all(|rect| rect[2] == 0.0 && rect[3] == 0.0),
        "a single view hides the other panes"
    );

    panes.set_layout(PaneLayout::Split);
    panes.set_split(0.6);
    assert_eq!(panes.count(), 2);
    let rects = panes.rects(STAGE, PANE_GAP);
    assert_eq!(rects[0][0], STAGE[0]);
    assert_eq!(rects[1][1], STAGE[1]);
    assert!(
        (rects[0][2] + PANE_GAP + rects[1][2] - STAGE[2]).abs() < 1.0e-3,
        "the split shares the stage width around one gap"
    );
    assert!(
        (rects[1][0] - rects[0][0] - rects[0][2] - PANE_GAP).abs() < 1.0e-3,
        "the split leaves one gap between the panes"
    );
    assert_eq!(rects[0][3], STAGE[3], "a split spans the stage height");

    panes.set_layout(PaneLayout::Quad);
    panes.set_row(0.4);
    assert_eq!(panes.count(), 4);
    let rects = panes.rects(STAGE, PANE_GAP);
    for index in 0..4 {
        assert!(rects[index][2] > 0.0 && rects[index][3] > 0.0);
        for other in index + 1..4 {
            assert!(
                !overlaps(rects[index], rects[other]),
                "the quad panes do not overlap"
            );
        }
    }
    assert_eq!(rects[0][1], STAGE[1]);
    assert_eq!(rects[2][1], rects[3][1]);
    assert!(
        (rects[0][3] + PANE_GAP + rects[2][3] - STAGE[3]).abs() < 1.0e-3,
        "the quad shares the stage height around one gap"
    );
    assert_eq!(rects[0][2], rects[2][2], "a quad keeps its columns aligned");
    assert_eq!(rects[1][2], rects[3][2], "a quad keeps its columns aligned");
    panes.set_split(0.2);
    assert!(
        (panes.split() - grana_display::SPLIT_MIN).abs() < f32::EPSILON,
        "a split stays inside its bounds"
    );
    panes.set_row(0.99);
    assert!(
        (panes.row() - grana_display::SPLIT_MAX).abs() < f32::EPSILON,
        "a row stays inside its bounds"
    );
}
