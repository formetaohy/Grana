use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grana_net::{Client, Probe, Span, Target};
use zarrs::filesystem::FilesystemStore;
use zarrs::storage::byte_range::{ByteRange, ByteRangeIterator};
use zarrs::storage::{
    Bytes, ListableStorageTraits, MaybeBytes, MaybeBytesIterator, ReadableListableStorageTraits,
    ReadableStorageTraits, StorageError, StoreKey, StoreKeys, StoreKeysPrefixes, StorePrefix,
};

use crate::Error;
use crate::witness::{self, Local};

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
        let store = RemoteStore {
            target: target.clone(),
            client: Client::shared(),
            blocks: Mutex::new(Blocks::default()),
            probes: Mutex::new(HashMap::new()),
        };
        Ok(Self {
            store: Store::Remote(Arc::new(store)),
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

    pub(crate) fn witness(&self, directory: &str) -> Result<Witness, Error> {
        match &self.store {
            Store::Local(_) => {
                let root = self.path().expect("a local root names its directory");
                let local = Local::of(root, directory).map_err(|error| Error::Read {
                    array: directory.to_string(),
                    detail: error.to_string(),
                })?;
                Ok(Witness::Local(local))
            }
            Store::Remote(store) => Ok(Witness::Remote {
                store: Arc::clone(store),
                tag: self.tag(directory),
            }),
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
            Self::Local(local) => Ok(Some(crate::identity::chunks(keys, |key| {
                local.metadata(key)
            }))),
            Self::Remote { store, tag } => {
                let mut hash = witness::START;
                for key in keys {
                    hash = witness::mix(hash, key.as_bytes());
                    let probed = store.probe(&key).map_err(|error| Error::Read {
                        array: tag.clone(),
                        detail: error.to_string(),
                    })?;
                    match probed {
                        Some(probe) => {
                            let Some(size) = probe.size else {
                                return Ok(None);
                            };
                            let Some(validator) =
                                probe.etag.as_deref().or(probe.last_modified.as_deref())
                            else {
                                return Ok(None);
                            };
                            hash = witness::mix(hash, &[1]);
                            hash = witness::mix(hash, &size.to_le_bytes());
                            hash = witness::mix(hash, validator.as_bytes());
                        }
                        None => hash = witness::mix(hash, &[0]),
                    }
                }
                Ok(Some(hash))
            }
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
        if let Some(held) = self.entries.get(&key) {
            self.bytes -= held.len() as u64;
            self.order.retain(|held| held != &key);
        }
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

pub(crate) struct RemoteStore {
    target: Target,
    client: Arc<Client>,
    blocks: Mutex<Blocks>,
    probes: Mutex<HashMap<String, Option<Probe>>>,
}

impl RemoteStore {
    fn url(&self, key: &str) -> String {
        self.target.join(key)
    }

    fn fetch(&self, key: &StoreKey, span: Option<Span>) -> Result<Option<Vec<u8>>, StorageError> {
        self.client
            .get(&self.url(key.as_str()), span)
            .map_err(|error| StorageError::Other(error.to_string()))
    }

    fn probe(&self, key: &str) -> Result<Option<Probe>, StorageError> {
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
        Ok(self.probe(key.as_str())?.and_then(|probe| probe.size))
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
        let Some(bytes) = self.fetch(key, None)? else {
            return Ok(None);
        };
        let block = Arc::new(bytes);
        self.blocks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name, Arc::clone(&block));
        Ok(Some(Block::Ready(block)))
    }

    fn ranged(&self, key: &StoreKey, range: ByteRange) -> Result<MaybeBytes, StorageError> {
        let size = self.size(key)?;
        let span = span(range, size)?;
        Ok(self.fetch(key, Some(span))?.map(Bytes::from))
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
