use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, SectionState};
use grana_volume::Axis;

#[test]
fn samples_a_timeline_between_keyframes() {
    let mut timeline = Timeline::new(20, 10.0);
    timeline.insert(key(0.0, 0, 0.0, 4.0, Axis::Z));
    timeline.insert(key(10.0, 100, 4.0, 8.0, Axis::X));
    let start = timeline.sample(0.0).expect("the timeline starts");
    assert_eq!(start.time, 0);
    assert_eq!(start.camera.yaw, 0.0);
    let middle = timeline.sample(5.0).expect("the timeline blends");
    assert!((middle.camera.yaw - 2.0).abs() < 1.0e-4);
    assert!((middle.camera.distance - 6.0).abs() < 1.0e-4);
    assert_eq!(middle.section.axis, Axis::X);
    assert_eq!(middle.time, 50);
    let end = timeline.sample(12.0).expect("the timeline holds its end");
    assert_eq!(end.time, 100);
    assert_eq!(end.camera.yaw, 4.0);
    assert_eq!(timeline.frames(), 200);
    assert_eq!(
        timeline.sample_frame(199).map(|sample| sample.time),
        Some(100)
    );
    assert!(timeline.sample_frame(200).is_none());
    let replaced = timeline.insert(key(5.0, 40, 1.0, 2.0, Axis::Y));
    assert_eq!(replaced, 1);
    assert_eq!(timeline.keys().len(), 3);
    assert_eq!(timeline.keys()[1].time, 40);
}

fn key(at: f32, time: u32, yaw: f32, distance: f32, axis: Axis) -> Keyframe {
    Keyframe {
        at,
        time,
        camera: CameraState {
            pivot: [0.0; 3],
            yaw,
            pitch: 0.0,
            distance,
            fov_y: 0.6,
        },
        section: SectionState { axis, span: 1.0 },
    }
}
