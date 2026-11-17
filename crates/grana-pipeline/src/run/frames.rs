use std::sync::atomic::AtomicBool;

use grana_analysis::{Delivery, Run, ordered};

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
    workers: usize,
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
    ordered(
        Run {
            total: total as usize,
            workers,
            cancel,
        },
        window,
        local,
        |state, index, notify| {
            let index = u32::try_from(index).expect("a frame index fits the u32 range");
            work(state, index, notify)
        },
        |delivery| match delivery {
            Delivery::Progress { unit, done, total } => {
                emit(Notice::Progress {
                    frame: unit as u32,
                    done,
                    total,
                });
                Ok(())
            }
            Delivery::Unit { index, value } => {
                if let Some(value) = value {
                    emit(Notice::Frame {
                        frame: index as u32,
                        value,
                    });
                }
                Ok(())
            }
        },
        |_| {},
    )
}
