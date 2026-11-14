use std::sync::Arc;

use grana_object::{LocalizationFilter, Spot, SpotSet, TrackSet};

use crate::frame::Frame;
use crate::object::ObjectFrame;
use crate::object_set::Visibility;

const ABSENT_ROW: u32 = u32::MAX;

pub struct ObjectSource {
    frames: Vec<Option<Frame>>,
    visibility: Visibility,
    hidden: Arc<Vec<Vec<bool>>>,
    restricted: bool,
    localization: LocalizationFilter,
    tracks: Option<Arc<TrackSet>>,
}

impl ObjectSource {
    pub(crate) fn live(
        frames: Vec<Option<Frame>>,
        visibility: Visibility,
        hidden: Vec<Vec<bool>>,
        localization: LocalizationFilter,
        tracks: Option<Arc<TrackSet>>,
    ) -> Self {
        let hidden = Arc::new(hidden);
        let restricted = !matches!(visibility, Visibility::All)
            || hidden.iter().any(|mask| mask.iter().any(|hidden| *hidden));
        Self {
            frames,
            visibility,
            hidden,
            restricted,
            localization,
            tracks,
        }
    }

    pub(crate) fn plain(frames: Vec<Option<Frame>>) -> Self {
        Self {
            frames,
            visibility: Visibility::All,
            hidden: Arc::new(Vec::new()),
            restricted: false,
            localization: LocalizationFilter::NONE,
            tracks: None,
        }
    }

    pub fn frames(&self) -> u32 {
        self.frames.len() as u32
    }

    pub fn counts(&self) -> Vec<Option<usize>> {
        self.frames
            .iter()
            .map(|frame| frame.as_ref().map(Frame::objects))
            .collect()
    }

    pub fn object_count(&self, time: u32) -> usize {
        self.frames
            .get(time as usize)
            .and_then(Option::as_ref)
            .map_or(0, Frame::objects)
    }

    pub fn frame(&self, time: u32) -> Option<Arc<ObjectFrame>> {
        self.frames.get(time as usize)?.as_ref().map(Frame::shared)
    }

    pub fn spot_frame(&self, time: u32) -> Option<Arc<SpotSet>> {
        self.frames.get(time as usize)?.as_ref()?.spot_frame()
    }

    pub fn filtered(&self) -> bool {
        self.restricted || self.localization.active()
    }

    pub fn per_object_visibility(&self) -> bool {
        !matches!(self.visibility, Visibility::All)
    }

    pub fn filtered_object(&self, time: u32, object: usize) -> bool {
        match &self.visibility {
            Visibility::All => true,
            Visibility::Objects { mask, frames } => frames
                .get(time as usize)
                .filter(|base| **base != ABSENT_ROW)
                .and_then(|base| base.checked_add(u32::try_from(object).ok()?))
                .is_some_and(|row| mask.contains(row as usize)),
            Visibility::Tracks(mask) => self
                .tracks
                .as_deref()
                .and_then(|set| set.track_at(time, object))
                .is_some_and(|track| mask.contains(track)),
            Visibility::Molecules { mask, rows } => self
                .spot_frame(time)
                .and_then(|spots| spots.spots().get(object).and_then(|spot| spot.group()))
                .and_then(|group| rows.get(&group).copied())
                .is_some_and(|row| mask.contains(row as usize)),
        }
    }

    pub fn visible_object(&self, time: u32, object: usize) -> bool {
        if !self.admits(time, object) {
            return false;
        }
        if !self.localization.active() {
            return true;
        }
        match self.spot_frame(time) {
            Some(set) => set
                .spots()
                .get(object)
                .is_none_or(|spot| self.localization.keeps(spot.quality(), spot.sigma())),
            None => true,
        }
    }

    pub fn visible_spot(&self, time: u32, object: usize, spot: &Spot) -> bool {
        self.admits_spot(time, object, spot)
            && (!self.localization.active()
                || self.localization.keeps(spot.quality(), spot.sigma()))
    }

    fn admits(&self, time: u32, object: usize) -> bool {
        self.filtered_object(time, object)
            && !self
                .hidden
                .get(time as usize)
                .and_then(|mask| mask.get(object))
                .copied()
                .unwrap_or(false)
    }

    fn admits_spot(&self, time: u32, object: usize, spot: &Spot) -> bool {
        let admitted = match &self.visibility {
            Visibility::Molecules { mask, rows } => spot
                .group()
                .and_then(|group| rows.get(&group).copied())
                .is_some_and(|row| mask.contains(row as usize)),
            Visibility::All | Visibility::Objects { .. } | Visibility::Tracks(_) => {
                self.filtered_object(time, object)
            }
        };
        admitted
            && !self
                .hidden
                .get(time as usize)
                .and_then(|mask| mask.get(object))
                .copied()
                .unwrap_or(false)
    }
}
