use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use rayon::{ThreadPool, ThreadPoolBuilder, Yield};

pub(crate) const WORKER_LIMIT: usize = 16;

const BUDGET_POLL: Duration = Duration::from_millis(20);

fn pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        ThreadPoolBuilder::new()
            .num_threads(workers(0))
            .thread_name(|index| format!("grana-worker-{index}"))
            .build()
            .expect("the analysis worker pool starts")
    })
}

pub fn parallel<'scope, R>(body: impl FnOnce(&rayon::Scope<'scope>) -> R) -> R {
    pool().in_place_scope(body)
}

pub fn receive<T>(receiver: &mpsc::Receiver<T>) -> Option<T> {
    if rayon::current_thread_index().is_none() {
        return receiver.recv().ok();
    }
    loop {
        match receiver.try_recv() {
            Ok(value) => return Some(value),
            Err(mpsc::TryRecvError::Empty) => match rayon::yield_now() {
                Some(Yield::Executed) => {}
                Some(Yield::Idle) | None => std::thread::yield_now(),
            },
            Err(mpsc::TryRecvError::Disconnected) => return None,
        }
    }
}

pub(crate) struct Budget {
    limit: u64,
    used: Mutex<u64>,
    released: Condvar,
}

impl Budget {
    pub(crate) fn new(limit: u64) -> Self {
        assert!(limit > 0, "a budget covers at least one byte");
        Self {
            limit,
            used: Mutex::new(0),
            released: Condvar::new(),
        }
    }

    pub(crate) fn acquire(&self, bytes: u64, halted: impl Fn() -> bool) -> bool {
        assert!(
            bytes <= self.limit,
            "a unit of work fits the shared array budget"
        );
        let mut used = self.used.lock().unwrap_or_else(|error| error.into_inner());
        while *used + bytes > self.limit {
            if halted() {
                return false;
            }
            let (guard, _) = self
                .released
                .wait_timeout(used, BUDGET_POLL)
                .unwrap_or_else(|error| error.into_inner());
            used = guard;
        }
        *used += bytes;
        true
    }

    pub(crate) fn release(&self, bytes: u64) {
        let mut used = self.used.lock().unwrap_or_else(|error| error.into_inner());
        assert!(
            *used >= bytes,
            "a released reservation was acquired beforehand"
        );
        *used -= bytes;
        drop(used);
        self.released.notify_all();
    }
}

pub fn workers(requested: usize) -> usize {
    match requested {
        0 => std::thread::available_parallelism().map_or(1, |value| value.get()),
        value => value,
    }
    .clamp(1, WORKER_LIMIT)
}

pub(crate) struct Run<'a> {
    pub total: usize,
    pub workers: usize,
    pub cancel: &'a AtomicBool,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

const HELD_PER_WORKER: usize = 4;

pub(crate) fn ordered<L, T>(
    run: Run<'_>,
    local: impl Fn() -> L + Sync,
    work: impl Fn(&mut L, usize) -> Result<T, String> + Sync,
    mut sink: impl FnMut(usize, T) -> Result<(), String>,
    mut finish: impl FnMut(L),
) -> Result<bool, String>
where
    L: Send,
    T: Send,
{
    let Run {
        total,
        workers,
        cancel,
        progress,
    } = run;
    if total == 0 {
        return Ok(true);
    }
    let workers = workers.max(1).min(total);
    let window = workers.saturating_mul(HELD_PER_WORKER).min(total);
    let mut states = (0..workers).map(|_| local()).collect::<Vec<_>>();
    let mut done = 0usize;
    let mut base = 0usize;
    let mut failure = None;
    while base < total {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let end = (base + window).min(total);
        let next = AtomicUsize::new(base);
        let slots = Mutex::new((0..end - base).map(|_| None).collect::<Vec<Option<T>>>());
        let error = Mutex::new(None::<String>);
        parallel(|scope| {
            for state in states.iter_mut() {
                let next = &next;
                let slots = &slots;
                let error = &error;
                let work = &work;
                scope.spawn(move |_| {
                    loop {
                        if cancel.load(Ordering::Relaxed) || error_is_some(error) {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= end {
                            break;
                        }
                        match work(state, index) {
                            Ok(value) => {
                                slots.lock().unwrap_or_else(|error| error.into_inner())
                                    [index - base] = Some(value);
                            }
                            Err(detail) => {
                                *error.lock().unwrap_or_else(|error| error.into_inner()) =
                                    Some(detail);
                                break;
                            }
                        }
                    }
                });
            }
        });
        let collected =
            std::mem::take(&mut *slots.lock().unwrap_or_else(|error| error.into_inner()));
        let mut missing = false;
        for (offset, value) in collected.into_iter().enumerate() {
            let Some(value) = value else {
                missing = true;
                break;
            };
            if let Err(detail) = sink(base + offset, value) {
                failure = Some(detail);
                break;
            }
            done += 1;
            progress(done, total);
        }
        if failure.is_none() {
            failure = error
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
        }
        if missing || failure.is_some() {
            break;
        }
        base = end;
    }
    for state in states {
        finish(state);
    }
    if done == total {
        return Ok(true);
    }
    if let Some(detail) = failure {
        return Err(detail);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    Err("a worker stopped before its unit of work was completed".to_string())
}

fn error_is_some(error: &Mutex<Option<String>>) -> bool {
    error
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .is_some()
}

pub(crate) fn map<T, R>(items: &[T], workers: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, WORKER_LIMIT).min(items.len());
    if workers == 1 {
        return items.iter().map(&work).collect();
    }
    let chunk = items.len().div_ceil(workers);
    let mut slots = (0..items.len()).map(|_| None).collect::<Vec<_>>();
    parallel(|scope| {
        let work = &work;
        for (slot, group) in slots.chunks_mut(chunk).zip(items.chunks(chunk)) {
            scope.spawn(move |_| {
                for (target, item) in slot.iter_mut().zip(group) {
                    *target = Some(work(item));
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|value| value.expect("every item is mapped"))
        .collect()
}
