use std::time::{Duration, Instant};

use grana_exec::{Executor, Pipe, Poll, Role};

const DEADLINE: Duration = Duration::from_secs(10);

fn receive<T>(pipe: &Pipe<T>) -> Option<T> {
    let start = Instant::now();
    loop {
        match pipe.poll() {
            Poll::Event(event) => return Some(event),
            Poll::Empty => {
                assert!(
                    start.elapsed() < DEADLINE,
                    "a submitted job reports within the deadline"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Poll::Closed => return None,
        }
    }
}

#[test]
fn a_panicking_job_keeps_its_lane_serving_and_closes_its_pipe() {
    let executor = Executor::new(Role::Compute, 1);
    let (sender, pipe) = Pipe::new(4);
    executor.submit(move |_| {
        let _ = sender.send(1u8);
        panic!("a job panics");
    });
    assert_eq!(receive(&pipe), Some(1));
    assert_eq!(
        receive(&pipe),
        None,
        "the event pipe of an unwinding job disconnects"
    );

    let (sender, pipe) = Pipe::new(4);
    executor.submit(move |_| {
        let _ = sender.send(2u8);
    });
    assert_eq!(receive(&pipe), Some(2), "the lane serves its next job");
    assert_eq!(receive(&pipe), None);
}
