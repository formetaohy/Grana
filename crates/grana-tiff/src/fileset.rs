use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_ome::{Image as OmeImage, TiffData};

use crate::directory::{self, Directory, Header};
use crate::error::Error;
use crate::layout::Layout;
use crate::page::Page;
use crate::store::Store;
use crate::tag::TAG_IMAGE_DESCRIPTION;

const PAGE_BYTES: u64 = 8 << 20;

const LAYOUT_BYTES: u64 = 32 << 20;

const EXTENSIONS: [&str; 5] = ["tif", "tiff", "tf2", "tf8", "btf"];

type PageCache = grana_store::LruCache<(usize, usize), Page>;
type LayoutCache = grana_store::LruCache<(usize, usize), Layout>;

pub(crate) struct File {
    pub(crate) path: PathBuf,
    pub(crate) store: Store,
    pub(crate) header: Header,
    pub(crate) ifds: Vec<u64>,
    pub(crate) pages: Vec<Arc<Page>>,
    reduced: PageCache,
    layouts: LayoutCache,
}

impl File {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let store = Store::open(path)?;
        let header = directory::header(&store, path)?;
        let ifds = Directory::chain(&store, &header, path)?;
        let mut pages = Vec::with_capacity(ifds.len());
        for offset in &ifds {
            let held = Directory::read(&store, &header, *offset, path)?;
            pages.push(Arc::new(Page::new(&held, &store, header.big_endian, path)?));
        }
        Ok(Self {
            path: path.to_path_buf(),
            store,
            header,
            ifds,
            pages,
            reduced: PageCache::new(PAGE_BYTES, |page| page.bytes()),
            layouts: LayoutCache::new(LAYOUT_BYTES, |layout| layout.bytes()),
        })
    }

    pub(crate) fn page(&self, page: usize, level: usize) -> Result<Arc<Page>, Error> {
        if level == 0 {
            return self
                .pages
                .get(page)
                .cloned()
                .ok_or_else(|| Error::read(&self.path, "the TIFF page is missing"));
        }
        if let Some(held) = self.reduced.fetch((page, level)) {
            return Ok(held);
        }
        let offset = self
            .pages
            .get(page)
            .and_then(|held| held.sub_ifds.get(level - 1))
            .copied()
            .ok_or_else(|| {
                Error::metadata(
                    &self.path,
                    "a TIFF page carries no reduced resolution image",
                )
            })?;
        let directory = Directory::read(&self.store, &self.header, offset, &self.path)?;
        let held = Arc::new(Page::new(
            &directory,
            &self.store,
            self.header.big_endian,
            &self.path,
        )?);
        self.reduced.insert((page, level), Arc::clone(&held));
        Ok(held)
    }

    pub(crate) fn layout(&self, page: usize, level: usize) -> Result<Arc<Layout>, Error> {
        if let Some(held) = self.layouts.fetch((page, level)) {
            return Ok(held);
        }
        let held = self.page(page, level)?;
        let offset = if level == 0 {
            self.ifds[page]
        } else {
            self.pages[page].sub_ifds[level - 1]
        };
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
        self.layouts.insert((page, level), Arc::clone(&layout));
        Ok(layout)
    }

    pub(crate) fn directory(&self) -> Result<Directory, Error> {
        let offset = self
            .ifds
            .first()
            .copied()
            .ok_or_else(|| Error::metadata(&self.path, "the file holds no TIFF directory"))?;
        Directory::read(&self.store, &self.header, offset, &self.path)
    }

    pub(crate) fn page_count(&self) -> usize {
        self.pages.len()
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
                let file = File::open(&sibling)?;
                let signature = identify(&file).map_err(|detail| {
                    Error::metadata(
                        &path,
                        format!(
                            "the OME-TIFF file `{name}` carries no readable OME metadata: {detail}"
                        ),
                    )
                })?;
                let index = members.len();
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
        let Ok(file) = File::open(&candidate) else {
            continue;
        };
        let Ok(Some(signature)) = identify(&file) else {
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
        let index = members.len();
        signatures.insert(signature.clone(), index);
        record_name(names, &candidate, index);
        unresolved.retain(|held| *held != signature);
        members.push(file);
    }
    Ok(())
}

fn identify(file: &File) -> Result<Option<String>, String> {
    let directory = file.directory().map_err(|error| error.to_string())?;
    let description = directory
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
