use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;

struct Progress {
    frame: u32,
    done: usize,
    total: usize,
}

enum Message<T> {
    Frame { index: u32, value: Option<T> },
    Failed(String),
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
    let next = AtomicU32::new(0);
    let (reports, progress) = mpsc::channel::<Progress>();
    let (sender, receiver) = mpsc::sync_channel::<Message<T>>(window * 2 + 2);
    grana_analysis::parallel(|scope| -> Result<bool, String> {
        for _ in 0..window {
            let sender = sender.clone();
            let reports = reports.clone();
            let next = &next;
            let local = &local;
            let work = &work;
            scope.spawn(move |_| {
                let mut state: Option<L> = None;
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        return;
                    }
                    let state = state.get_or_insert_with(local);
                    let outcome = {
                        let mut notify = |done: usize, blocks: usize| {
                            let _ = reports.send(Progress {
                                frame: index,
                                done,
                                total: blocks,
                            });
                        };
                        work(state, index, &mut notify)
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
        drop(reports);
        let mut pending: BTreeMap<u32, Option<T>> = BTreeMap::new();
        let mut installed = 0u32;
        let mut failure = None;
        while installed < total {
            drain(&progress, emit);
            let Some(message) = grana_analysis::receive(&receiver) else {
                break;
            };
            match message {
                Message::Frame { index, value } => {
                    pending.insert(index, value);
                    while let Some(value) = pending.remove(&installed) {
                        if let Some(value) = value {
                            emit(Notice::Frame {
                                frame: installed,
                                value,
                            });
                        }
                        installed += 1;
                    }
                }
                Message::Failed(detail) => {
                    failure = Some(detail);
                    break;
                }
            }
        }
        while grana_analysis::receive(&receiver).is_some() {}
        drain(&progress, emit);
        match failure {
            Some(detail) => Err(detail),
            None if cancel.load(Ordering::Relaxed) => Ok(false),
            None if installed < total => {
                Err("a worker stopped before its frame was completed".to_string())
            }
            None => Ok(true),
        }
    })
}

fn drain<T>(progress: &mpsc::Receiver<Progress>, emit: &mut dyn FnMut(Notice<T>)) {
    while let Ok(report) = progress.try_recv() {
        emit(Notice::Progress {
            frame: report.frame,
            done: report.done,
            total: report.total,
        });
    }
}
