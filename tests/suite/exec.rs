use std::time::{Duration, Instant};

use grana_exec::{Executor, Pipe, Poll, Priority, Role};

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
fn a_batch_runs_every_item_once_across_its_shards() {
    let executor = Executor::new(Role::Read, 4);
    let (sender, pipe) = Pipe::new(1024);
    executor.submit_batch(
        Priority::Demand,
        (0..200usize).collect(),
        7,
        move |_, item| {
            let _ = sender.send(item);
        },
    );
    let mut delivered = Vec::new();
    while delivered.len() < 200 {
        delivered.push(receive(&pipe).expect("a batch reports every item"));
    }
    delivered.sort_unstable();
    assert_eq!(
        delivered,
        (0..200usize).collect::<Vec<_>>(),
        "a batch runs each item once"
    );

    let ordered = Executor::new(Role::Read, 1);
    let (sender, pipe) = Pipe::new(1024);
    ordered.submit_batch(
        Priority::Demand,
        (0..200usize).collect(),
        4,
        move |_, item| {
            let _ = sender.send(item);
        },
    );
    let mut sequence = Vec::new();
    while sequence.len() < 200 {
        sequence.push(receive(&pipe).expect("a batch reports every item"));
    }
    assert_eq!(
        sequence,
        (0..200usize).collect::<Vec<_>>(),
        "one lane serves its bricks in the order they were requested"
    );
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
