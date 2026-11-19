use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    ChannelInfo, Extent, Intensity, Level, Probe, Pyramid, Spacing, VoxelData, VoxelFormat,
    name_of, tint_of,
};

use grana_ome::{Image as OmeImage, TiffData};

use crate::Cache;
use crate::error::Error;
use crate::page::{Layout, Page, Plane, Region, Source as PageSource};
use crate::planes::Planes;
use crate::store::Store;
use crate::tag::{
    self, Directory, TAG_IMAGE_DESCRIPTION, TAG_RESOLUTION_UNIT, TAG_X_RESOLUTION, TAG_Y_RESOLUTION,
};

const CACHE_BYTES: u64 = 128 << 20;
const LAYOUT_BYTES: u64 = 32 << 20;
const PAGE_BYTES: u64 = 8 << 20;

type PageCache = grana_store::LruCache<(usize, usize), Page>;
type LayoutCache = grana_store::LruCache<(usize, usize), Layout>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    extent: Extent,
    channels: usize,
    frames: u32,
    name: Option<String>,
}

impl Series {
    fn new(extent: Extent, channels: usize, frames: u32, name: Option<String>) -> Self {
        Self {
            extent,
            channels,
            frames,
            name,
        }
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

pub struct Image {
    path: PathBuf,
    name: String,
    pyramid: Pyramid,
    levels: Vec<LevelPages>,
    series: Vec<Series>,
    files: Vec<Arc<File>>,
    cache: Cache,
}

struct File {
    path: PathBuf,
    store: Store,
    header: tag::Header,
    ifds: Vec<u64>,
    pages: Vec<Arc<Page>>,
    reduced: PageCache,
    layouts: LayoutCache,
}

struct LevelPages {
    extent: Extent,
    planes: Planes,
    slots: Vec<Slot>,
    format: VoxelFormat,
}

#[derive(Clone, Copy)]
struct Slot {
    file: usize,
    page: usize,
}

struct Request {
    time: u32,
    level: usize,
    channel: u32,
    x: Range<u32>,
    y: Range<u32>,
    z: Range<u32>,
}

impl File {
    fn open(path: &Path) -> Result<Self, Error> {
        let store = Store::open(path)?;
        let header = tag::header(&store, path)?;
        let ifds = Directory::chain(&store, &header, path)?;
        let mut pages = Vec::with_capacity(ifds.len());
        for offset in &ifds {
            pages.push(Arc::new(Self::page_at(&store, &header, *offset, path)?));
        }
        Ok(Self {
            path: path.to_path_buf(),
            store,
            header,
            ifds,
            pages,
            reduced: PageCache::new(PAGE_BYTES, Page::bytes),
            layouts: LayoutCache::new(LAYOUT_BYTES, Layout::bytes),
        })
    }

    fn page_at(
        store: &Store,
        header: &tag::Header,
        offset: u64,
        path: &Path,
    ) -> Result<Page, Error> {
        let directory = Directory::read(store, header, offset, path)?;
        Page::new(&directory, store, header.big_endian, path)
    }

    fn page(&self, page: usize, level: usize) -> Result<Arc<Page>, Error> {
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
        let offset = self.offset(page, level)?;
        let held = Arc::new(Self::page_at(
            &self.store,
            &self.header,
            offset,
            &self.path,
        )?);
        self.reduced.insert((page, level), Arc::clone(&held));
        Ok(held)
    }

    fn layout(&self, page: usize, level: usize) -> Result<Arc<Layout>, Error> {
        if let Some(held) = self.layouts.fetch((page, level)) {
            return Ok(held);
        }
        let held = Arc::new(self.read_layout(page, level)?);
        self.layouts.insert((page, level), Arc::clone(&held));
        Ok(held)
    }

    fn read_layout(&self, page: usize, level: usize) -> Result<Layout, Error> {
        let held = self.page(page, level)?;
        let offset = self.offset(page, level)?;
        let directory = Directory::read(&self.store, &self.header, offset, &self.path)?;
        Layout::new(&directory, &self.store, held.width, held.height, &self.path)
    }

    fn offset(&self, page: usize, level: usize) -> Result<u64, Error> {
        if level == 0 {
            return self
                .ifds
                .get(page)
                .copied()
                .ok_or_else(|| Error::read(&self.path, "the TIFF page is missing"));
        }
        self.pages
            .get(page)
            .and_then(|held| held.sub_ifds.get(level - 1))
            .copied()
            .ok_or_else(|| {
                Error::metadata(
                    &self.path,
                    "a TIFF page carries no reduced resolution image",
                )
            })
    }

    fn directory(&self) -> Result<Directory, Error> {
        let offset = self
            .ifds
            .first()
            .copied()
            .ok_or_else(|| Error::metadata(&self.path, "the file holds no TIFF directory"))?;
        Directory::read(&self.store, &self.header, offset, &self.path)
    }

    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn depth(&self) -> usize {
        self.pages
            .iter()
            .map(|page| page.sub_ifds.len())
            .min()
            .unwrap_or(0)
    }
}

impl Image {
    pub fn open(path: &Path, series: usize) -> Result<Self, Error> {
        let first = File::open(path)?;
        let images = metadata(&first, path)?;
        let page = first.page(0, 0)?;
        let chunky = !page.planar && page.samples > 1;
        let format = page.format;
        let samples = page.samples;
        let stacked = first
            .pages
            .iter()
            .take_while(|held| held.width == page.width && held.height == page.height)
            .count();
        let declared = declared(&images, &page, stacked, chunky);
        let ome = match images.get(series) {
            Some(ome) => Some(ome),
            None if series == 0 => None,
            None => {
                return Err(Error::unsupported(
                    path,
                    format!(
                        "the image holds {} series and series {} was requested",
                        declared.len(),
                        series + 1
                    ),
                ));
            }
        };
        let sizes = counts(ome, stacked, samples, chunky, path)?;
        let order = ome
            .and_then(|ome| ome.dimension_order.clone())
            .unwrap_or_else(|| "XYZCT".to_string());
        let planes = Planes::new(sizes[2], sizes[1], sizes[0], chunky, &order)
            .map_err(|detail| Error::metadata(path, detail))?;
        let mut files = vec![Arc::new(first)];
        let mut placed: HashMap<usize, Slot> = HashMap::new();
        let mut requested = Vec::new();
        assign(
            0,
            &files[0],
            ome,
            &planes,
            format,
            &mut placed,
            &mut requested,
        )?;
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        for name in requested {
            let sibling = directory.join(&name);
            if !sibling.is_file() {
                return Err(Error::metadata(
                    path,
                    format!("the OME-TIFF series names the missing file `{name}`"),
                ));
            }
            let index = files.len();
            files.push(Arc::new(File::open(&sibling)?));
            let mut ignored = Vec::new();
            assign(
                index,
                &files[index],
                ome,
                &planes,
                format,
                &mut placed,
                &mut ignored,
            )?;
        }
        if placed.len() != planes.count() as usize {
            return Err(Error::metadata(
                path,
                format!(
                    "the OME-TIFF series describes {} planes and its files hold {}",
                    planes.count(),
                    placed.len()
                ),
            ));
        }
        let mut levels = vec![gather(&files, &planes, &placed, format, 0, path)?];
        let deepest = files.iter().map(|file| file.depth()).min().unwrap_or(0);
        for level in 1..=deepest {
            let deeper = gather(&files, &planes, &placed, format, level, path)?;
            let previous = levels.last().expect("the base level is present");
            if deeper.extent.x() > previous.extent.x()
                || deeper.extent.y() > previous.extent.y()
                || deeper.extent.z() > previous.extent.z()
                || deeper.extent == previous.extent
            {
                break;
            }
            levels.push(deeper);
        }
        let base = levels[0].extent;
        let spacing = spacing(ome, &files[0], path)?;
        let pyramid_levels = levels
            .iter()
            .map(|level| {
                let extent = level.extent;
                Level::new(
                    extent,
                    Spacing::new(
                        spacing.x() * base.x() as f32 / extent.x() as f32,
                        spacing.y() * base.y() as f32 / extent.y() as f32,
                        spacing.z() * base.z() as f32 / extent.z() as f32,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let cache = crate::cache(CACHE_BYTES);
        let mut channels = Vec::with_capacity(sizes[1] as usize);
        for index in 0..sizes[1] {
            let info = ome.and_then(|ome| ome.channels.get(index as usize));
            let name = info
                .and_then(|channel| channel.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| name_of(index));
            let tint = info
                .and_then(|channel| channel.tint)
                .unwrap_or_else(|| tint_of(index));
            let sampled = sample_range(&files, &levels, &cache, index, sizes[0])?;
            channels.push(ChannelInfo::resolve(
                name,
                tint,
                format,
                &[sampled],
                &[sampled],
            ));
        }
        let interval = ome.and_then(|ome| ome.interval).unwrap_or(1.0);
        let name = ome
            .and_then(|ome| ome.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| dataset_name(path));
        let placement = placement(ome, pyramid_levels[0], sizes[2], path)?;
        let stored = pyramid_levels.len();
        Ok(Self {
            path: path.to_path_buf(),
            name,
            pyramid: Pyramid::placed(
                pyramid_levels,
                stored,
                channels,
                sizes[0],
                interval,
                placement,
            ),
            levels,
            series: declared,
            files,
            cache,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<VoxelData, Error> {
        region(
            &self.files,
            &self.cache,
            &self.levels,
            &Request {
                time,
                level,
                channel: u32::try_from(channel).map_err(|_| {
                    Error::unsupported(&self.path, "the requested channel is out of range")
                })?,
                x,
                y,
                z,
            },
        )
    }
}

fn named(path: &Path, file: &str) -> bool {
    let held = path.file_name().and_then(|held| held.to_str());
    if held == Some(file) {
        return true;
    }
    let bare = Path::new(file).file_name().and_then(|name| name.to_str());
    held.is_some() && held == bare
}

fn assign(
    file: usize,
    volume: &File,
    ome: Option<&OmeImage>,
    planes: &Planes,
    format: VoxelFormat,
    placed: &mut HashMap<usize, Slot>,
    requested: &mut Vec<String>,
) -> Result<(), Error> {
    let path = &volume.path;
    let blocks = ome.map_or(&[][..], |ome| ome.tiff_data.as_slice());
    let mut raster: Vec<(usize, usize)> = Vec::new();
    for block in blocks {
        if let Some(name) = block.file.as_deref()
            && !named(path, name)
        {
            if !requested.iter().any(|held| held == name) {
                requested.push(name.to_string());
            }
            continue;
        }
        place(block, planes, &mut raster, path)?;
    }
    if blocks.is_empty() {
        let limit = volume.page_count().min(planes.count() as usize);
        raster.extend((0..limit).map(|page| (page, page)));
    }
    for (index, page) in raster {
        if page >= volume.page_count() {
            return Err(Error::metadata(
                path,
                "the OME-TIFF metadata names a missing TIFF page",
            ));
        }
        let held = volume.page(page, 0)?;
        if held.format != format {
            return Err(Error::unsupported(
                path,
                "the TIFF pages of one series disagree on their voxel type",
            ));
        }
        if index >= planes.count() as usize {
            return Err(Error::metadata(
                path,
                "the OME-TIFF metadata places a plane outside the declared pixels",
            ));
        }
        placed.entry(index).or_insert(Slot { file, page });
    }
    Ok(())
}

fn place(
    block: &TiffData,
    planes: &Planes,
    raster: &mut Vec<(usize, usize)>,
    path: &Path,
) -> Result<(), Error> {
    if block.z >= planes.depth() || block.c >= planes.channels() || block.t >= planes.frames() {
        return Err(Error::metadata(
            path,
            "the OME-TIFF TiffData starts outside the declared pixels",
        ));
    }
    let start = planes.index(block.t, block.c, block.z);
    for offset in 0..block.planes {
        let index = start + offset as usize;
        if index >= planes.count() as usize {
            return Err(Error::metadata(
                path,
                "the OME-TIFF TiffData runs past the declared pixels",
            ));
        }
        raster.push((index, block.ifd as usize + offset as usize));
    }
    Ok(())
}

fn gather(
    files: &[Arc<File>],
    planes: &Planes,
    placed: &HashMap<usize, Slot>,
    format: VoxelFormat,
    level: usize,
    path: &Path,
) -> Result<LevelPages, Error> {
    let mut ordered = placed.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(index, _)| **index);
    let mut slots = Vec::with_capacity(ordered.len());
    let mut extent: Option<Extent> = None;
    for (_, slot) in ordered {
        let page = files[slot.file].page(slot.page, level)?;
        if page.format != format {
            return Err(Error::unsupported(
                path,
                "the resolution levels disagree on the voxel type",
            ));
        }
        extent = Some(match extent {
            None => Extent::new(page.width, page.height, planes.depth()),
            Some(held) if held.x() == page.width && held.y() == page.height => held,
            Some(_) => {
                return Err(Error::metadata(
                    path,
                    "the pages of one resolution level disagree on their size",
                ));
            }
        });
        slots.push(*slot);
    }
    let Some(extent) = extent else {
        return Err(Error::metadata(path, "the OME-TIFF series holds no planes"));
    };
    Ok(LevelPages {
        extent,
        planes: planes.clone(),
        slots,
        format,
    })
}

fn counts(
    ome: Option<&OmeImage>,
    pages: usize,
    samples: u32,
    chunky: bool,
    path: &Path,
) -> Result<[u32; 3], Error> {
    let Some(ome) = ome else {
        let depth = u32::try_from(pages)
            .map_err(|_| Error::metadata(path, "the TIFF stacks more pages than a volume holds"))?;
        return Ok([1, if chunky { samples } else { 1 }, depth.max(1)]);
    };
    let size_c = ome.size_c.unwrap_or(1);
    let size_t = ome.size_t.unwrap_or(1);
    if size_c == 0 || size_t == 0 {
        return Err(Error::metadata(
            path,
            "the OME metadata declares a zero length channel or time axis",
        ));
    }
    if chunky && samples != size_c {
        return Err(Error::metadata(
            path,
            format!(
                "the TIFF pages carry {samples} samples but the image describes {size_c} channels"
            ),
        ));
    }
    Ok([size_t, size_c, ome.size_z.unwrap_or(1)])
}

fn spacing(ome: Option<&OmeImage>, volume: &File, path: &Path) -> Result<Spacing, Error> {
    let physical = ome.map(|ome| ome.physical).unwrap_or([None; 3]);
    let x = match physical[0] {
        Some(value) => value,
        None => resolution(volume, TAG_X_RESOLUTION, path)?.unwrap_or(1.0),
    };
    let y = match physical[1] {
        Some(value) => value,
        None => resolution(volume, TAG_Y_RESOLUTION, path)?.unwrap_or(x),
    };
    let z = physical[2].unwrap_or(x);
    Ok(Spacing::new(x, y, z))
}

fn placement(
    ome: Option<&OmeImage>,
    level: grana_volume::Level,
    depth: u32,
    path: &Path,
) -> Result<[f32; 3], Error> {
    let Some(ome) = ome else {
        return Ok([0.0; 3]);
    };
    let plane = ome
        .planes
        .iter()
        .filter(|plane| plane.position.iter().any(Option::is_some))
        .min_by_key(|plane| (plane.t, plane.c, plane.z));
    let Some(plane) = plane else {
        return Ok([0.0; 3]);
    };
    if plane.z >= depth {
        return Err(Error::metadata(
            path,
            "the OME plane position lies outside the declared depth",
        ));
    }
    let step = level.basis().column(2)[2];
    let reach = 0.5 * (f64::from(depth) - 1.0 - f64::from(plane.z)) * f64::from(step);
    let mut placed = [0.0f32; 3];
    for (axis, (slot, position)) in placed.iter_mut().zip(plane.position).enumerate() {
        let Some(position) = position else {
            continue;
        };
        let value = f64::from(position) + if axis == 2 { reach } else { 0.0 };
        if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
            return Err(Error::metadata(
                path,
                "the OME plane position is outside the supported range",
            ));
        }
        *slot = value as f32;
    }
    Ok(placed)
}

fn resolution(volume: &File, tag: u16, path: &Path) -> Result<Option<f32>, Error> {
    let directory = volume.directory()?;
    let Some(value) = directory
        .rational(&volume.store, tag, path)?
        .filter(|value| *value > 0.0)
    else {
        return Ok(None);
    };
    let unit = directory
        .integer(&volume.store, TAG_RESOLUTION_UNIT, path)?
        .unwrap_or(2);
    let micrometres = match unit {
        2 => 25_400.0,
        3 => 10_000.0,
        _ => return Ok(None),
    };
    Ok(Some((micrometres / value) as f32))
}

fn metadata(volume: &File, path: &Path) -> Result<Vec<OmeImage>, Error> {
    let description = volume
        .directory()?
        .ascii(&volume.store, TAG_IMAGE_DESCRIPTION, path)?;
    parse_metadata(description.as_deref(), path)
}

fn parse_metadata(description: Option<&str>, path: &Path) -> Result<Vec<OmeImage>, Error> {
    let Some(text) = description else {
        return Ok(Vec::new());
    };
    if !text.trim_start().starts_with('<') {
        return Ok(Vec::new());
    }
    grana_ome::parse(text).map_err(|detail| Error::metadata(path, detail))
}

pub fn series_count(path: &Path) -> Result<usize, Error> {
    Ok(series_names(path)?.len().max(1))
}

pub fn series_names(path: &Path) -> Result<Vec<Option<String>>, Error> {
    let store = Store::open(path)?;
    let header = tag::header(&store, path)?;
    let directory = Directory::read(&store, &header, header.first, path)?;
    let description = directory.ascii(&store, TAG_IMAGE_DESCRIPTION, path)?;
    Ok(parse_metadata(description.as_deref(), path)?
        .into_iter()
        .map(|ome| ome.name)
        .collect())
}

fn declared(images: &[OmeImage], page: &Page, stacked: usize, chunky: bool) -> Vec<Series> {
    if images.is_empty() {
        let depth = u32::try_from(stacked).unwrap_or(u32::MAX).max(1);
        let channels = if chunky {
            usize::try_from(page.samples).unwrap_or(1)
        } else {
            1
        };
        return vec![Series::new(
            Extent::new(page.width, page.height, depth),
            channels,
            1,
            None,
        )];
    }
    images
        .iter()
        .map(|ome| {
            Series::new(
                Extent::new(
                    ome.size_x.filter(|value| *value > 0).unwrap_or(page.width),
                    ome.size_y.filter(|value| *value > 0).unwrap_or(page.height),
                    ome.size_z.filter(|value| *value > 0).unwrap_or(1),
                ),
                usize::try_from(ome.size_c.filter(|value| *value > 0).unwrap_or(1)).unwrap_or(1),
                ome.size_t.filter(|value| *value > 0).unwrap_or(1),
                ome.name.clone().filter(|name| !name.is_empty()),
            )
        })
        .collect()
}

fn region(
    files: &[Arc<File>],
    cache: &Cache,
    levels: &[LevelPages],
    request: &Request,
) -> Result<VoxelData, Error> {
    let Some(level) = levels.get(request.level) else {
        return Err(Error::Region {
            path: PathBuf::new(),
            detail: "the requested resolution level does not exist".to_string(),
        });
    };
    let extent = level.extent;
    if request.time >= level.planes.frames() {
        return Err(Error::Region {
            path: PathBuf::new(),
            detail: "the requested frame does not exist".to_string(),
        });
    }
    if request.channel >= level.planes.channels() {
        return Err(Error::Region {
            path: PathBuf::new(),
            detail: "the requested channel does not exist".to_string(),
        });
    }
    if request.x.start >= request.x.end
        || request.x.end > extent.x()
        || request.y.start >= request.y.end
        || request.y.end > extent.y()
        || request.z.start >= request.z.end
        || request.z.end > extent.z()
    {
        return Err(Error::Region {
            path: PathBuf::new(),
            detail: "the region lies outside the image extent".to_string(),
        });
    }
    let width = request.x.end - request.x.start;
    let height = request.y.end - request.y.start;
    let depth = request.z.end - request.z.start;
    let count = u64::from(width) * u64::from(height) * u64::from(depth);
    let count = usize::try_from(count).map_err(|_| Error::Region {
        path: PathBuf::new(),
        detail: "the region holds more voxels than this platform addresses".to_string(),
    })?;
    let stride =
        usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| Error::Region {
            path: PathBuf::new(),
            detail: "the plane holds more voxels than this platform addresses".to_string(),
        })?;
    let mut data = match level.format {
        VoxelFormat::U8 => VoxelData::U8(vec![0; count]),
        VoxelFormat::U16 => VoxelData::U16(vec![0; count]),
        VoxelFormat::F32 => VoxelData::F32(vec![0.0; count]),
    };
    let region = Region {
        x: &request.x,
        y: &request.y,
    };
    for z in request.z.clone() {
        let raster = level.planes.index(request.time, request.channel, z);
        let slot = *level.slots.get(raster).ok_or_else(|| Error::Region {
            path: PathBuf::new(),
            detail: "the series holds no such plane".to_string(),
        })?;
        let volume = &files[slot.file];
        let page = volume.page(slot.page, request.level)?;
        let layout = volume.layout(slot.page, request.level)?;
        let source = PageSource {
            store: &volume.store,
            cache,
            path: &volume.path,
            file: slot.file,
        };
        let plane = Plane {
            layout: &layout,
            source: &source,
            level: request.level,
            page: slot.page,
            sample: if page.planar {
                0
            } else {
                request.channel.min(page.samples.saturating_sub(1))
            },
            region: &region,
        };
        let offset = ((z - request.z.start) as usize) * stride;
        match &mut data {
            VoxelData::U8(values) => {
                page.read_plane(&plane, &mut values[offset..offset + stride])?
            }
            VoxelData::U16(values) => {
                page.read_plane(&plane, &mut values[offset..offset + stride])?
            }
            VoxelData::F32(values) => {
                page.read_plane(&plane, &mut values[offset..offset + stride])?
            }
        }
    }
    Ok(data)
}

fn sample_range(
    files: &[Arc<File>],
    levels: &[LevelPages],
    cache: &Cache,
    channel: u32,
    frames: u32,
) -> Result<Option<Intensity>, Error> {
    let Some(level) = levels.last() else {
        return Ok(None);
    };
    let index = levels.len() - 1;
    let mut probe = Probe::of(level.extent, frames);
    probe.measure(|time, ranges| {
        region(
            files,
            cache,
            levels,
            &Request {
                time,
                level: index,
                channel,
                x: ranges[0].clone(),
                y: ranges[1].clone(),
                z: ranges[2].clone(),
            },
        )
    })
}

pub fn dataset_name(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return "OME-TIFF".to_string();
    };
    for suffix in [".ome.tiff", ".ome.tif", ".tiff", ".tif"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return stem.to_string();
        }
    }
    name.to_string()
}
