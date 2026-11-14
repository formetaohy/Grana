use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing,
    VoxelData, VoxelFormat, pad_brick,
};

use crate::cache::Cache;
use crate::error::Error;
use crate::ome::{self, Ome, TiffData};
use crate::page::{Page, Region, Source as PageSource};
use crate::planes::Planes;
use crate::store::Store;
use crate::tag::{
    self, Directory, TAG_IMAGE_DESCRIPTION, TAG_RESOLUTION_UNIT, TAG_X_RESOLUTION, TAG_Y_RESOLUTION,
};

const CACHE_BYTES: u64 = 128 << 20;
const PAGE_LIMIT: usize = 4096;
const RANGE_SAMPLE_VOXELS: u64 = 4 << 20;
const RANGE_PROBE_SIDE: u32 = 32;
const RANGE_SAMPLE_FRAMES: u32 = 4;
const PALETTE: [[f32; 3]; 8] = [
    [0.25, 0.55, 1.00],
    [0.20, 0.95, 0.35],
    [1.00, 0.25, 0.55],
    [1.00, 0.72, 0.20],
    [0.60, 0.40, 1.00],
    [0.20, 0.90, 0.90],
    [1.00, 0.45, 0.20],
    [0.85, 0.85, 0.30],
];

pub struct Image {
    path: PathBuf,
    name: String,
    pyramid: Pyramid,
    levels: Vec<LevelPages>,
    files: Vec<Arc<File>>,
    cache: Cache,
}

struct File {
    path: PathBuf,
    store: Store,
    header: tag::Header,
    directories: Vec<Directory>,
    pages: Vec<Page>,
}

