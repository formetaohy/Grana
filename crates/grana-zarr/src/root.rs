use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grana_net::{Client, Fetched, Span, Target};
use zarrs::filesystem::FilesystemStore;
use zarrs::storage::byte_range::{ByteRange, ByteRangeIterator};
use zarrs::storage::{
    Bytes, ListableStorageTraits, MaybeBytes, MaybeBytesIterator, ReadableListableStorageTraits,
    ReadableStorageTraits, StorageError, StoreKey, StoreKeys, StoreKeysPrefixes, StorePrefix,
};

use crate::Error;

const BLOCK_BUDGET: u64 = 96 << 20;
const BLOCK_LIMIT: u64 = 64 << 20;

pub struct Root {
    store: Store,
    origin: Origin,
}

enum Store {
    Local(Arc<FilesystemStore>),
    Remote(Arc<RemoteStore>),
}

enum Origin {
    Local(PathBuf),
    Remote {
        target: Target,
        revision: Arc<Mutex<Option<String>>>,
    },
}

impl Root {
    pub fn local(path: &Path) -> Result<Self, Error> {
        let store = FilesystemStore::new(path).map_err(|error| Error::Open {
            path: path.display().to_string(),
            detail: error.to_string(),
        })?;
        Ok(Self {
            store: Store::Local(Arc::new(store)),
            origin: Origin::Local(path.to_path_buf()),
        })
    }

    pub fn remote(url: &str) -> Result<Self, Error> {
        let target = Target::parse(url).map_err(|error| Error::Unsupported {
            path: url.to_string(),
            detail: error.to_string(),
        })?;
        let revision = Arc::new(Mutex::new(None));
        let store = RemoteStore {
            target: target.clone(),
            client: Client::shared(),
            revision: Arc::clone(&revision),
            blocks: Mutex::new(Blocks::default()),
            sizes: Mutex::new(HashMap::new()),
        };
        Ok(Self {
            store: Store::Remote(Arc::new(store)),
            origin: Origin::Remote { target, revision },
        })
    }

    pub fn display(&self) -> String {
        match &self.origin {
            Origin::Local(path) => path.display().to_string(),
            Origin::Remote { target, .. } => target.display().to_string(),
        }
    }

    pub fn revision(&self) -> Option<String> {
        match &self.origin {
            Origin::Local(_) => None,
            Origin::Remote { revision, .. } => revision
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        }
    }

    pub fn name(&self) -> String {
        self.name_at("/")
    }

    pub fn name_at(&self, group: &str) -> String {
        let name = match &self.origin {
            Origin::Local(path) => path
                .join(group.trim_start_matches('/'))
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string),
            Origin::Remote { .. } => self.tag(group).rsplit('/').next().map(str::to_string),
        };
        let name = name.unwrap_or_else(|| "OME-Zarr".to_string());
        let name = name.strip_suffix(".zarr").unwrap_or(&name);
        let name = name.strip_suffix(".ome").unwrap_or(name);
        if name.is_empty() {
            "OME-Zarr".to_string()
        } else {
            name.to_string()
        }
    }

    pub fn tag(&self, group: &str) -> String {
        match &self.origin {
            Origin::Local(path) => path
                .join(group.trim_start_matches('/'))
                .display()
                .to_string(),
            Origin::Remote { target, .. } => {
                let group = group.trim_matches('/');
                if group.is_empty() {
                    target.display().to_string()
                } else {
                    target.join(group)
                }
            }
        }
    }

    pub(crate) fn readable(&self) -> Arc<dyn ReadableStorageTraits> {
        match &self.store {
            Store::Local(store) => store.clone(),
            Store::Remote(store) => store.clone(),
        }
    }

    pub(crate) fn listable(&self) -> Arc<dyn ReadableListableStorageTraits> {
        match &self.store {
            Store::Local(store) => store.clone(),
            Store::Remote(store) => store.clone(),
        }
    }
}

#[derive(Default)]
struct Blocks {
    entries: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    bytes: u64,
}

impl Blocks {
    fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, block: Arc<Vec<u8>>) {
        self.bytes += block.len() as u64;
        self.order.push_back(key.clone());
        self.entries.insert(key, block);
        while self.bytes > BLOCK_BUDGET {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(block) = self.entries.remove(&oldest) {
                self.bytes -= block.len() as u64;
            }
        }
    }
}

enum Block {
    Ready(Arc<Vec<u8>>),
    Ranged,
}

struct RemoteStore {
    target: Target,
    client: Arc<Client>,
    revision: Arc<Mutex<Option<String>>>,
    blocks: Mutex<Blocks>,
    sizes: Mutex<HashMap<String, Option<u64>>>,
}

impl RemoteStore {
    fn url(&self, key: &StoreKey) -> String {
        self.target.join(key.as_str())
    }

