use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use grana_ome::{Image as OmeImage, TiffData};

use crate::directory::{self, Directory, Header};
use crate::error::Error;
use crate::layout::Layout;
use crate::page::Page;
use crate::store::{Handles, Store};
use crate::tag::TAG_IMAGE_DESCRIPTION;

const PAGE_BYTES: u64 = 8 << 20;

const LAYOUT_BYTES: u64 = 32 << 20;

const MEMO_LIMIT: usize = 1 << 16;

const EXTENSIONS: [&str; 5] = ["tif", "tiff", "tf2", "tf8", "btf"];

const FNV_BASE: u64 = 0xcbf2_9ce4_8422_2325;

type PageCache = grana_store::LruCache<(usize, usize, usize), Page>;
type LayoutCache = grana_store::LruCache<(usize, usize, usize), Layout>;
type MemoKey = (PathBuf, u64, u128);

static PROBED: OnceLock<Mutex<HashMap<MemoKey, Option<String>>>> = OnceLock::new();

pub(crate) struct Caches {
    pages: PageCache,
    layouts: LayoutCache,
}

impl Caches {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            pages: PageCache::new(PAGE_BYTES, |page| page.bytes()),
            layouts: LayoutCache::new(LAYOUT_BYTES, |layout| layout.bytes()),
        })
    }
}

pub(crate) struct File {
    pub(crate) index: usize,
    pub(crate) path: PathBuf,
    pub(crate) store: Store,
    pub(crate) header: Header,
    pub(crate) ifds: Vec<u64>,
    first: Directory,
    caches: Arc<Caches>,
}

impl File {
    pub(crate) fn open(path: &Path, caches: Arc<Caches>) -> Result<Self, Error> {
        Self::open_with(path, 0, caches, None)
    }

    fn open_with(
        path: &Path,
        index: usize,
        caches: Arc<Caches>,
        handles: Option<Arc<Handles>>,
    ) -> Result<Self, Error> {
        let store = match handles {
            Some(handles) => Store::open_with(path, handles)?,
            None => Store::open(path)?,
        };
        let header = directory::header(&store, path)?;
        let ifds = Directory::chain(&store, &header, path)?;
        let first = ifds
            .first()
            .map(|offset| Directory::read(&store, &header, *offset, path))
            .transpose()?
            .ok_or_else(|| Error::metadata(path, "the file holds no TIFF directory"))?;
        Ok(Self {
            index,
            path: path.to_path_buf(),
            store,
            header,
            ifds,
            first,
            caches,
        })
    }

    pub(crate) fn page(&self, page: usize, level: usize) -> Result<Arc<Page>, Error> {
        if let Some(held) = self.caches.pages.fetch((self.index, page, level)) {
            return Ok(held);
        }
        let offset = self.offset(page, level)?;
        let directory = Directory::read(&self.store, &self.header, offset, &self.path)?;
        let held = Arc::new(Page::new(
            &directory,
            &self.store,
            self.header.big_endian,
            &self.path,
        )?);
        self.caches
            .pages
            .insert((self.index, page, level), Arc::clone(&held));
        Ok(held)
    }

    pub(crate) fn layout(&self, page: usize, level: usize) -> Result<Arc<Layout>, Error> {
        if let Some(held) = self.caches.layouts.fetch((self.index, page, level)) {
            return Ok(held);
        }
        let held = self.page(page, level)?;
        let offset = self.offset(page, level)?;
        let directory = Directory::read(&self.store, &self.header, offset, &self.path)?;
        let layout = Arc::new(Layout::new(
            &directory,
            &self.store,
            held.width,
            held.height,
            held.samples,
            held.planar,
            &self.path,
        )?);
        self.caches
            .layouts
            .insert((self.index, page, level), Arc::clone(&layout));
        Ok(layout)
    }

    fn offset(&self, page: usize, level: usize) -> Result<u64, Error> {
        if level == 0 {
            return self
                .ifds
                .get(page)
                .copied()
                .ok_or_else(|| Error::read(&self.path, "the TIFF page is missing"));
        }
        let base = self.page(page, 0)?;
        base.sub_ifds.get(level - 1).copied().ok_or_else(|| {
            Error::metadata(
                &self.path,
                "a TIFF page carries no reduced resolution image",
            )
        })
    }

