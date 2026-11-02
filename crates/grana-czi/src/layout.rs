use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use grana_volume::{Extent, Spacing};

use crate::Error;
use crate::directory::{Dimension, Entry, PixelType};
use crate::metadata::Metadata;
use crate::plane::Tile;

const MINIFICATION_FLOOR: f64 = 1.5;
const MINIFICATION_CEILING: f64 = 1.0e6;

pub(crate) type PlaneKey = (u32, u32, u32);

pub(crate) struct Plan {
    tiles: Vec<Tile>,
}

impl Plan {
    pub(crate) fn tiles(&self) -> &[Tile] {
        &self.tiles
    }
}

pub(crate) struct LevelLayout {
    pub(crate) extent: Extent,
    pub(crate) spacing: Spacing,
    pub(crate) plans: HashMap<PlaneKey, Plan>,
}

pub(crate) struct Layout {
    pub(crate) pixels: Vec<PixelType>,
    pub(crate) frames: u32,
    pub(crate) interval: f32,
    pub(crate) levels: Vec<LevelLayout>,
}

impl Layout {
    pub(crate) fn of(entries: Vec<Entry>, metadata: &Metadata, path: &Path) -> Result<Self, Error> {
        let mut groups: BTreeMap<u32, Vec<Entry>> = BTreeMap::new();
        for entry in entries {
            block(&entry, path)?;
            if entry.file_part != 0 {
                return Err(Error::unsupported(
                    path,
                    "the CZI image is split over several files",
                ));
            }
            if entry.start(Dimension::Scene) > 0 {
                continue;
            }
            axes(&entry, path)?;
            if let Some(scale) = minification(&entry) {
                groups.entry(scale).or_default().push(entry);
            }
        }
        if groups.is_empty() {
            return Err(Error::metadata(path, "the CZI image holds no pixels"));
        }
        let base = groups.remove(&1).ok_or_else(|| {
            Error::metadata(path, "the CZI image holds no full resolution subblocks")
        })?;
        let mut low = [i32::MAX; 2];
        let mut high = [i32::MIN; 2];
        let mut channels = 0;
        let mut frames = 0;
        let mut depth = 0;
        for entry in &base {
            let starts = [entry.start(Dimension::X), entry.start(Dimension::Y)];
            let sizes = [entry.size(Dimension::X), entry.size(Dimension::Y)];
            for axis in 0..2 {
                low[axis] = low[axis].min(starts[axis]);
                high[axis] =
                    high[axis].max((i64::from(starts[axis]) + i64::from(sizes[axis])) as i32);
            }
            channels = channels.max(entry.start(Dimension::C) as u32 + 1);
            frames = frames.max(entry.start(Dimension::T) as u32 + 1);
            depth = depth.max(entry.start(Dimension::Z) as u32 + 1);
        }
        let extent = Extent::new((high[0] - low[0]) as u32, (high[1] - low[1]) as u32, depth);
        if metadata.size_x.is_some_and(|size| size != extent.x())
            || metadata.size_y.is_some_and(|size| size != extent.y())
        {
            return Err(Error::metadata(
                path,
                "the CZI metadata disagrees with the subblock extent",
            ));
        }
        if metadata.size_c.is_some_and(|count| count != channels)
            || metadata.size_t.is_some_and(|count| count != frames)
            || metadata.size_z.is_some_and(|count| count != depth)
        {
            return Err(Error::metadata(
                path,
                "the CZI metadata disagrees with the subblock axis lengths",
            ));
        }
        let mut pixels = vec![None; channels as usize];
        for entry in &base {
            let channel = entry.start(Dimension::C) as usize;
            match &pixels[channel] {
                Some(known) if *known != entry.pixel => {
                    return Err(Error::unsupported(
                        path,
                        "the CZI channels disagree on their pixel type",
                    ));
                }
                Some(_) => {}
                None => pixels[channel] = Some(entry.pixel),
            }
        }
        let pixels: Vec<PixelType> = pixels
            .into_iter()
            .enumerate()
            .map(|(channel, pixel)| {
                pixel.ok_or_else(|| {
                    Error::metadata(
                        path,
                        format!("the CZI image has no subblock for channel {channel}"),
                    )
                })
            })
            .collect::<Result<_, Error>>()?;
        let fallback = metadata
            .spacing
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(1.0);
        let spacing = metadata.spacing.map(|value| value.unwrap_or(fallback));
        let context = Context {
            path,
            low,
            logical_extent: [extent.x(), extent.y()],
            frames,
            channels,
            base_depth: depth,
            spacing: Spacing::new(spacing[0], spacing[1], spacing[2]),
            pixels: &pixels,
        };
        let mut levels = vec![assemble(&context, &base, 1)?];
        for (scale, entries) in groups {
            let Ok(level) = assemble(&context, &entries, scale) else {
                continue;
            };
            let previous = levels.last().expect("the base level is present");
            if level.extent.x() > previous.extent.x()
                || level.extent.y() > previous.extent.y()
                || level.extent.z() > previous.extent.z()
                || level.spacing.x() < previous.spacing.x()
                || level.spacing.y() < previous.spacing.y()
                || level.spacing.z() < previous.spacing.z()
            {
                continue;
            }
            levels.push(level);
        }
        Ok(Self {
            pixels,
            frames,
            interval: metadata.interval.unwrap_or(1.0),
            levels,
        })
    }
}

