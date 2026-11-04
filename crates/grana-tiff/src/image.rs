use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grana_volume::{
    BrickCoord, BrickData, BrickGrid, ChannelInfo, Extent, Intensity, Level, Pyramid, Spacing,
    VoxelData, VoxelFormat, pad_brick,
};

use crate::cache::Cache;
use crate::error::Error;
use crate::ome::{self, Ome};
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
    store: Store,
    cache: Mutex<Cache>,
}

struct LevelPages {
    extent: Extent,
    planes: Planes,
    pages: Vec<Page>,
}

struct Request {
    time: u32,
    level: usize,
    channel: u32,
    x: Range<u32>,
    y: Range<u32>,
    z: Range<u32>,
}

impl Image {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let store = Store::open(path)?;
        let header = tag::header(&store, path)?;
        let chain = chain(&store, &header, path)?;
        let description = chain[0].ascii(&store, TAG_IMAGE_DESCRIPTION, path)?;
        let ome = parse_metadata(description.as_deref(), path)?;
        let level_zero = chain
            .iter()
            .map(|directory| Page::new(directory, &store, header.big_endian, path))
            .collect::<Result<Vec<_>, Error>>()?;
        let first = &level_zero[0];
        let chunky = !first.planar;
        let format = first.format;
        let samples = first.samples;
        validate_pages(&level_zero, path)?;
        let (size_c, size_t) = match &ome {
            Some(ome) => ome_counts(ome, first, path)?,
            None => (if chunky { samples } else { 1 }, 1),
        };
        check_samples(chunky, samples, size_c, path)?;
        let per_frame = u64::from(if chunky { 1 } else { size_c });
        let page_count = level_zero.len() as u64;
        let divisor = u64::from(size_t) * per_frame;
        if divisor == 0 || !page_count.is_multiple_of(divisor) {
            return Err(Error::metadata(
                path,
                format!(
                    "the image holds {page_count} planes for {size_t} frames and {size_c} channels"
                ),
            ));
        }
        let depth = page_count / divisor;
        if let Some(ome) = &ome
            && ome
                .size_z
                .is_some_and(|declared| u64::from(declared) != depth)
        {
            return Err(Error::metadata(
                path,
                "the OME metadata disagrees with the number of planes in the file",
            ));
        }
        let depth = u32::try_from(depth)
            .ok()
            .filter(|depth| *depth > 0)
            .ok_or_else(|| Error::unsupported(path, "the image holds too many planes"))?;
        validate_tiff_data(&ome, page_count, path)?;
        let order = ome
            .as_ref()
            .and_then(|ome| ome.dimension_order.clone())
            .unwrap_or_else(|| "XYZCT".to_string());
        let base_extent = Extent::new(first.width, first.height, depth);
        let base_spacing = spacing(&ome, &chain[0], &store, path)?;
        let groups = groups(&store, &header, level_zero, path)?;
        let mut pyramid_levels = Vec::with_capacity(groups.len());
        let mut levels = Vec::with_capacity(groups.len());
        for (index, group) in groups.into_iter().enumerate() {
            validate_pages(&group, path)?;
            let pages = group.len() as u64;
            if !pages.is_multiple_of(divisor) {
                return Err(Error::metadata(
                    path,
                    "a resolution level does not hold a whole number of planes",
                ));
            }
            let depth = u32::try_from(pages / divisor)
                .ok()
                .filter(|depth| *depth > 0)
                .ok_or_else(|| {
                    Error::unsupported(path, "a resolution level holds too many planes")
                })?;
            let page = &group[0];
            let extent = Extent::new(page.width, page.height, depth);
            let spacing = if index == 0 {
                base_spacing
            } else {
                Spacing::new(
                    base_spacing.x() * base_extent.x() as f32 / extent.x() as f32,
                    base_spacing.y() * base_extent.y() as f32 / extent.y() as f32,
                    base_spacing.z() * base_extent.z() as f32 / extent.z() as f32,
                )
            };
            let planes = Planes::new(depth, size_c, size_t, chunky, &order)
                .map_err(|detail| Error::metadata(path, detail))?;
            if planes.count() != pages {
                return Err(Error::metadata(
                    path,
                    "the plane table disagrees with the number of TIFF pages",
                ));
            }
            pyramid_levels.push(Level::new(extent, spacing));
            levels.push(LevelPages {
                extent,
                planes,
                pages: group,
            });
        }
        for pair in pyramid_levels.windows(2) {
            let (fine, coarse) = (pair[0], pair[1]);
            let (fine_extent, coarse_extent) = (fine.extent(), coarse.extent());
            let (fine_spacing, coarse_spacing) = (fine.spacing(), coarse.spacing());
            if fine_extent.x() < coarse_extent.x()
                || fine_extent.y() < coarse_extent.y()
                || fine_extent.z() < coarse_extent.z()
                || fine_spacing.x() > coarse_spacing.x()
                || fine_spacing.y() > coarse_spacing.y()
                || fine_spacing.z() > coarse_spacing.z()
            {
                return Err(Error::unsupported(
                    path,
                    "the resolution levels do not shrink from fine to coarse",
                ));
            }
        }
        let cache = Cache::new(CACHE_BYTES);
        let mut channels = Vec::with_capacity(size_c as usize);
        for index in 0..size_c {
            let metadata = ome
                .as_ref()
                .and_then(|ome| ome.channels.get(index as usize));
            let name = metadata
                .and_then(|channel| channel.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Channel {}", index + 1));
            let tint = metadata
                .and_then(|channel| channel.tint)
                .unwrap_or(PALETTE[index as usize % PALETTE.len()]);
            let sampled = sample_range(&levels, &store, &cache, path, index, size_t)?;
            let intensity = match format {
                VoxelFormat::U8 => Intensity::new(0.0, f32::from(u8::MAX)),
                VoxelFormat::U16 => Intensity::new(0.0, f32::from(u16::MAX)),
                VoxelFormat::F32 => sampled.unwrap_or_else(|| Intensity::new(0.0, 1.0)),
            };
            let window = sampled
                .filter(|sampled| sampled.span() > 0.0)
                .unwrap_or(intensity);
            let window = if window.span() > 0.0 {
                window
            } else {
                Intensity::new(0.0, 1.0)
            };
            channels.push(ChannelInfo::new(name, tint, intensity, window, format));
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
            pyramid: Pyramid::new(pyramid_levels, stored, channels, size_t, interval),
            levels,
            store,
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
            &self.levels,
            &self.store,
            &self.cache,
            &self.path,
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
        let Some(level_pages) = self.levels.get(level) else {
            return Err(Error::unsupported(
                &self.path,
                "the requested resolution level does not exist",
            ));
        };
        let grid = BrickGrid::new(level_pages.extent);
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
                level_pages.extent,
            ));
        }
        Ok(BrickData::new(channels))
    }
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

