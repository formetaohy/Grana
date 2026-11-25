use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::budget::{Meter, Pool};

const NONE: u32 = u32::MAX;

struct Node<K, V> {
    key: K,
    value: Arc<V>,
    bytes: u64,
    newer: u32,
    older: u32,
}

struct Rows<K, V> {
    slots: HashMap<K, u32>,
    nodes: Vec<Node<K, V>>,
    free: Vec<u32>,
    pending: Vec<K>,
    newest: u32,
    oldest: u32,
    bytes: u64,
}

impl<K: Clone + Eq + std::hash::Hash, V> Rows<K, V> {
    fn new() -> Self {
        Self {
            slots: HashMap::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            pending: Vec::new(),
            newest: NONE,
            oldest: NONE,
            bytes: 0,
        }
    }

    fn held(&mut self, key: &K) -> Option<Arc<V>> {
        let slot = *self.slots.get(key)?;
        self.promote(slot);
        Some(Arc::clone(&self.nodes[slot as usize].value))
    }

    fn waiting(&self, key: &K) -> bool {
        self.pending.iter().any(|held| held == key)
    }

    fn claim(&mut self, key: K) {
        self.pending.push(key);
    }

    fn release(&mut self, key: &K) {
        self.pending.retain(|held| held != key);
    }

    fn promote(&mut self, slot: u32) {
        if slot == self.newest {
            return;
        }
        let node = &self.nodes[slot as usize];
        let (newer, older) = (node.newer, node.older);
        if older != NONE {
            self.nodes[older as usize].newer = newer;
        } else {
            self.oldest = newer;
        }
        if newer != NONE {
            self.nodes[newer as usize].older = older;
        }
        let previous = self.newest;
        self.nodes[slot as usize].newer = NONE;
        self.nodes[slot as usize].older = previous;
        if previous != NONE {
            self.nodes[previous as usize].newer = slot;
        }
        self.newest = slot;
    }

    fn insert(&mut self, key: K, value: Arc<V>, bytes: u64, budget: u64) -> u64 {
        let slot = match self.free.pop() {
            Some(slot) => {
                self.nodes[slot as usize] = Node {
                    key,
                    value,
                    bytes,
                    newer: NONE,
                    older: self.newest,
                };
                slot
            }
            None => {
                let slot =
                    u32::try_from(self.nodes.len()).expect("a cache holds fewer than 4G values");
                self.nodes.push(Node {
                    key,
                    value,
                    bytes,
                    newer: NONE,
                    older: self.newest,
                });
                slot
            }
        };
        if self.newest == NONE {
            self.oldest = slot;
        } else {
            self.nodes[self.newest as usize].newer = slot;
        }
        self.newest = slot;
        self.slots
            .insert(self.nodes[slot as usize].key.clone(), slot);
        self.bytes += bytes;
        self.evict(budget)
    }

    fn evict(&mut self, budget: u64) -> u64 {
        let mut evicted = 0;
        while self.bytes > budget && self.slots.len() > 1 {
            let slot = self.oldest;
            if slot == NONE {
                break;
            }
            let node = &self.nodes[slot as usize];
            let key = node.key.clone();
            let bytes = node.bytes;
            self.oldest = node.newer;
            if self.oldest == NONE {
                self.newest = NONE;
            } else {
                self.nodes[self.oldest as usize].older = NONE;
            }
            self.slots.remove(&key);
            self.nodes[slot as usize].older = NONE;
            self.nodes[slot as usize].newer = NONE;
            self.free.push(slot);
            self.bytes = self.bytes.saturating_sub(bytes);
            evicted += bytes;
        }
        evicted
    }
}

pub struct LruCache<K, V> {
    rows: Mutex<Box<Rows<K, V>>>,
    ready: Condvar,
    size: fn(&V) -> u64,
    pool: Pool,
    scopes: Option<&'static AtomicUsize>,
}

impl<K: Clone + Eq + std::hash::Hash, V> LruCache<K, V> {
    pub fn pooled(pool: Pool, size: fn(&V) -> u64) -> Self {
        Self {
            rows: Mutex::new(Box::new(Rows::new())),
            ready: Condvar::new(),
            size,
            pool,
            scopes: None,
        }
    }

    pub fn scoped(pool: Pool, scopes: &'static AtomicUsize, size: fn(&V) -> u64) -> Self {
        Self {
            scopes: Some(scopes),
            ..Self::pooled(pool, size)
        }
    }

    pub fn pool(&self) -> Pool {
        self.pool
    }

    pub fn meter(&self) -> &'static Meter {
        self.pool.meter()
    }

    pub fn share(&self) -> u64 {
        let share = self.pool.share();
        match self.scopes {
            Some(scopes) => {
                let live = scopes.load(Ordering::Relaxed).max(1) as u64;
                (share / live).max(1)
            }
            None => share,
        }
    }

    pub fn fetch(&self, key: &K) -> Option<Arc<V>> {
        let held = self.lock().held(key);
        match &held {
            Some(_) => self.meter().hit(),
            None => self.meter().miss(),
        }
        held
    }

    pub fn load<E>(&self, key: K, load: impl FnOnce() -> Result<V, E>) -> Result<Arc<V>, E> {
        {
            let mut rows = self.lock();
            loop {
                if let Some(held) = rows.held(&key) {
                    drop(rows);
                    self.meter().hit();
                    return Ok(held);
                }
                if rows.waiting(&key) {
                    rows = self.wait(rows);
                    continue;
                }
                rows.claim(key.clone());
                break;
            }
        }
        self.meter().miss();
        let value = match load() {
            Ok(value) => Arc::new(value),
            Err(detail) => {
                self.abandon(&key);
                return Err(detail);
            }
        };
        let budget = self.share();
        let bytes = (self.size)(&value);
        let evicted = {
            let mut rows = self.lock();
            rows.release(&key);
            rows.insert(key, Arc::clone(&value), bytes, budget)
        };
        self.meter().evict(evicted);
        self.wake();
        Ok(value)
    }

    fn abandon(&self, key: &K) {
        self.lock().release(key);
        self.wake();
    }

    fn lock(&self) -> MutexGuard<'_, Box<Rows<K, V>>> {
        self.rows.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn wait<'a>(&self, rows: MutexGuard<'a, Box<Rows<K, V>>>) -> MutexGuard<'a, Box<Rows<K, V>>> {
        self.ready
            .wait(rows)
            .unwrap_or_else(|error| error.into_inner())
    }

    fn wake(&self) {
        self.ready.notify_all();
    }
}