    pub(crate) fn directory(&self) -> &Directory {
        &self.first
    }

    pub(crate) fn page_count(&self) -> usize {
        self.ifds.len()
    }

    fn digest(&self) -> u64 {
        let path = std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        let hash = mix(FNV_BASE, path.to_string_lossy().as_bytes());
        let metadata = std::fs::metadata(&self.path).ok();
        let length = metadata.as_ref().map_or(0, std::fs::Metadata::len);
        let modified = metadata.as_ref().map_or(0, |held| {
            held.modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos() as u64)
        });
        mix(mix(hash, &length.to_le_bytes()), &modified.to_le_bytes())
    }
}

pub(crate) struct Fileset {
    path: PathBuf,
    members: Vec<File>,
    names: HashMap<String, usize>,
    signatures: HashMap<String, usize>,
}

impl Fileset {
    pub(crate) fn open(
        root: File,
        signature: Option<String>,
        ome: Option<&OmeImage>,
    ) -> Result<Self, Error> {
        let path = root.path.clone();
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        let mut names = HashMap::new();
        let mut signatures = HashMap::new();
        let mut members = Vec::new();
        if let Some(signature) = &signature {
            signatures.insert(signature.clone(), 0);
        }
        record_name(&mut names, &path, 0);
        let caches = Arc::clone(&root.caches);
        let handles = root.store.handles();
        members.push(root);
        let mut unresolved = Vec::new();
        for block in ome.map_or(&[][..], |ome| ome.tiff_data.as_slice()) {
            if let Some(name) = block.file.as_deref()
                && !named(&path, name)
                && !known(&names, name)
            {
                let sibling = directory.join(name);
                if !sibling.is_file() {
                    return Err(Error::metadata(
                        &path,
                        format!("the OME-TIFF series names the missing file `{name}`"),
                    ));
                }
                let index = members.len();
                let file = File::open_with(
                    &sibling,
                    index,
                    Arc::clone(&caches),
                    Some(Arc::clone(&handles)),
                )?;
                let signature = identify(&file).map_err(|detail| {
                    Error::metadata(
                        &path,
                        format!(
                            "the OME-TIFF file `{name}` carries no readable OME metadata: {detail}"
                        ),
                    )
                })?;
                if let Some(signature) = &signature
                    && let Some(held) = signatures.insert(signature.clone(), index)
                {
                    return Err(Error::metadata(
                        &path,
                        format!(
                            "the OME-TIFF files `{}` and `{name}` declare one UUID",
                            members[held].path.display()
                        ),
                    ));
                }
                record_name(&mut names, &sibling, index);
                names.insert(name.to_string(), index);
                members.push(file);
            }
            if let Some(signature) = block.uuid.as_deref()
                && !signatures.contains_key(signature)
                && !unresolved.iter().any(|held: &String| held == signature)
            {
                unresolved.push(signature.to_string());
            }
        }
        if !unresolved.is_empty() {
            scan(
                directory,
                &caches,
                &handles,
                &mut members,
                &mut names,
                &mut signatures,
                &mut unresolved,
            )?;
            if let Some(missing) = unresolved.first() {
                return Err(Error::metadata(
                    &path,
                    format!("the OME-TIFF series references the missing UUID `{missing}`"),
                ));
            }
        }
        Ok(Self {
            path,
            members,
            names,
            signatures,
        })
    }

    pub(crate) fn root(&self) -> &File {
        &self.members[0]
    }

    pub(crate) fn file(&self, index: usize) -> &File {
        &self.members[index]
    }

    pub(crate) fn count(&self) -> usize {
        self.members.len()
    }

    pub(crate) fn stamp(&self) -> u64 {
        let mut digests = self.members.iter().map(File::digest).collect::<Vec<_>>();
        digests.sort_unstable();
        digests
            .iter()
            .fold(FNV_BASE, |hash, digest| mix(hash, &digest.to_le_bytes()))
    }