    fn fetch(&self, key: &StoreKey, span: Option<Span>) -> Result<Option<Fetched>, StorageError> {
        let url = self.url(key);
        let fetched = self
            .client
            .get(&url, span)
            .map_err(|error| StorageError::Other(error.to_string()))?;
        if let Some(fetched) = &fetched
            && let Some(stamp) = fetched
                .etag
                .clone()
                .or_else(|| fetched.last_modified.clone())
        {
            let mut revision = self
                .revision
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if revision.is_none() {
                *revision = Some(stamp);
            }
        }
        Ok(fetched)
    }

    fn size(&self, key: &StoreKey) -> Result<Option<u64>, StorageError> {
        let name = key.as_str().to_string();
        if let Some(size) = self
            .sizes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&name)
        {
            return Ok(*size);
        }
        let size = self
            .client
            .size(&self.url(key))
            .map_err(|error| StorageError::Other(error.to_string()))?;
        self.sizes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name, size);
        Ok(size)
    }

    fn block(&self, key: &StoreKey) -> Result<Option<Block>, StorageError> {
        let name = key.as_str().to_string();
        if let Some(block) = self
            .blocks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&name)
        {
            return Ok(Some(Block::Ready(block)));
        }
        let Some(size) = self.size(key)? else {
            return Ok(None);
        };
        if size > BLOCK_LIMIT {
            return Ok(Some(Block::Ranged));
        }
        let Some(fetched) = self.fetch(key, None)? else {
            return Ok(None);
        };
        let block = Arc::new(fetched.bytes);
        self.blocks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name, Arc::clone(&block));
        Ok(Some(Block::Ready(block)))
    }

    fn ranged(&self, key: &StoreKey, range: ByteRange) -> Result<MaybeBytes, StorageError> {
        let size = self.size(key)?;
        let span = span(range, size)?;
        Ok(self
            .fetch(key, Some(span))?
            .map(|fetched| Bytes::from(fetched.bytes)))
    }
}

impl ReadableStorageTraits for RemoteStore {
    fn get(&self, key: &StoreKey) -> Result<MaybeBytes, StorageError> {
        Ok(self
            .fetch(key, None)?
            .map(|fetched| Bytes::from(fetched.bytes)))
    }

    fn get_partial_many<'a>(
        &'a self,
        key: &StoreKey,
        byte_ranges: ByteRangeIterator<'a>,
    ) -> Result<MaybeBytesIterator<'a>, StorageError> {
        let ranges = byte_ranges.collect::<Vec<_>>();
        let Some(block) = self.block(key)? else {
            return Ok(None);
        };
        let mut values = Vec::with_capacity(ranges.len());
        match block {
            Block::Ready(block) => {
                for range in ranges {
                    values.push(Ok(slice(&block, range)?));
                }
            }
            Block::Ranged => {
                for range in ranges {
                    match self.ranged(key, range)? {
                        Some(bytes) => values.push(Ok(bytes)),
                        None => return Ok(None),
                    }
                }
            }
        }
        Ok(Some(Box::new(values.into_iter())))
    }

    fn size_key(&self, key: &StoreKey) -> Result<Option<u64>, StorageError> {
        self.size(key)
    }

    fn supports_get_partial(&self) -> bool {
        true
    }
}

impl ListableStorageTraits for RemoteStore {
    fn list(&self) -> Result<StoreKeys, StorageError> {
        Err(unsupported())
    }

    fn list_prefix(&self, _prefix: &StorePrefix) -> Result<StoreKeys, StorageError> {
        Err(unsupported())
    }

    fn list_dir(&self, _prefix: &StorePrefix) -> Result<StoreKeysPrefixes, StorageError> {
        Err(unsupported())
    }

    fn size_prefix(&self, _prefix: &StorePrefix) -> Result<u64, StorageError> {
        Err(unsupported())
    }
}

fn span(range: ByteRange, size: Option<u64>) -> Result<Span, StorageError> {
    let required =
        || StorageError::Other("the byte range reads a key that is not present".to_string());
    match range {
        ByteRange::FromStart(offset, None) => Ok(Span::From(offset)),
        ByteRange::FromStart(_, Some(0)) => Err(empty()),
        ByteRange::FromStart(offset, Some(length)) => {
            Ok(Span::Between(offset, offset + length - 1))
        }
        ByteRange::Suffix(0) => Err(empty()),
        ByteRange::Suffix(length) => {
            let size = size.ok_or_else(required)?;
            Ok(Span::Between(size.saturating_sub(length), size - 1))
        }
    }
}

fn slice(block: &[u8], range: ByteRange) -> Result<Bytes, StorageError> {
    let size = block.len() as u64;
    let span = span(range, Some(size))?;
    let (start, end) = match span {
        Span::From(start) => (start, size.saturating_sub(1)),
        Span::Between(start, end) => (start, end),
    };
    if start > end || end >= size {
        return Err(StorageError::Other(format!(
            "the byte range {start}-{end} lies outside the {size} stored bytes"
        )));
    }
    Ok(Bytes::copy_from_slice(
        &block[start as usize..=end as usize],
    ))
}

fn empty() -> StorageError {
    StorageError::Other("the byte range is empty".to_string())
}

fn unsupported() -> StorageError {
    StorageError::Unsupported("a remote store cannot list its keys".to_string())
}
