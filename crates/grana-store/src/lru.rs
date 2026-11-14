use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

const SHARDS: usize = 16;
const NONE: u32 = u32::MAX;

struct Node<K, V> {
    key: K,
    value: Arc<V>,
    bytes: u64,
    newer: u32,
    older: u32,
}

struct Shard<K, V> {
    slots: HashMap<K, u32>,
    nodes: Vec<Node<K, V>>,
    free: Vec<u32>,
    newest: u32,
    oldest: u32,
    bytes: u64,
    budget: u64,
}

impl<K: Copy + Eq + Hash, V> Shard<K, V> {
    fn new(budget: u64) -> Self {
        Self {
            slots: HashMap::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            newest: NONE,
            oldest: NONE,
            bytes: 0,
            budget,
        }
    }

    fn fetch(&mut self, key: K) -> Option<Arc<V>> {
        let slot = *self.slots.get(&key)?;
        self.promote(slot);
        Some(Arc::clone(&self.nodes[slot as usize].value))
    }

    fn insert(&mut self, key: K, value: Arc<V>, bytes: u64) {
        if let Some(slot) = self.slots.get(&key).copied() {
            let node = &mut self.nodes[slot as usize];
            self.bytes = self.bytes - node.bytes + bytes;
            node.value = value;
            node.bytes = bytes;
            self.promote(slot);
            self.evict();
            return;
        }
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
                let slot = u32::try_from(self.nodes.len())
                    .expect("a cache shard holds fewer than 4G rows");
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
        self.slots.insert(key, slot);
        self.bytes += bytes;
        self.evict();
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

    fn evict(&mut self) {
        while self.bytes > self.budget && self.slots.len() > 1 {
            let slot = self.oldest;
            if slot == NONE {
                return;
            }
            let node = &self.nodes[slot as usize];
            let key = node.key;
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
        }
    }
}

pub struct LruCache<K, V> {
    shards: Vec<Mutex<Shard<K, V>>>,
    size: fn(&V) -> u64,
}

impl<K: Copy + Eq + Hash, V> LruCache<K, V> {
    pub fn new(budget: u64, size: fn(&V) -> u64) -> Self {
        let budget = (budget / SHARDS as u64).max(1);
        Self {
            shards: (0..SHARDS)
                .map(|_| Mutex::new(Shard::new(budget)))
                .collect(),
            size,
        }
    }

    pub fn fetch(&self, key: K) -> Option<Arc<V>> {
        self.lock(key).fetch(key)
    }

    pub fn insert(&self, key: K, value: Arc<V>) {
        let bytes = (self.size)(&value);
        self.lock(key).insert(key, value, bytes);
    }

    fn lock(&self, key: K) -> std::sync::MutexGuard<'_, Shard<K, V>> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let index = (hasher.finish() % self.shards.len() as u64) as usize;
        self.shards[index]
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
