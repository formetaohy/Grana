use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Key = (usize, usize, usize);

const SHARDS: usize = 16;

struct Entry {
    data: Arc<Vec<u8>>,
    stamp: u64,
}

struct Shard {
    blocks: HashMap<Key, Entry>,
    bytes: u64,
    stamp: u64,
    limit: u64,
}

pub(crate) struct Cache {
    shards: Vec<Mutex<Shard>>,
}

impl Cache {
    pub(crate) fn new(limit: u64) -> Self {
        let limit = (limit / SHARDS as u64).max(1);
        Self {
            shards: (0..SHARDS)
                .map(|_| {
                    Mutex::new(Shard {
                        blocks: HashMap::new(),
                        bytes: 0,
                        stamp: 0,
                        limit,
                    })
                })
                .collect(),
        }
    }

    fn shard(&self, key: Key) -> &Mutex<Shard> {
        let slot = key.0.wrapping_mul(0x9e37_79b1) ^ key.1.wrapping_mul(0x85eb_ca6b);
        &self.shards[(slot ^ key.2.wrapping_mul(0xc2b2_ae35)) % SHARDS]
    }

    pub(crate) fn lookup(&self, key: Key) -> Option<Arc<Vec<u8>>> {
        let mut shard = self
            .shard(key)
            .lock()
            .expect("the tile cache is not poisoned");
        shard.stamp += 1;
        let stamp = shard.stamp;
        let entry = shard.blocks.get_mut(&key)?;
        entry.stamp = stamp;
        Some(Arc::clone(&entry.data))
    }

    pub(crate) fn insert(&self, key: Key, data: Arc<Vec<u8>>) {
        let mut shard = self
            .shard(key)
            .lock()
            .expect("the tile cache is not poisoned");
        let size = data.len() as u64;
        if size > shard.limit {
            return;
        }
        shard.stamp += 1;
        let stamp = shard.stamp;
        if let Some(previous) = shard.blocks.insert(key, Entry { data, stamp }) {
            shard.bytes -= previous.data.len() as u64;
        }
        shard.bytes += size;
        while shard.bytes > shard.limit {
            let oldest = shard
                .blocks
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| *key);
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(removed) = shard.blocks.remove(&oldest) {
                shard.bytes -= removed.data.len() as u64;
            }
        }
    }
}
