use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use grana_net::{Client, Presence, Span, Target, Transfer};
use grana_store::{LruCache, Pool};
use zarrs::filesystem::FilesystemStore;
use zarrs::storage::byte_range::{ByteRange, ByteRangeIterator};
use zarrs::storage::{
    Bytes, ListableStorageTraits, MaybeBytes, MaybeBytesIterator, ReadableListableStorageTraits,
    ReadableStorageTraits, StorageError, StoreKey, StoreKeys, StoreKeysPrefixes, StorePrefix,
};

use crate::Error;
use crate::witness::{self, Local};

const BLOCK_LIMIT: u64 = 64 << 20;

#[derive(Clone)]
pub struct Root {
    store: Store,
    origin: Origin,
}

#[derive(Clone)]
enum Store {
    Local(Arc<FilesystemStore>),
    Remote(Arc<RemoteStore>),
}

#[derive(Clone)]
enum Origin {
    Local(PathBuf),
    Remote { target: Target },
}

impl Root {
    pub fn local(path: &Path) -> Result<Self, Error> {
        grana_file::recover(path).map_err(|error| Error::Open {
            path: path.display().to_string(),
            detail: format!("failed to recover the dataset: {error}"),
        })?;
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
        let store = shared(&target);
        Ok(Self {
            store: Store::Remote(store),
            origin: Origin::Remote { target },
        })
    }

    pub fn path(&self) -> Option<&Path> {
        match &self.origin {
            Origin::Local(path) => Some(path),
            Origin::Remote { .. } => None,
        }
    }

    pub fn display(&self) -> String {
        match &self.origin {
            Origin::Local(path) => path.display().to_string(),
            Origin::Remote { target } => target.display().to_string(),
        }
    }

    pub fn transfer(&self) -> Option<Arc<Transfer>> {
        match &self.store {
            Store::Local(_) => None,
            Store::Remote(store) => Some(Arc::clone(&store.transfer)),
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
            Origin::Remote { target } => {
                let group = group.trim_matches('/');
                if group.is_empty() {
                    target.display().to_string()
                } else {
                    target.join(group)
                }
            }
        }
    }

