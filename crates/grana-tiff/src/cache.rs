use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Key = (usize, usize, usize);

struct Entry {
    data: Arc<Vec<u8>>,
    stamp: u64,
}

pub(crate) struct Cache {
    blocks: HashMap<Key, Entry>,
    bytes: u64,
    stamp: u64,
    limit: u64,
}

impl Cache {
    pub(crate) fn new(limit: u64) -> Mutex<Self> {
        Mutex::new(Self {
            blocks: HashMap::new(),
            bytes: 0,
            stamp: 0,
            limit,
        })
    }

    pub(crate) fn lookup(cache: &Mutex<Self>, key: Key) -> Option<Arc<Vec<u8>>> {
        let mut cache = cache.lock().expect("the tile cache is not poisoned");
        cache.stamp += 1;
        let stamp = cache.stamp;
        let entry = cache.blocks.get_mut(&key)?;
        entry.stamp = stamp;
        Some(Arc::clone(&entry.data))
    }

    pub(crate) fn insert(cache: &Mutex<Self>, key: Key, data: Arc<Vec<u8>>) {
        let mut cache = cache.lock().expect("the tile cache is not poisoned");
        let size = data.len() as u64;
        if size > cache.limit {
            return;
        }
        cache.stamp += 1;
        let stamp = cache.stamp;
        if let Some(previous) = cache.blocks.insert(key, Entry { data, stamp }) {
            cache.bytes -= previous.data.len() as u64;
        }
        cache.bytes += size;
        while cache.bytes > cache.limit {
            let oldest = cache
                .blocks
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| *key);
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(removed) = cache.blocks.remove(&oldest) {
                cache.bytes -= removed.data.len() as u64;
            }
        }
    }
}