fn ome_counts(ome: &Ome, first: &Page, path: &Path) -> Result<(u32, u32), Error> {
    let size_x = ome
        .size_x
        .ok_or_else(|| Error::metadata(path, "the OME metadata has no SizeX"))?;
    let size_y = ome
        .size_y
        .ok_or_else(|| Error::metadata(path, "the OME metadata has no SizeY"))?;
    if size_x != first.width || size_y != first.height {
        return Err(Error::metadata(
            path,
            format!(
                "the OME metadata declares a {size_x}x{size_y} image but the TIFF page is {}x{}",
                first.width, first.height
            ),
        ));
    }
    let channels = ome.size_c.unwrap_or(1);
    let frames = ome.size_t.unwrap_or(1);
    if channels == 0 || frames == 0 {
        return Err(Error::metadata(
            path,
            "the OME metadata declares a zero length channel or time axis",
        ));
    }
    Ok((channels, frames))
}

fn validate_tiff_data(ome: &Option<Ome>, pages: u64, path: &Path) -> Result<(), Error> {
    let Some(ome) = ome else {
        return Ok(());
    };
    if ome.tiff_data.is_empty() {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    for block in &ome.tiff_data {
        if block.file.as_deref().is_some_and(|file| file != name) {
            return Err(Error::unsupported(
                path,
                "the image splits its planes over several TIFF files",
            ));
        }
    }
    let total = ome
        .tiff_data
        .iter()
        .map(|block| u64::from(block.planes))
        .sum::<u64>();
    if total != pages {
        return Err(Error::metadata(
            path,
            format!("the OME metadata describes {total} planes but the file holds {pages}"),
        ));
    }
    Ok(())
}

fn check_samples(chunky: bool, samples: u32, channels: u32, path: &Path) -> Result<(), Error> {
    if chunky && samples != channels {
        return Err(Error::metadata(
            path,
            format!(
                "the TIFF pages carry {samples} samples but the image describes {channels} channels"
            ),
        ));
    }
    Ok(())
}

fn validate_pages(pages: &[Page], path: &Path) -> Result<(), Error> {
    let first = &pages[0];
    for page in pages {
        if page.width != first.width
            || page.height != first.height
            || page.samples != first.samples
            || page.format != first.format
            || page.planar != first.planar
        {
            return Err(Error::unsupported(
                path,
                "the TIFF pages of one resolution level disagree on their geometry",
            ));
        }
    }
    Ok(())
}

fn groups(
    store: &Store,
    header: &tag::Header,
    level_zero: Vec<Page>,
    path: &Path,
) -> Result<Vec<Vec<Page>>, Error> {
    let sub_ifds = level_zero[0].sub_ifds.clone();
    let consistent = level_zero
        .iter()
        .all(|page| page.sub_ifds.len() == sub_ifds.len());
    if !consistent {
        return Err(Error::unsupported(
            path,
            "only some TIFF pages carry reduced resolution sub images",
        ));
    }
    if sub_ifds.is_empty() {
        let mut runs: Vec<Vec<Page>> = Vec::new();
        for page in level_zero {
            let fits = runs.last().is_some_and(|run: &Vec<Page>| {
                run[0].width == page.width && run[0].height == page.height
            });
            if fits {
                runs.last_mut().expect("the run is ready").push(page);
            } else {
                runs.push(vec![page]);
            }
        }
        return Ok(runs);
    }
    let mut levels = Vec::with_capacity(sub_ifds.len() + 1);
    for level in 0..sub_ifds.len() {
        let mut pages = Vec::with_capacity(level_zero.len());
        for page in &level_zero {
            let directory = Directory::read(store, header, page.sub_ifds[level], path)?;
            pages.push(Page::new(&directory, store, header.big_endian, path)?);
        }
        levels.push(pages);
    }
    let mut all = Vec::with_capacity(levels.len() + 1);
    all.push(level_zero);
    all.extend(levels);
    Ok(all)
}

fn spacing(
    ome: &Option<Ome>,
    directory: &Directory,
    store: &Store,
    path: &Path,
) -> Result<Spacing, Error> {
    let physical = ome.as_ref().map(|ome| ome.physical).unwrap_or([None; 3]);
    let x = match physical[0] {
        Some(value) => value,
        None => resolution(directory, store, TAG_X_RESOLUTION, path)?.unwrap_or(1.0),
    };
    let y = match physical[1] {
        Some(value) => value,
        None => resolution(directory, store, TAG_Y_RESOLUTION, path)?.unwrap_or(x),
    };
    let z = physical[2].unwrap_or(x);
    Ok(Spacing::new(x, y, z))
}

fn resolution(
    directory: &Directory,
    store: &Store,
    tag: u16,
    path: &Path,
) -> Result<Option<f32>, Error> {
    let Some(value) = directory.rational(store, tag, path)? else {
        return Ok(None);
    };
    if value <= 0.0 {
        return Ok(None);
    }
    let unit = directory
        .integer(store, TAG_RESOLUTION_UNIT, path)?
        .unwrap_or(2);
    let micrometres = match unit {
        2 => 25_400.0,
        3 => 10_000.0,
        _ => return Ok(None),
    };
    Ok(Some((micrometres / value) as f32))
}

fn region(
    levels: &[LevelPages],
    store: &Store,
    cache: &Mutex<Cache>,
    path: &Path,
    request: &Request,
) -> Result<VoxelData, Error> {
    let Some(level) = levels.get(request.level) else {
        return Err(Error::Region {
            path: path.to_path_buf(),
            detail: "the requested resolution level does not exist".to_string(),
        });
    };
    let extent = level.extent;
    if request.time >= level.planes.frames() {
        return Err(Error::Region {
            path: path.to_path_buf(),
            detail: "the requested frame does not exist".to_string(),
        });
    }
    if request.channel >= level.planes.channels() {
        return Err(Error::Region {
            path: path.to_path_buf(),
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
            path: path.to_path_buf(),
            detail: "the region lies outside the image extent".to_string(),
        });
    }
    let planar = level.pages[0].planar;
    let sample = if planar { 0 } else { request.channel };
    let width = request.x.end - request.x.start;
    let height = request.y.end - request.y.start;
    let depth = request.z.end - request.z.start;
    let count = u64::from(width) * u64::from(height) * u64::from(depth);
    let count = usize::try_from(count).map_err(|_| Error::Region {
        path: path.to_path_buf(),
        detail: "the region holds more voxels than this platform addresses".to_string(),
    })?;
    let plane = (width * height) as usize;
    let mut data = match level.pages[0].format {
        VoxelFormat::U8 => VoxelData::U8(vec![0; count]),
        VoxelFormat::U16 => VoxelData::U16(vec![0; count]),
        VoxelFormat::F32 => VoxelData::F32(vec![0.0; count]),
    };
    let region = Region {
        x: &request.x,
        y: &request.y,
    };
    let source = PageSource { store, cache, path };
    for z in request.z.clone() {
        let page_index = level.planes.index(request.time, request.channel, z);
        let Some(page) = level.pages.get(page_index) else {
            return Err(Error::Region {
                path: path.to_path_buf(),
                detail: "the plane lies outside the TIFF pages".to_string(),
            });
        };
        let offset = ((z - request.z.start) as usize) * plane;
        match &mut data {
            VoxelData::U8(values) => page.read_plane(
                &source,
                request.level,
                page_index,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
            VoxelData::U16(values) => page.read_plane(
                &source,
                request.level,
                page_index,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
            VoxelData::F32(values) => page.read_plane(
                &source,
                request.level,
                page_index,
                sample,
                &region,
                &mut values[offset..offset + plane],
            )?,
        }
    }
    Ok(data)
}

fn sample_range(
    levels: &[LevelPages],
    store: &Store,
    cache: &Mutex<Cache>,
    path: &Path,
    channel: u32,
    frames: u32,
) -> Result<Option<Intensity>, Error> {
    let level = levels.len() - 1;
    let extent = levels[level].extent;
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
        let data = region(
            levels,
            store,
            cache,
            path,
            &Request {
                time,
                level,
                channel,
                x: spatial.0.clone(),
                y: spatial.1.clone(),
                z: spatial.2.clone(),
            },
        )?;
        let range = data.range();
        low = low.min(range.min());
        high = high.max(range.max());
    }
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
