use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

const REPORT: Duration = Duration::from_millis(40);
const THREAD_FLOOR: usize = 2;
const THREAD_CEILING: usize = 8;

pub(crate) fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(THREAD_FLOOR, |available| available.get())
        .clamp(THREAD_FLOOR, THREAD_CEILING)
}

pub(crate) fn write_each(
    count: usize,
    progress: &mut dyn FnMut(usize, usize),
    write: impl Fn(usize) -> Result<u64, String> + Sync,
) -> Result<u64, String> {
    assert!(count > 0, "a written level holds at least one chunk");
    let threads = threads().min(count);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let bytes = AtomicU64::new(0);
    let active = AtomicUsize::new(threads);
    let stopped = AtomicBool::new(false);
    let failure = Mutex::new(None::<String>);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while !stopped.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    match write(index) {
                        Ok(written) => {
                            bytes.fetch_add(written, Ordering::Relaxed);
                            done.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(detail) => {
                            let mut held =
                                failure.lock().unwrap_or_else(|error| error.into_inner());
                            if held.is_none() {
                                *held = Some(detail);
                            }
                            stopped.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
        loop {
            progress(done.load(Ordering::Relaxed), count);
            if active.load(Ordering::Relaxed) == 0 {
                break;
            }
            std::thread::sleep(REPORT);
        }
    });
    progress(done.load(Ordering::Relaxed), count);
    match failure
        .into_inner()
        .unwrap_or_else(|error| error.into_inner())
    {
        Some(detail) => Err(detail),
        None => Ok(bytes.load(Ordering::Relaxed)),
    }
}
