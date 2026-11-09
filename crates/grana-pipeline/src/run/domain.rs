use glam::Vec3;
use grana_object::{Mask, Roi};
use grana_volume::{Level, Region};

pub(crate) struct Domain {
    boxed: Option<Mask>,
    level: Level,
}

impl Domain {
    pub fn new(region: Region, level: Level) -> Self {
        let boxed = (region.enabled() && region.axes() != [Vec3::X, Vec3::Y, Vec3::Z])
            .then(|| Mask::boxed(region, level));
        Self { boxed, level }
    }

    pub fn mask(&self, roi: Option<&Roi>, time: u32) -> Result<Option<Mask>, String> {
        let shaped = match roi {
            Some(roi) => Some(roi.mask(self.level, time)?),
            None => None,
        };
        Ok(match (shaped, &self.boxed) {
            (Some(shaped), Some(boxed)) => Some(shaped.intersected(boxed)),
            (Some(mask), None) => Some(mask),
            (None, Some(mask)) => Some(mask.clone()),
            (None, None) => None,
        })
    }
}
