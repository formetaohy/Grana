use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

pub(crate) const WORKER_LIMIT: usize = 16;

pub(crate) fn workers(requested: usize) -> usize {
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
    let (sender, receiver) = mpsc::sync_channel::<Result<(usize, T), String>>(workers.max(1) * 2);
    let (retired, retired_receiver) = mpsc::channel::<Option<L>>();
    let completed = std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            let sender = sender.clone();
            let retired = retired.clone();
            let next = &next;
            let stop = &stop;
            let local = &local;
            let work = &work;
            scope.spawn(move || {
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
        while let Ok(message) = receiver.recv() {
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
    std::thread::scope(|scope| {
        let work = &work;
        for (slot, group) in slots.chunks_mut(chunk).zip(items.chunks(chunk)) {
            scope.spawn(move || {
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
