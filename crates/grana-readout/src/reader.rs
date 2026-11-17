use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread;

use openreadout_core::Dataset;

use crate::error::Error;
use crate::open;

const LIMIT_FLOOR: usize = 2;
const LIMIT_CEILING: usize = 8;

pub(crate) fn limit() -> usize {
    thread::available_parallelism()
        .map_or(LIMIT_FLOOR, |available| available.get())
        .clamp(LIMIT_FLOOR, LIMIT_CEILING)
}

pub(crate) struct Readers {
    path: PathBuf,
    limit: usize,
    pool: Mutex<Pool>,
    ready: Condvar,
}

struct Pool {
    idle: Vec<Box<dyn Dataset>>,
    opened: usize,
}

impl Readers {
    pub(crate) fn new(path: PathBuf, dataset: Box<dyn Dataset>, limit: usize) -> Self {
        assert!(limit > 0, "a reader pool opens at least one dataset");
        Self {
            path,
            limit,
            pool: Mutex::new(Pool {
                idle: vec![dataset],
                opened: 1,
            }),
            ready: Condvar::new(),
        }
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    pub(crate) fn with<T>(
        &self,
        read: impl FnOnce(&mut dyn Dataset) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut lease = self.acquire()?;
        read(lease.handle())
    }

    fn acquire(&self) -> Result<Lease<'_>, Error> {
        loop {
            let mut pool = self.lock();
            if let Some(handle) = pool.idle.pop() {
                drop(pool);
                return Ok(Lease::new(self, handle));
            }
            if pool.opened < self.limit {
                pool.opened += 1;
                drop(pool);
                return self.reopen();
            }
            drop(self.wait(pool));
        }
    }

    fn reopen(&self) -> Result<Lease<'_>, Error> {
        match open::open(&self.path) {
            Ok(document) => Ok(Lease::new(self, document.dataset)),
            Err(error) => {
                let mut pool = self.lock();
                pool.opened -= 1;
                drop(pool);
                self.ready.notify_one();
                Err(error)
            }
        }
    }

    fn release(&self, handle: Box<dyn Dataset>) {
        let mut pool = self.lock();
        pool.idle.push(handle);
        drop(pool);
        self.ready.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, Pool> {
        self.pool.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn wait<'a>(&self, pool: MutexGuard<'a, Pool>) -> MutexGuard<'a, Pool> {
        self.ready
            .wait(pool)
            .unwrap_or_else(|error| error.into_inner())
    }
}

struct Lease<'a> {
    readers: &'a Readers,
    handle: Option<Box<dyn Dataset>>,
}

impl<'a> Lease<'a> {
    fn new(readers: &'a Readers, handle: Box<dyn Dataset>) -> Self {
        Self {
            readers,
            handle: Some(handle),
        }
    }

    fn handle(&mut self) -> &mut dyn Dataset {
        self.handle
            .as_deref_mut()
            .expect("a lease holds its dataset until it is released")
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.readers.release(
            self.handle
                .take()
                .expect("a lease holds its dataset until it is dropped"),
        );
    }
}
