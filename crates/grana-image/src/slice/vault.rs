use std::ops::Range;
use std::sync::Arc;

use grana_store::{LruCache, Pool};
use grana_volume::{Extent, NoData, Pyramid, VoxelData};
use rayon::prelude::*;

use crate::Error;
use crate::reader::Reader;
use crate::region;
use crate::slice::store::{Shape, Store};
use crate::slice::{Key, Plane, mip};

pub(crate) struct Vault {
    reader: Reader,
    namespace: u64,
    store: Option<Store>,
    planes: LruCache<Key, Plane>,
}

impl Vault {
    pub(crate) fn new(reader: Reader, namespace: u64, store: Option<Store>) -> Self {
        Self {
            reader,
            namespace,
            store,
            planes: LruCache::pooled(Pool::Plane, Plane::bytes),
        }
    }

    pub(crate) fn reader(&self) -> &Reader {
        &self.reader
    }

    pub(crate) fn window(
        &self,
        pyramid: &Pyramid,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        fill: NoData,
    ) -> Result<VoxelData, Error> {
        let extent = pyramid
            .levels()
            .get(level)
            .ok_or_else(|| Error::Resolved {
                detail: "the requested resolution level does not exist".to_string(),
            })?
            .extent();
        let clipped = clip(ranges, extent);
        let mut out = region::allocate(fill, ranges);
        if clipped.iter().any(|range| range.start >= range.end) {
            return Ok(out);
        }
        let rows = clipped[2].clone();
        if rows.len() == 1 {
            let plane = self.plane(
                pyramid,
                key(self.namespace, time, level, channel, rows.start),
            )?;
            region::copy_plane(
                &mut out,
                ranges,
                plane.data(),
                [plane.width(), plane.height()],
                [clipped[0].clone(), clipped[1].clone()],
                rows.start,
            );
            return Ok(out);
        }
        let planes = rows
            .clone()
            .into_par_iter()
            .map(|z| self.plane(pyramid, key(self.namespace, time, level, channel, z)))
            .collect::<Result<Vec<_>, Error>>()?;
        for (offset, plane) in planes.iter().enumerate() {
            region::copy_plane(
                &mut out,
                ranges,
                plane.data(),
                [plane.width(), plane.height()],
                [clipped[0].clone(), clipped[1].clone()],
                rows.start + offset as u32,
            );
        }
        Ok(out)
    }

    fn plane(&self, pyramid: &Pyramid, key: Key) -> Result<Arc<Plane>, Error> {
        self.planes.load(key, || self.build(pyramid, key))
    }

    fn build(&self, pyramid: &Pyramid, key: Key) -> Result<Plane, Error> {
        let level = key.level as usize;
        let channel = pyramid
            .channels()
            .get(key.channel as usize)
            .ok_or_else(|| Error::Resolved {
                detail: "the requested channel does not exist".to_string(),
            })?;
        let extent = pyramid
            .levels()
            .get(level)
            .ok_or_else(|| Error::Resolved {
                detail: "the requested resolution level does not exist".to_string(),
            })?
            .extent();
        if level < pyramid.stored() {
            let data = self
                .reader
                .read_plane(level, key.time, key.channel as usize, key.z)?;
            return Ok(Plane::new(extent.x(), extent.y(), data));
        }
        let shape = Shape {
            width: extent.x(),
            height: extent.y(),
            format: channel.format(),
        };
        let stamp = self.stamp(pyramid, key)?;
        if let (Some(store), Some(stamp)) = (self.store.as_ref(), stamp)
            && let Some(data) = store.load(key, shape, stamp)
        {
            return Ok(Plane::new(extent.x(), extent.y(), data));
        }
        let child = pyramid.level(level - 1).extent();
        let lower = self.plane(pyramid, key.with_level(key.level - 1).with_z(2 * key.z))?;
        let upper = self.plane(
            pyramid,
            key.with_level(key.level - 1)
                .with_z((2 * key.z + 1).min(child.z() - 1)),
        )?;
        let held = mip::reduce(
            &lower,
            &upper,
            extent.x(),
            extent.y(),
            channel.semantics().averages(),
            channel.fill(),
        );
        if let (Some(store), Some(stamp)) = (self.store.as_ref(), stamp) {
            store.save(key, shape, stamp, held.data());
        }
        Ok(held)
    }

    fn stamp(&self, pyramid: &Pyramid, key: Key) -> Result<Option<u64>, Error> {
        let level = key.level as usize;
        let source = level.min(pyramid.stored().saturating_sub(1));
        let factor = 1u32 << (level - source);
        let extent = pyramid.level(source).extent();
        let start = key.z.saturating_mul(factor);
        let end = start.saturating_add(factor).min(extent.z());
        self.reader
            .stamp(source, key.time, key.channel as usize, start..end)
    }
}

fn key(namespace: u64, time: u32, level: usize, channel: usize, z: u32) -> Key {
    Key {
        namespace,
        level: u32::try_from(level).expect("a pyramid holds fewer than 4G levels"),
        time,
        channel: u32::try_from(channel).expect("a pyramid holds fewer than 4G channels"),
        z,
    }
}

fn clip(ranges: &[Range<u32>; 3], extent: Extent) -> [Range<u32>; 3] {
    let limits = [extent.x(), extent.y(), extent.z()];
    [0, 1, 2].map(|axis| {
        let start = ranges[axis].start.min(limits[axis]);
        let end = ranges[axis].end.min(limits[axis]);
        start..end
    })
}
