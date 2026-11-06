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
        let mut used = self
            .used
            .lock()
            .expect("the array budget is never poisoned");
        while *used + bytes > self.limit {
            if halted() {
                return false;
            }
            let (guard, _) = self
                .released
                .wait_timeout(used, BUDGET_POLL)
                .expect("the array budget is never poisoned");
            used = guard;
        }
        *used += bytes;
        true
    }

    pub(crate) fn release(&self, bytes: u64) {
        let mut used = self
            .used
            .lock()
            .expect("the array budget is never poisoned");
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
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel::<Result<(usize, T), String>>();
    let (retired, retired_receiver) = mpsc::channel::<Option<L>>();
    let completed = parallel(|scope| {
        for _ in 0..workers.max(1) {
            let sender = sender.clone();
            let retired = retired.clone();
            let next = &next;
            let stop = &stop;
            let local = &local;
            let work = &work;
            scope.spawn(move |_| {
                let mut state = None;
                loop {
                    if halted(cancel, stop) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    let state = state.get_or_insert_with(local);
                    let outcome = work(state, index);
                    let failed = outcome.is_err();
                    if sender.send(outcome.map(|value| (index, value))).is_err() {
                        break;
                    }
                    if failed {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                let _ = retired.send(state);
            });
        }
        drop(sender);
        drop(retired);
        let mut pending: Vec<Option<T>> = (0..total).map(|_| None).collect();
        let mut assembled = 0usize;
        let mut done = 0usize;
        while let Some(message) = receive(&receiver) {
            match message {
                Ok((index, value)) => {
                    done += 1;
                    pending[index] = Some(value);
                    while assembled < total {
                        let Some(value) = pending[assembled].take() else {
                            break;
                        };
                        sink(assembled, value)?;
                        assembled += 1;
                    }
                    progress(done, total);
                }
                Err(detail) => {
                    stop.store(true, Ordering::Relaxed);
                    return Err(detail);
                }
            }
        }
        if done == total {
            return Ok(true);
        }
        if cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        Err("a worker stopped before its unit of work was completed".to_string())
    })?;
    for state in retired_receiver.into_iter().flatten() {
        finish(state);
    }
    Ok(completed)
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

fn halted(cancel: &AtomicBool, stop: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_a_reservation_until_the_budget_is_released() {
        let budget = Budget::new(8);
        assert!(budget.acquire(8, || false));
        assert!(
            !budget.acquire(8, || true),
            "a full budget refuses a halted reservation"
        );
        budget.release(8);
        assert!(budget.acquire(8, || true));
        budget.release(8);
    }

    #[test]
    fn shares_one_budget_between_workers() {
        let budget = Budget::new(16);
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let budget = &budget;
                let live = &live;
                let peak = &peak;
                scope.spawn(move || {
                    assert!(budget.acquire(8, || false));
                    let current = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    std::thread::yield_now();
                    live.fetch_sub(1, Ordering::SeqCst);
                    budget.release(8);
                });
            }
        });
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "the reservations stay within the shared budget"
        );
    }
}
