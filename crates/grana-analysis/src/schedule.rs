use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use rayon::{ThreadPool, ThreadPoolBuilder};

pub(crate) const WORKER_LIMIT: usize = 16;

const POLL: Duration = Duration::from_millis(20);

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

pub fn workers(requested: usize) -> usize {
    match requested {
        0 => std::thread::available_parallelism().map_or(1, |value| value.get()),
        value => value,
    }
    .clamp(1, WORKER_LIMIT)
}

pub enum Delivery<T> {
    Progress {
        unit: usize,
        done: usize,
        total: usize,
    },
    Unit {
        index: usize,
        value: T,
    },
}

pub struct Run<'a> {
    pub total: usize,
    pub workers: usize,
    pub cancel: &'a AtomicBool,
}

enum Message<T> {
    Unit { index: usize, value: T },
    Failed(String),
}

struct Held {
    slots: Mutex<usize>,
    ready: Condvar,
}

impl Held {
    fn new(slots: usize) -> Self {
        assert!(slots > 0, "an assembly holds at least one unit");
        Self {
            slots: Mutex::new(slots),
            ready: Condvar::new(),
        }
    }

    fn take(&self, stopped: &dyn Fn() -> bool) -> bool {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        while *slots == 0 {
            if stopped() {
                return false;
            }
            let (guard, _) = self
                .ready
                .wait_timeout(slots, POLL)
                .unwrap_or_else(|error| error.into_inner());
            slots = guard;
        }
        *slots -= 1;
        !stopped()
    }

    fn put(&self) {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        *slots += 1;
        drop(slots);
        self.ready.notify_all();
    }

    fn wake(&self) {
        self.ready.notify_all();
    }
}

pub fn ordered<L, T>(
    run: Run<'_>,
    window: usize,
    local: impl Fn() -> L + Sync,
    work: impl Fn(&mut L, usize, &mut dyn FnMut(usize, usize)) -> Result<T, String> + Sync,
    mut deliver: impl FnMut(Delivery<T>) -> Result<(), String>,
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
    } = run;
    if total == 0 {
        return Ok(true);
    }
    let window = window.clamp(1, total);
    let running = workers.clamp(1, total).min(window);
    let next = AtomicUsize::new(0);
    let halted = AtomicBool::new(false);
    let held = Held::new(window);
    let (results, received) = mpsc::sync_channel::<Message<T>>(window);
    let (reports, reported) = mpsc::channel::<(usize, usize, usize)>();
    let mut states = (0..running).map(|_| local()).collect::<Vec<_>>();
    let (installed, failure, denied) = std::thread::scope(|scope| {
        let work = &work;
        let held = &held;
        for state in states.iter_mut() {
            let results = results.clone();
            let reports = reports.clone();
            let next = &next;
            let halted = &halted;
            scope.spawn(move || {
                let stop = || cancel.load(Ordering::Relaxed) || halted.load(Ordering::Relaxed);
                loop {
                    if !held.take(&stop) {
                        return;
                    }
                    if stop() {
                        return;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        return;
                    }
                    let mut notify = |done: usize, units: usize| {
                        let _ = reports.send((index, done, units));
                    };
                    let message = match work(state, index, &mut notify) {
                        Ok(value) => Message::Unit { index, value },
                        Err(detail) => Message::Failed(detail),
                    };
                    if results.send(message).is_err() {
                        return;
                    }
                }
            });
        }
        drop(results);
        drop(reports);
        let mut pending = BTreeMap::new();
        let mut installed = 0usize;
        let mut failure = None;
        let mut denied = None;
        'assembly: while installed < total {
            while let Ok((unit, done, units)) = reported.try_recv() {
                if let Err(detail) = deliver(Delivery::Progress {
                    unit,
                    done,
                    total: units,
                }) {
                    denied = Some(detail);
                    break 'assembly;
                }
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            match received.recv_timeout(POLL) {
                Ok(Message::Unit { index, value }) => {
                    pending.insert(index, value);
                    while let Some(value) = pending.remove(&installed) {
                        if let Err(detail) = deliver(Delivery::Unit {
                            index: installed,
                            value,
                        }) {
                            denied = Some(detail);
                            break 'assembly;
                        }
                        installed += 1;
                        held.put();
                    }
                }
                Ok(Message::Failed(detail)) => {
                    failure = Some(detail);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        halted.store(true, Ordering::Relaxed);
        held.wake();
        while received.recv_timeout(POLL).is_ok() {}
        (installed, failure, denied)
    });
    for state in states {
        finish(state);
    }
    if let Some(detail) = denied {
        return Err(detail);
    }
    if installed == total {
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