struct Context<'a> {
    path: &'a Path,
    low: [i32; 2],
    logical_extent: [u32; 2],
    frames: u32,
    channels: u32,
    base_depth: u32,
    spacing: Spacing,
    pixels: &'a [PixelType],
}

fn assemble(context: &Context<'_>, entries: &[Entry], scale: u32) -> Result<LevelLayout, Error> {
    let mut plans: HashMap<PlaneKey, Plan> = HashMap::new();
    let mut high = [0; 2];
    let mut depth = 0;
    for entry in entries {
        let time = entry.start(Dimension::T) as u32;
        let channel = entry.start(Dimension::C) as u32;
        let plane = entry.start(Dimension::Z) as u32;
        if time >= context.frames || channel >= context.channels {
            return Err(Error::metadata(
                context.path,
                "a resolution level exceeds the base plane layout",
            ));
        }
        if context.pixels[channel as usize] != entry.pixel {
            return Err(Error::unsupported(
                context.path,
                "the resolution levels disagree on the pixel type",
            ));
        }
        let position = [
            position(context, entry.start(Dimension::X), context.low[0], scale)?,
            position(context, entry.start(Dimension::Y), context.low[1], scale)?,
        ];
        let size = [entry.stored(Dimension::X), entry.stored(Dimension::Y)];
        for axis in 0..2 {
            let end = position[axis]
                .checked_add(size[axis])
                .ok_or_else(|| Error::metadata(context.path, "a resolution level is too large"))?;
            high[axis] = high[axis].max(end);
        }
        depth = depth.max(
            plane
                .checked_add(1)
                .ok_or_else(|| Error::metadata(context.path, "a resolution level is too deep"))?,
        );
        let tile = Tile {
            position: entry.position,
            x: position[0],
            y: position[1],
            width: size[0],
            height: size[1],
            mosaic: entry.start(Dimension::Mosaic),
            pixel: entry.pixel,
            compression: entry.compression,
        };
        plans
            .entry((time, channel, plane))
            .or_insert(Plan { tiles: Vec::new() })
            .tiles
            .push(tile);
    }
    if high[0] == 0 || high[1] == 0 || depth == 0 {
        return Err(Error::metadata(
            context.path,
            "a resolution level holds no pixels",
        ));
    }
    for plan in plans.values_mut() {
        plan.tiles.sort_by_key(|tile| tile.mosaic);
    }
    for time in 0..context.frames {
        for channel in 0..context.channels {
            for plane in 0..depth {
                if !plans.contains_key(&(time, channel, plane)) {
                    return Err(Error::metadata(
                        context.path,
                        format!(
                            "the CZI image has no subblock for channel {channel}, frame {time}, plane {plane}"
                        ),
                    ));
                }
            }
        }
    }
    let extent = Extent::new(high[0], high[1], depth);
    let spacing = Spacing::new(
        context.spacing.x() * context.logical_extent[0] as f32 / extent.x() as f32,
        context.spacing.y() * context.logical_extent[1] as f32 / extent.y() as f32,
        context.spacing.z() * context.base_depth as f32 / extent.z() as f32,
    );
    Ok(LevelLayout {
        extent,
        spacing,
        plans,
    })
}

fn position(context: &Context<'_>, start: i32, low: i32, scale: u32) -> Result<u32, Error> {
    let offset = i64::from(start) - i64::from(low);
    if offset < 0 {
        return Err(Error::metadata(
            context.path,
            "a resolution level lies before the base bounding box",
        ));
    }
    u32::try_from(offset / i64::from(scale))
        .map_err(|_| Error::metadata(context.path, "a resolution level is too large"))
}

fn minification(entry: &Entry) -> Option<u32> {
    if entry.is_full_resolution() {
        return Some(1);
    }
    let stored = [entry.stored(Dimension::X), entry.stored(Dimension::Y)];
    let size = [entry.size(Dimension::X), entry.size(Dimension::Y)];
    let factor = if stored[0] > stored[1] {
        f64::from(size[0]) / f64::from(stored[0])
    } else {
        f64::from(size[1]) / f64::from(stored[1])
    };
    if !(MINIFICATION_FLOOR..=MINIFICATION_CEILING).contains(&factor) {
        return None;
    }
    u32::try_from(factor.round() as u64).ok()
}

fn axes(entry: &Entry, path: &Path) -> Result<(), Error> {
    for dimension in [Dimension::Z, Dimension::C, Dimension::T] {
        if entry.size(dimension) != 1 {
            return Err(Error::unsupported(
                path,
                format!(
                    "a CZI subblock holds {} planes along {}",
                    entry.size(dimension),
                    dimension.label()
                ),
            ));
        }
        if entry.start(dimension) < 0 {
            return Err(Error::metadata(
                path,
                format!(
                    "a CZI subblock starts before the image along {}",
                    dimension.label()
                ),
            ));
        }
    }
    for axis in &entry.axes {
        if let Dimension::Other(code) = axis.dimension
            && axis.size > 1
        {
            return Err(Error::unsupported(
                path,
                format!("the CZI dimension `{}` is not supported", char::from(code)),
            ));
        }
    }
    Ok(())
}

fn block(entry: &Entry, path: &Path) -> Result<(), Error> {
    if entry.size(Dimension::Block) > 1 || entry.start(Dimension::Block) > 0 {
        return Err(Error::unsupported(
            path,
            "the CZI image is split into several acquisition blocks",
        ));
    }
    Ok(())
}
