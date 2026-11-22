use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

use crate::cancel::Cancel;

const READ_THREADS: usize = 4;
const WRITE_THREADS: usize = 2;
const COMPUTE_CEILING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Background,
    Demand,
    Immediate,
}

impl Priority {
    fn lane(self) -> usize {
        match self {
            Self::Background => 0,
            Self::Demand => 1,
            Self::Immediate => 2,
        }
    }
}

type Work = Box<dyn FnOnce(&Cancel) + Send + 'static>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Read,
    Compute,
    Write,
}

impl Role {
    const ALL: [Role; 3] = [Role::Read, Role::Compute, Role::Write];

    fn index(self) -> usize {
        match self {
            Self::Read => 0,
            Self::Compute => 1,
            Self::Write => 2,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Compute => "compute",
            Self::Write => "write",
        }
    }

    fn threads(self) -> usize {
        match self {
            Self::Read => READ_THREADS,
            Self::Write => WRITE_THREADS,
            Self::Compute => std::thread::available_parallelism()
                .map_or(2, |count| count.get().clamp(2, COMPUTE_CEILING)),
        }
    }
}

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

#[derive(Default)]
struct Queue {
    lanes: [VecDeque<Task>; 3],
}

impl Queue {
    fn push(&mut self, priority: Priority, task: Task) {
        self.lanes[priority.lane()].push_back(task);
    }

    fn pop(&mut self) -> Option<Task> {
        self.lanes
            .iter_mut()
            .rev()
            .find_map(|lane| lane.pop_front())
    }

    fn vacant(&self) -> bool {
        self.lanes.iter().all(VecDeque::is_empty)
    }
}

struct Shard {
    queue: Mutex<Queue>,
}

impl Shard {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|error| error.into_inner())
    }
}

struct Inner {
    shards: Vec<Shard>,
    sleeper: Mutex<()>,
    ready: Condvar,
    idle: AtomicUsize,
    closed: AtomicBool,
    next: AtomicUsize,
}

pub struct Executor {
    inner: Arc<Inner>,
}

impl Executor {
    pub fn new(role: Role, threads: usize) -> Self {
        assert!(threads > 0, "an executor runs at least one worker");
        let inner = Arc::new(Inner {
            shards: (0..threads)
                .map(|_| Shard {
                    queue: Mutex::new(Queue::default()),
                })
                .collect(),
            sleeper: Mutex::new(()),
            ready: Condvar::new(),
            idle: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            next: AtomicUsize::new(0),
        });
        for index in 0..threads {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("grana-{}-{index}", role.name()))
                .spawn(move || serve(&inner, index))
                .expect("an executor worker starts");
        }
        Self { inner }
    }

    pub fn lane(role: Role) -> &'static Executor {
        static LANES: OnceLock<[Executor; 3]> = OnceLock::new();
        let lanes = LANES.get_or_init(|| {
            std::array::from_fn(|index| {
                let role = Role::ALL[index];
                Executor::new(role, role.threads())
            })
        });
        &lanes[role.index()]
    }

    pub fn submit(&self, work: impl FnOnce(&Cancel) + Send + 'static) -> Job {
        self.submit_with(Priority::Immediate, work)
    }

    pub fn submit_with(
        &self,
        priority: Priority,
        work: impl FnOnce(&Cancel) + Send + 'static,
    ) -> Job {
        let cancel = Cancel::new();
        let shard = self.shard();
        let mut queue = self.inner.shards[shard].lock();
        assert!(
            !self.inner.closed.load(Ordering::Relaxed),
            "an executor accepts work until its caller releases it"
        );
        queue.push(
            priority,
            Task {
                cancel: cancel.clone(),
                work: Box::new(work),
            },
        );
        drop(queue);
        self.wake();
        Job { cancel }
    }

    pub fn submit_batch<T, F>(&self, priority: Priority, items: Vec<T>, size: usize, work: F) -> Job
    where
        T: Send + 'static,
        F: Fn(&Cancel, T) + Send + Sync + 'static,
    {
        assert!(size > 0, "a batch task runs at least one item");
        let cancel = Cancel::new();
        if items.is_empty() {
            return Job { cancel };
        }
        assert!(
            !self.inner.closed.load(Ordering::Relaxed),
            "an executor accepts work until its caller releases it"
        );
        let work = Arc::new(work);
        let mut rest = items;
        while !rest.is_empty() {
            let held = rest.len().min(size);
            let tail = rest.split_off(held);
            let part = rest;
            rest = tail;
            let work = Arc::clone(&work);
            let token = cancel.clone();
            let shard = self.shard();
            let mut queue = self.inner.shards[shard].lock();
            queue.push(
                priority,
                Task {
                    cancel: token.clone(),
                    work: Box::new(move |cancel| {
                        for item in part {
                            work(cancel, item);
                        }
                    }),
                },
            );
            drop(queue);
            self.wake();
        }
        Job { cancel }
    }

    fn shard(&self) -> usize {
        self.inner.next.fetch_add(1, Ordering::Relaxed) % self.inner.shards.len()
    }

    fn wake(&self) {
        if self.inner.idle.load(Ordering::Relaxed) == 0 {
            return;
        }
        let _held = self
            .inner
            .sleeper
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.inner.ready.notify_all();
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Relaxed);
        let _held = self
            .inner
            .sleeper
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.inner.ready.notify_all();
    }
}

fn serve(inner: &Inner, index: usize) {
    while let Some(Task { cancel, work }) = take(inner, index) {
        if let Err(fault) = crate::fault::contain(move || work(&cancel)) {
            let detail = fault.describe("a background task");
            grana_log::error!(target: "grana::exec", "{detail}");
            eprintln!("{detail}");
        }
    }
}

fn take(inner: &Inner, index: usize) -> Option<Task> {
    loop {
        if let Some(task) = take_ready(inner, index) {
            return Some(task);
        }
        inner.idle.fetch_add(1, Ordering::SeqCst);
        let held = inner
            .sleeper
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(task) = take_ready(inner, index) {
            inner.idle.fetch_sub(1, Ordering::SeqCst);
            return Some(task);
        }
        if inner.closed.load(Ordering::Relaxed) {
            inner.idle.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        drop(
            inner
                .ready
                .wait(held)
                .unwrap_or_else(|error| error.into_inner()),
        );
        inner.idle.fetch_sub(1, Ordering::SeqCst);
    }
}

fn take_ready(inner: &Inner, index: usize) -> Option<Task> {
    if let Some(task) = inner.shards[index].lock().pop() {
        return Some(task);
    }
    let count = inner.shards.len();
    for offset in 1..count {
        let shard = (index + offset) % count;
        let mut queue = inner.shards[shard].lock();
        if queue.vacant() {
            continue;
        }
        if let Some(task) = queue.pop() {
            return Some(task);
        }
    }
    None
}