struct LevelPages {
    extent: Extent,
    planes: Planes,
    slots: Vec<Slot>,
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
        let directories = chain(&store, &header, path)?;
        let pages = directories
            .iter()
            .map(|directory| Page::new(directory, &store, header.big_endian, path))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            path: path.to_path_buf(),
            store,
            header,
            directories,
            pages,
        })
    }

    fn sub(&self, page: usize, level: usize) -> Result<Page, Error> {
        if level == 0 {
            return self
                .pages
                .get(page)
                .cloned()
                .ok_or_else(|| Error::read(&self.path, "the TIFF page is missing"));
        }
        let offset = self
            .pages
            .get(page)
            .and_then(|page| page.sub_ifds.get(level - 1))
            .copied()
            .ok_or_else(|| {
                Error::metadata(
                    &self.path,
                    "a TIFF page carries no reduced resolution image",
                )
            })?;
        let directory = Directory::read(&self.store, &self.header, offset, &self.path)?;
        Page::new(&directory, &self.store, self.header.big_endian, &self.path)
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
    pub fn open(path: &Path) -> Result<Self, Error> {
        let first = File::open(path)?;
        let description = first.directories[0].ascii(&first.store, TAG_IMAGE_DESCRIPTION, path)?;
        let ome = parse_metadata(description.as_deref(), path)?;
        let page = &first.pages[0];
        let chunky = !page.planar && page.samples > 1;
        let format = page.format;
        let samples = page.samples;
        let sizes = counts(&ome, samples, chunky, path)?;
        let order = ome
            .as_ref()
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
            &ome,
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
                &ome,
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
        let spacing = spacing(&ome, &files[0], path)?;
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
        let cache = Cache::new(CACHE_BYTES);
        let mut channels = Vec::with_capacity(sizes[1] as usize);
        for index in 0..sizes[1] {
            let info = ome
                .as_ref()
                .and_then(|ome| ome.channels.get(index as usize));
            let name = info
                .and_then(|channel| channel.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Channel {}", index + 1));
            let tint = info
                .and_then(|channel| channel.tint)
                .unwrap_or(PALETTE[index as usize % PALETTE.len()]);
            let sampled = sample_range(&files, &levels, &cache, path, index, sizes[0])?;
            let full = match format {
                VoxelFormat::U8 => Intensity::new(0.0, f32::from(u8::MAX)),
                VoxelFormat::U16 => Intensity::new(0.0, f32::from(u16::MAX)),
                VoxelFormat::F32 => sampled.unwrap_or_else(|| Intensity::new(0.0, 1.0)),
            };
            let window = sampled.filter(|window| window.span() > 0.0).unwrap_or(full);
            let window = if window.span() > 0.0 {
                window
            } else {
                Intensity::new(0.0, 1.0)
            };
            channels.push(ChannelInfo::new(name, tint, full, window, format));
        }
        let interval = ome.as_ref().and_then(|ome| ome.interval).unwrap_or(1.0);
        let name = ome
            .as_ref()
            .and_then(|ome| ome.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| dataset_name(path));
        let stored = pyramid_levels.len();
        Ok(Self {
            path: path.to_path_buf(),
            name,
            pyramid: Pyramid::new(pyramid_levels, stored, channels, sizes[0], interval),
            levels,
            files,
            cache,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
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

    pub fn read_brick(
        &self,
        time: u32,
        level: usize,
        coord: BrickCoord,
    ) -> Result<BrickData, Error> {
        let Some(pages) = self.levels.get(level) else {
            return Err(Error::unsupported(
                &self.path,
                "the requested resolution level does not exist",
            ));
        };
        let grid = BrickGrid::new(pages.extent);
        let spatial = grid.padded_range(coord);
        let origin = [spatial[0].start, spatial[1].start, spatial[2].start];
        let extent = Extent::new(
            spatial[0].end - spatial[0].start,
            spatial[1].end - spatial[1].start,
            spatial[2].end - spatial[2].start,
        );
        let mut channels = Vec::with_capacity(self.pyramid.channel_count());
        for channel in 0..self.pyramid.channel_count() {
            let data = self.read_region(
                time,
                level,
                channel,
                spatial[0].clone(),
                spatial[1].clone(),
                spatial[2].clone(),
            )?;
            channels.push(pad_brick(
                &data,
                origin,
                extent,
                grid.origin(coord),
                pages.extent,
            ));
        }
        Ok(BrickData::new(channels))
    }
}

fn assign(
    file: usize,
    volume: &File,
    ome: &Option<Ome>,
    planes: &Planes,
    format: VoxelFormat,
    placed: &mut HashMap<usize, Slot>,
    requested: &mut Vec<String>,
) -> Result<(), Error> {
    let path = &volume.path;
    let blocks = ome
        .as_ref()
        .map(|ome| ome.tiff_data.as_slice())
        .unwrap_or_default();
    let mut raster: Vec<(usize, usize)> = Vec::new();
    for block in blocks {
        if let Some(name) = block.file.as_deref()
            && path.file_name().and_then(|held| held.to_str()) != Some(name)
        {
            if !requested.iter().any(|held| held == name) {
                requested.push(name.to_string());
            }
            continue;
        }
        place(block, planes, &mut raster, path)?;
    }
    if blocks.is_empty() {
        let limit = volume.pages.len().min(planes.count() as usize);
        raster.extend((0..limit).map(|page| (page, page)));
    }
    for (index, page) in raster {
        let held = volume.pages.get(page).ok_or_else(|| {
            Error::metadata(path, "the OME-TIFF metadata names a missing TIFF page")
        })?;
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
    let mut coordinate = [block.z, block.c, block.t];
    for offset in 0..block.planes {
        if coordinate[0] >= planes.depth()
            || coordinate[1] >= planes.channels()
            || coordinate[2] >= planes.frames()
        {
            return Err(Error::metadata(
                path,
                "the OME-TIFF TiffData runs past the declared pixels",
            ));
        }
        let index = planes.index(coordinate[2], coordinate[1], coordinate[0]);
        raster.push((index, block.ifd as usize + offset as usize));
        coordinate[0] += 1;
        if coordinate[0] == planes.depth() {
            coordinate[0] = 0;
            coordinate[1] += 1;
            if coordinate[1] == planes.channels() {
                coordinate[1] = 0;
                coordinate[2] += 1;
            }
        }
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
        let page = files[slot.file].sub(slot.page, level)?;
        if page.format != format {
            return Err(Error::unsupported(
                path,
                "the resolution levels disagree on the voxel type",
            ));
        }
        extent = Some(match extent {
            None => Extent::new(page.width, page.height, planes.depth()),
            Some(held) => Extent::new(
                held.x().max(page.width),
                held.y().max(page.height),
                held.z(),
            ),
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
    })
}

fn counts(ome: &Option<Ome>, samples: u32, chunky: bool, path: &Path) -> Result<[u32; 3], Error> {
    let Some(ome) = ome else {
        return Ok([1, if chunky { samples } else { 1 }, 1]);
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

fn spacing(ome: &Option<Ome>, volume: &File, path: &Path) -> Result<Spacing, Error> {
    let physical = ome.as_ref().map(|ome| ome.physical).unwrap_or([None; 3]);
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

fn resolution(volume: &File, tag: u16, path: &Path) -> Result<Option<f32>, Error> {
    let directory = volume
        .directories
        .first()
        .ok_or_else(|| Error::metadata(path, "the file holds no TIFF directory"))?;
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

fn chain(store: &Store, header: &tag::Header, path: &Path) -> Result<Vec<Directory>, Error> {
    let mut directories = Vec::new();
    let mut offset = header.first;
    while offset != 0 {
        if directories.len() >= PAGE_LIMIT {
            return Err(Error::unsupported(
                path,
                "the image holds more TIFF pages than grana reads",
            ));
        }
        let directory = Directory::read(store, header, offset, path)?;
        offset = directory.next;
        directories.push(directory);
    }
    if directories.is_empty() {
        return Err(Error::unsupported(path, "the file holds no TIFF page"));
    }
    Ok(directories)
}

fn parse_metadata(description: Option<&str>, path: &Path) -> Result<Option<Ome>, Error> {
    let Some(text) = description else {
        return Ok(None);
    };
    if !text.trim_start().starts_with('<') {
        return Ok(None);
    }
    ome::parse(text).map_err(|detail| Error::metadata(path, detail))
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
    let plane =
        usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| Error::Region {
            path: PathBuf::new(),
            detail: "the plane holds more voxels than this platform addresses".to_string(),
        })?;
    let mut data = match levels[request.level]
        .slots
        .first()
        .and_then(|slot| files[slot.file].sub(slot.page, request.level).ok())
        .map(|page| page.format)
    {
        Some(VoxelFormat::U8) => VoxelData::U8(vec![0; count]),
        Some(VoxelFormat::U16) => VoxelData::U16(vec![0; count]),
        Some(VoxelFormat::F32) => VoxelData::F32(vec![0.0; count]),
        None => {
            return Err(Error::Region {
                path: PathBuf::new(),
                detail: "the series holds no plane".to_string(),
            });
        }
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
        let page = volume.sub(slot.page, request.level)?;
        let sample = if page.planar {
            0
        } else {
            request.channel.min(page.samples.saturating_sub(1))
        };
        let source = PageSource {
            store: &volume.store,
            cache,
            path: &volume.path,
            file: slot.file,
        };
        let offset = ((z - request.z.start) as usize) * plane;
        match &mut data {
            VoxelData::U8(values) => page.read_plane(
                &source,
                request.level,
                slot.page,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
            VoxelData::U16(values) => page.read_plane(
                &source,
                request.level,
                slot.page,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
            VoxelData::F32(values) => page.read_plane(
                &source,
                request.level,
                slot.page,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
        }
    }
    Ok(data)
}

fn sample_range(
    files: &[Arc<File>],
    levels: &[LevelPages],
    cache: &Cache,
    path: &Path,
    channel: u32,
    frames: u32,
) -> Result<Option<Intensity>, Error> {
    let Some(level) = levels.last() else {
        return Ok(None);
    };
    let index = levels.len() - 1;
    let extent = level.extent;
    let samples = frames.clamp(1, RANGE_SAMPLE_FRAMES);
    let budget = (RANGE_SAMPLE_VOXELS / u64::from(samples)).max(1);
    let probe = |limit: u32| {
        let side = limit.min(RANGE_PROBE_SIDE);
        let start = (limit - side) / 2;
        start..start + side
    };
    let spatial = if extent.voxel_count() <= budget {
        (0..extent.x(), 0..extent.y(), 0..extent.z())
    } else {
        (probe(extent.x()), probe(extent.y()), probe(extent.z()))
    };
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for sample in 0..samples {
        let time = sample * frames / samples;
        let data = match region(
            files,
            cache,
            levels,
            &Request {
                time,
                level: index,
                channel,
                x: spatial.0.clone(),
                y: spatial.1.clone(),
                z: spatial.2.clone(),
            },
        ) {
            Ok(data) => data,
            Err(_) => return Ok(None),
        };
        let range = data.range();
        low = low.min(range.min());
        high = high.max(range.max());
    }
    let _ = path;
    if high > low {
        Ok(Some(Intensity::new(low, high)))
    } else {
        Ok(None)
    }
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