    pub(crate) fn resolve(&self, block: &TiffData) -> Result<usize, Error> {
        if let Some(name) = block.file.as_deref() {
            if named(&self.path, name) {
                return Ok(0);
            }
            if let Some(index) = self.names.get(name) {
                return Ok(*index);
            }
            if let Some(bare) = Path::new(name).file_name().and_then(|held| held.to_str())
                && let Some(index) = self.names.get(bare)
            {
                return Ok(*index);
            }
            return Err(Error::metadata(
                &self.path,
                format!("the OME-TIFF series names the missing file `{name}`"),
            ));
        }
        if let Some(signature) = block.uuid.as_deref() {
            return self.signatures.get(signature).copied().ok_or_else(|| {
                Error::metadata(
                    &self.path,
                    format!("the OME-TIFF series references the missing UUID `{signature}`"),
                )
            });
        }
        Ok(0)
    }
}

fn scan(
    directory: &Path,
    caches: &Arc<Caches>,
    handles: &Arc<crate::store::Handles>,
    members: &mut Vec<File>,
    names: &mut HashMap<String, usize>,
    signatures: &mut HashMap<String, usize>,
    unresolved: &mut Vec<String>,
) -> Result<(), Error> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    let held = members
        .iter()
        .map(|member| member.path.clone())
        .collect::<Vec<_>>();
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && !held.contains(path))
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                })
        })
        .collect::<Vec<_>>();
    candidates.sort();
    for candidate in candidates {
        if unresolved.is_empty() {
            break;
        }
        let Ok(Some(signature)) = signature(&candidate, handles) else {
            continue;
        };
        if !unresolved.contains(&signature) {
            continue;
        }
        if let Some(held) = signatures.get(&signature) {
            return Err(Error::metadata(
                &members[*held].path,
                format!(
                    "the OME-TIFF files `{}` and `{}` declare one UUID",
                    members[*held].path.display(),
                    candidate.display()
                ),
            ));
        }
        let file = File::open_with(
            &candidate,
            members.len(),
            Arc::clone(caches),
            Some(Arc::clone(handles)),
        )?;
        let index = members.len();
        signatures.insert(signature.clone(), index);
        record_name(names, &candidate, index);
        unresolved.retain(|held| *held != signature);
        members.push(file);
    }
    Ok(())
}

fn signature(path: &Path, handles: &Arc<Handles>) -> Result<Option<String>, Error> {
    let metadata = fs::metadata(path).map_err(|error| Error::open(path, error.to_string()))?;
    let key = (
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        metadata.len(),
        modified(&metadata),
    );
    let memo = PROBED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(held) = memo
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
    {
        return Ok(held.clone());
    }
    let store = Store::open_with(path, Arc::clone(handles))?;
    let header = directory::header(&store, path)?;
    let directory = Directory::read(&store, &header, header.first, path)?;
    let description = directory.ascii(&store, TAG_IMAGE_DESCRIPTION, path)?;
    let signature = grana_ome::parse(description.as_deref().unwrap_or_default())
        .map_err(|detail| Error::metadata(path, detail))?
        .uuid;
    let mut memo = memo.lock().unwrap_or_else(|error| error.into_inner());
    if memo.len() >= MEMO_LIMIT {
        memo.clear();
    }
    memo.insert(key, signature.clone());
    Ok(signature)
}

fn modified(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn identify(file: &File) -> Result<Option<String>, String> {
    let description = file
        .directory()
        .ascii(&file.store, TAG_IMAGE_DESCRIPTION, &file.path)
        .map_err(|error| error.to_string())?;
    let metadata = grana_ome::parse(description.as_deref().unwrap_or_default())
        .map_err(|error| error.to_string())?;
    Ok(metadata.uuid)
}

fn record_name(names: &mut HashMap<String, usize>, path: &Path, index: usize) {
    if let Some(name) = path.file_name().and_then(|held| held.to_str()) {
        names.entry(name.to_string()).or_insert(index);
    }
}

fn known(names: &HashMap<String, usize>, file: &str) -> bool {
    if names.contains_key(file) {
        return true;
    }
    Path::new(file)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| names.contains_key(name))
}

fn named(path: &Path, file: &str) -> bool {
    let held = path.file_name().and_then(|held| held.to_str());
    if held == Some(file) {
        return true;
    }
    let bare = Path::new(file).file_name().and_then(|name| name.to_str());
    held.is_some() && held == bare
}

fn mix(hash: u64, bytes: &[u8]) -> u64 {
    let mut hash = hash;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}
