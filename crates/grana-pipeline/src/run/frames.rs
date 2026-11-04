use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Condvar, Mutex};

enum Message<T> {
    Progress {
        frame: u32,
        done: usize,
        total: usize,
    },
    Frame {
        index: u32,
        value: Option<T>,
    },
    Failed(String),
}

struct Gate {
    next: u32,
    installed: u32,
    stop: bool,
}

impl Gate {
    fn ready<T>(&mut self, pending: &mut BTreeMap<u32, Option<T>>) -> Vec<(u32, Option<T>)> {
        let mut ready = Vec::new();
        while let Some(value) = pending.remove(&self.installed) {
            ready.push((self.installed, value));
            self.installed += 1;
        }
        ready
    }
}

pub(crate) enum Notice<T> {
    Progress {
        frame: u32,
        done: usize,
        total: usize,
    },
    Frame {
        frame: u32,
        value: T,
    },
}

pub(crate) fn frames<L, T>(
    total: u32,
    window: usize,
    cancel: &AtomicBool,
    local: impl Fn() -> L + Sync,
    work: impl Fn(&mut L, u32, &mut dyn FnMut(usize, usize)) -> Result<Option<T>, String> + Sync,
    emit: &mut dyn FnMut(Notice<T>),
) -> Result<bool, String>
where
    L: Send,
    T: Send,
{
    if total == 0 {
        return Ok(true);
    }
    let window = window.clamp(1, total as usize);
    let gate = Mutex::new(Gate {
        next: 0,
        installed: 0,
        stop: false,
    });
    let signal = Condvar::new();
    let (sender, receiver) = mpsc::sync_channel::<Message<T>>(window * 2 + 2);
    std::thread::scope(|scope| -> Result<bool, String> {
        for _ in 0..window {
            let sender: SyncSender<Message<T>> = sender.clone();
            let gate = &gate;
            let signal = &signal;
            let local = &local;
            let work = &work;
            scope.spawn(move || {
                let mut state = local();
                loop {
                    let index = {
                        let mut lock = gate.lock().expect("the frame window is not poisoned");
                        while lock.next >= lock.installed + window as u32 {
                            if lock.stop || cancel.load(Ordering::Relaxed) {
                                return;
                            }
                            lock = signal.wait(lock).expect("the frame window is not poisoned");
                        }
                        if lock.stop || cancel.load(Ordering::Relaxed) || lock.next >= total {
                            return;
                        }
                        let index = lock.next;
                        lock.next += 1;
                        index
                    };
                    let outcome = {
                        let mut report = |done: usize, blocks: usize| {
                            let _ = sender.send(Message::Progress {
                                frame: index,
                                done,
                                total: blocks,
                            });
                        };
                        work(&mut state, index, &mut report)
                    };
                    let failed = outcome.is_err();
                    let message = match outcome {
                        Ok(value) => Message::Frame { index, value },
                        Err(detail) => Message::Failed(detail),
                    };
                    if sender.send(message).is_err() || failed {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut pending: BTreeMap<u32, Option<T>> = BTreeMap::new();
        let mut completed = 0u32;
        let mut failure = None;
        while completed < total {
            let Ok(message) = receiver.recv() else {
                break;
            };
            match message {
                Message::Progress { frame, done, total } => {
                    emit(Notice::Progress { frame, done, total })
                }
                Message::Frame { index, value } => {
                    completed += 1;
                    pending.insert(index, value);
                    let ready = {
                        let mut lock = gate.lock().expect("the frame window is not poisoned");
                        let ready = lock.ready(&mut pending);
                        if !ready.is_empty() {
                            signal.notify_all();
                        }
                        ready
                    };
                    for (frame, value) in ready {
                        if let Some(value) = value {
                            emit(Notice::Frame { frame, value });
                        }
                    }
                }
                Message::Failed(detail) => {
                    let mut lock = gate.lock().expect("the frame window is not poisoned");
                    lock.stop = true;
                    drop(lock);
                    signal.notify_all();
                    failure = Some(detail);
                    break;
                }
            }
        }
        while receiver.recv().is_ok() {}
        match failure {
            Some(detail) => Err(detail),
            None if cancel.load(Ordering::Relaxed) => Ok(false),
            None if completed < total => {
                Err("a worker stopped before its frame was completed".to_string())
            }
            None => Ok(true),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn installs_every_frame_in_order_within_the_window() {
        let cancel = AtomicBool::new(false);
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let mut installed = Vec::new();
        let completed = frames::<(), u32>(
            24,
            4,
            &cancel,
            || (),
            |(), index, _| {
                let current = live.fetch_add(1, Ordering::Relaxed) + 1;
                peak.fetch_max(current, Ordering::Relaxed);
                std::thread::yield_now();
                live.fetch_sub(1, Ordering::Relaxed);
                Ok(Some(index))
            },
            &mut |notice| {
                if let Notice::Frame { frame, value } = notice {
                    installed.push((frame, value));
                }
            },
        )
        .expect("the frames complete");
        assert!(completed);
        assert_eq!(installed.len(), 24);
        assert!(
            installed
                .iter()
                .enumerate()
                .all(|(position, (frame, value))| *frame as usize == position
                    && *value as usize == position)
        );
        assert!(peak.load(Ordering::Relaxed) <= 4);
    }
}
