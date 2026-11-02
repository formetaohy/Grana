use std::collections::HashMap;
use std::path::Path;

use grana_volume::{Extent, Spacing};

use crate::Error;
use crate::directory::{Dimension, Entry, PixelType};
use crate::metadata::Metadata;
use crate::plane::Tile;

pub(crate) type PlaneKey = (u32, u32, u32);

pub(crate) struct Plan {
    tiles: Vec<Tile>,
}

impl Plan {
    pub(crate) fn tiles(&self) -> &[Tile] {
        &self.tiles
    }
}

pub(crate) struct Layout {
    pub(crate) extent: Extent,
    pub(crate) pixels: Vec<PixelType>,
    pub(crate) frames: u32,
    pub(crate) spacing: Spacing,
    pub(crate) interval: f32,
    pub(crate) plans: HashMap<PlaneKey, Plan>,
}

impl Layout {
    pub(crate) fn of(entries: Vec<Entry>, metadata: &Metadata, path: &Path) -> Result<Self, Error> {
        let mut selected = Vec::with_capacity(entries.len());
        for entry in &entries {
            block(entry, path)?;
            if entry.file_part != 0 {
                return Err(Error::unsupported(
                    path,
                    "the CZI image is split over several files",
                ));
            }
            if entry.start(Dimension::Scene) > 0 || !entry.is_full_resolution() {
                continue;
            }
            axes(entry, path)?;
            selected.push(entry);
        }
        if selected.is_empty() {
            return Err(Error::metadata(path, "the CZI image holds no pixels"));
        }
        let mut low = [i32::MAX; 2];
        let mut high = [i32::MIN; 2];
        let mut channels = 0;
        let mut frames = 0;
        let mut depth = 0;
        for entry in &selected {
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
        let mut plans: HashMap<PlaneKey, Plan> = HashMap::new();
        for entry in selected {
            let key = (
                entry.start(Dimension::T) as u32,
                entry.start(Dimension::C) as u32,
                entry.start(Dimension::Z) as u32,
            );
            match &pixels[key.1 as usize] {
                Some(known) if *known != entry.pixel => {
                    return Err(Error::unsupported(
                        path,
                        "the CZI channels disagree on their pixel type",
                    ));
                }
                Some(_) => {}
                None => pixels[key.1 as usize] = Some(entry.pixel),
            }
            let tile = Tile {
                position: entry.position,
                x: (entry.start(Dimension::X) - low[0]) as u32,
                y: (entry.start(Dimension::Y) - low[1]) as u32,
                width: entry.size(Dimension::X),
                height: entry.size(Dimension::Y),
                mosaic: entry.start(Dimension::Mosaic),
                pixel: entry.pixel,
                compression: entry.compression,
            };
            plans
                .entry(key)
                .or_insert(Plan { tiles: Vec::new() })
                .tiles
                .push(tile);
        }
        for plan in plans.values_mut() {
            plan.tiles.sort_by_key(|tile| tile.mosaic);
        }
        for channel in 0..channels {
            for time in 0..frames {
                for plane in 0..depth {
                    if !plans.contains_key(&(time, channel, plane)) {
                        return Err(Error::metadata(
                            path,
                            format!(
                                "the CZI image has no subblock for channel {channel}, frame {time}, plane {plane}"
                            ),
                        ));
                    }
                }
            }
        }
        let fallback = metadata
            .spacing
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(1.0);
        let spacing = metadata.spacing.map(|value| value.unwrap_or(fallback));
        Ok(Self {
            extent,
            pixels: pixels
                .into_iter()
                .map(|pixel| pixel.expect("every channel holds a pixel type"))
                .collect(),
            frames,
            spacing: Spacing::new(spacing[0], spacing[1], spacing[2]),
            interval: metadata.interval.unwrap_or(1.0),
            plans,
        })
    }
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
