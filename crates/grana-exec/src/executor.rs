use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::cancel::Cancel;

const WORKER_CEILING: usize = 8;

type Work = Box<dyn FnOnce(&Cancel) + Send + 'static>;

pub struct Job {
    cancel: Cancel,
}

impl Job {
    pub fn cancel(&self) {
        self.cancel.trigger();
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.cancelled()
    }

    pub fn token(&self) -> Cancel {
        self.cancel.clone()
    }
}

struct Task {
    cancel: Cancel,
    work: Work,
}

struct State {
    tasks: VecDeque<Task>,
    closed: bool,
}

struct Inner {
    state: Mutex<State>,
    ready: Condvar,
}

pub struct Executor {
    inner: Arc<Inner>,
}

impl Executor {
    pub fn new(threads: usize) -> Self {
        assert!(threads > 0, "an executor runs at least one worker");
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                tasks: VecDeque::new(),
                closed: false,
            }),
            ready: Condvar::new(),
        });
        for index in 0..threads {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("grana-exec-{index}"))
                .spawn(move || serve(&inner))
                .expect("an executor worker starts");
        }
        Self { inner }
    }

    pub fn shared() -> &'static Executor {
        static SHARED: OnceLock<Executor> = OnceLock::new();
        SHARED.get_or_init(|| Executor::new(workers()))
    }

    pub fn submit(&self, work: impl FnOnce(&Cancel) + Send + 'static) -> Job {
        let cancel = Cancel::new();
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            !state.closed,
            "an executor accepts work until its caller releases it"
        );
        state.tasks.push_back(Task {
            cancel: cancel.clone(),
            work: Box::new(work),
        });
        drop(state);
        self.inner.ready.notify_one();
        Job { cancel }
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        drop(state);
        self.inner.ready.notify_all();
    }
}

fn serve(inner: &Inner) {
    loop {
        let task = take(inner);
        let Some(task) = task else {
            return;
        };
        (task.work)(&task.cancel);
    }
}

fn take(inner: &Inner) -> Option<Task> {
    let mut state = inner
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    loop {
        if let Some(task) = state.tasks.pop_front() {
            return Some(task);
        }
        if state.closed {
            return None;
        }
        state = inner
            .ready
            .wait(state)
            .unwrap_or_else(|error| error.into_inner());
    }
}

fn workers() -> usize {
    std::thread::available_parallelism().map_or(2, |count| count.get().clamp(2, WORKER_CEILING))
}
