use std::time::{Duration, Instant};

use grana_app::stream::{Stream, View};

const MIB: f32 = 1024.0 * 1024.0;
const BRICK: u64 = 128 << 10;

fn view(rate: Option<f32>, bricks: usize, rendered: usize, now: Instant) -> View {
    View {
        remote: true,
        levels: 5,
        rate,
        bricks,
        rendered,
        brick_bytes: BRICK,
        now,
    }
}

#[test]
fn the_level_cap_follows_the_measured_throughput() {
    let start = Instant::now();
    let mut stream = Stream::new();
    assert_eq!(
        stream.cap(view(None, 0, 0, start)),
        2,
        "an unmeasured network starts two levels above the finest"
    );

    let fast = 400.0 * MIB;
    assert_eq!(
        stream.cap(view(Some(fast), 64, 2, start)),
        2,
        "a jumped cap never spans more than one level"
    );
    assert_eq!(
        stream.cap(view(Some(fast), 64, 2, start + Duration::from_millis(500))),
        3,
        "the cap refines once the step interval elapses"
    );
    assert_eq!(
        stream.cap(view(Some(fast), 64, 3, start + Duration::from_secs(2))),
        4,
        "a fast link reaches the finest level"
    );

    let slow = 8.0 * MIB;
    assert_eq!(
        stream.cap(view(Some(slow), 512, 4, start + Duration::from_secs(3))),
        1,
        "a slow link falls back to the level its throughput affords"
    );
    assert_eq!(
        stream.cap(view(Some(slow), 0, 1, start + Duration::from_secs(4))),
        1,
        "an idle frame keeps the current cap"
    );

    let mut local = Stream::new();
    assert_eq!(
        local.cap(View {
            remote: false,
            ..view(None, 0, 0, start)
        }),
        4,
        "a local dataset always renders its finest level"
    );
}