    pub(crate) fn witness(&self, directory: &str) -> Witness {
        match &self.store {
            Store::Local(_) => {
                let root = self.path().expect("a local root names its directory");
                Witness::Local(Local::new(root))
            }
            Store::Remote(store) => Witness::Remote {
                store: Arc::clone(store),
                tag: self.tag(directory),
            },
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

pub(crate) enum Witness {
    Local(Local),
    Remote {
        store: Arc<RemoteStore>,
        tag: String,
    },
}

impl Witness {
    pub(crate) fn stamp(&self, keys: impl Iterator<Item = String>) -> Result<Option<u64>, Error> {
        match self {
            Self::Local(local) => crate::identity::chunks(keys, |key| local.metadata(key))
                .map(Some)
                .map_err(|error| Error::Read {
                    array: local.root().display().to_string(),
                    detail: error.to_string(),
                }),
            Self::Remote { store, tag } => {
                let mut hash = witness::START;
                for key in keys {
                    hash = witness::mix(hash, key.as_bytes());
                    let probed = store.probe(&key).map_err(|error| Error::Read {
                        array: tag.clone(),
                        detail: error.to_string(),
                    })?;
                    match probed {
                        Presence::Missing => hash = witness::mix(hash, &[0]),
                        Presence::Object {
                            size: Some(size),
                            etag,
                            last_modified,
                        } => {
                            let Some(validator) = etag.as_deref().or(last_modified.as_deref())
                            else {
                                return Ok(None);
                            };
                            hash = witness::mix(hash, &[1]);
                            hash = witness::mix(hash, &size.to_le_bytes());
                            hash = witness::mix(hash, validator.as_bytes());
                        }
                        Presence::Object { size: None, .. } => return Ok(None),
                    }
                }
                Ok(Some(hash))
            }
        }
    }
}

enum Block {
    Ready(Arc<Vec<u8>>),
    Ranged,
}

pub(crate) struct RemoteStore {
    target: Target,
    client: Arc<Client>,
    probes: Mutex<HashMap<String, Presence>>,
    blocks: LruCache<String, Vec<u8>>,
    transfer: Arc<Transfer>,
}

impl Drop for RemoteStore {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

static LIVE: AtomicUsize = AtomicUsize::new(0);

fn shared(target: &Target) -> Arc<RemoteStore> {
    static STORES: OnceLock<Mutex<HashMap<String, Weak<RemoteStore>>>> = OnceLock::new();
    let mut stores = STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    stores.retain(|_, held| held.strong_count() > 0);
    if let Some(store) = stores.get(target.display()).and_then(Weak::upgrade) {
        return store;
    }
    let store = Arc::new(RemoteStore {
        target: target.clone(),
        client: Client::shared(),
        probes: Mutex::new(HashMap::new()),
        blocks: LruCache::scoped(Pool::Remote, &LIVE, |bytes: &Vec<u8>| bytes.len() as u64),
        transfer: Arc::new(Transfer::default()),
    });
    LIVE.fetch_add(1, Ordering::Relaxed);
    stores.insert(target.display().to_string(), Arc::downgrade(&store));
    store
}

impl RemoteStore {
    fn url(&self, key: &str) -> String {
        self.target.join(key)
    }

    fn fetch(&self, key: &StoreKey, span: Option<Span>) -> Result<Option<Vec<u8>>, StorageError> {
        let fetched = self
            .client
            .get(&self.url(key.as_str()), span)
            .map_err(|error| StorageError::Other(error.to_string()))?;
        let Some(fetched) = fetched else {
            return Ok(None);
        };
        self.transfer
            .record(fetched.bytes.len() as u64, fetched.resumed);
        Ok(Some(fetched.bytes))
    }

    fn probe(&self, key: &str) -> Result<Presence, StorageError> {
        if let Some(probed) = self
            .probes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
        {
            return Ok(probed.clone());
        }
        let probed = self
            .client
            .probe(&self.url(key))
            .map_err(|error| StorageError::Other(error.to_string()))?;
        self.probes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key.to_string(), probed.clone());
        Ok(probed)
    }

    fn size(&self, key: &StoreKey) -> Result<Option<u64>, StorageError> {
        Ok(self.probe(key.as_str())?.size())
    }

    fn name(&self, key: &StoreKey) -> String {
        let mut name = self.target.display().to_string();
        name.push('\u{0}');
        name.push_str(key.as_str());
        name
    }

    fn block(&self, key: &StoreKey) -> Result<Option<Block>, StorageError> {
        let name = self.name(key);
        if let Some(block) = self.blocks.fetch(name.clone()) {
            return Ok(Some(Block::Ready(block)));
        }
        match self.probe(key.as_str())? {
            Presence::Missing => Ok(None),
            Presence::Object {
                size: Some(size), ..
            } if size > BLOCK_LIMIT => Ok(Some(Block::Ranged)),
            Presence::Object { .. } => {
                let Some(bytes) = self.fetch(key, None)? else {
                    return Ok(None);
                };
                let block = Arc::new(bytes);
                self.blocks.insert(name, Arc::clone(&block));
                Ok(Some(Block::Ready(block)))
            }
        }
    }

    fn ranged(&self, key: &StoreKey, range: ByteRange) -> Result<MaybeBytes, StorageError> {
        let size = match self.probe(key.as_str())? {
            Presence::Missing => return Ok(None),
            Presence::Object { size, .. } => size,
        };
        let span = span(range, size)?;
        let target = self.url(key.as_str());
        let Some(fetched) = self
            .client
            .get(&target, Some(span))
            .map_err(|error| StorageError::Other(error.to_string()))?
        else {
            return Ok(None);
        };
        self.transfer
            .record(fetched.bytes.len() as u64, fetched.resumed);
        Ok(Some(Bytes::from(fetched.bytes)))
    }
}

impl ReadableStorageTraits for RemoteStore {
    fn get(&self, key: &StoreKey) -> Result<MaybeBytes, StorageError> {
        Ok(self.fetch(key, None)?.map(Bytes::from))
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
    let required = || {
        StorageError::Other(
            "the byte range counts from the end of a key whose size stays unknown".to_string(),
        )
    };
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
