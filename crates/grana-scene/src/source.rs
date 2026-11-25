use std::sync::Arc;

use grana_object::{LocalizationFilter, ObjectIds, Spot, SpotSet, TrackSet};

use crate::frame::Frame;
use crate::object::ObjectFrame;
use crate::object_set::Visibility;

const ABSENT_ROW: u32 = u32::MAX;

pub struct ObjectSource {
    frames: Vec<Option<Frame>>,
    visibility: Visibility,
    hidden: Vec<Option<Arc<Vec<bool>>>>,
    restricted: bool,
    localization: LocalizationFilter,
    tracks: Option<Arc<TrackSet>>,
}

impl ObjectSource {
    pub(crate) fn live(
        frames: Vec<Option<Frame>>,
        visibility: Visibility,
        hidden: Arc<Vec<ObjectIds>>,
        localization: LocalizationFilter,
        tracks: Option<Arc<TrackSet>>,
    ) -> Self {
        let hidden = hidden
            .iter()
            .enumerate()
            .map(|(time, ids)| {
                (!ids.is_empty()).then(|| {
                    let count = frames
                        .get(time)
                        .and_then(Option::as_ref)
                        .map_or(0, Frame::objects);
                    let frame = frames.get(time).and_then(Option::as_ref).map(Frame::shared);
                    let mut mask = vec![false; count];
                    if let Some(frame) = frame {
                        for id in ids.as_slice() {
                            if let Some(object) = frame.index_of(*id) {
                                mask[object] = true;
                            }
                        }
                    }
                    Arc::new(mask)
                })
            })
            .collect::<Vec<_>>();
        let restricted =
            !matches!(visibility, Visibility::All) || hidden.iter().any(Option::is_some);
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
            hidden: Vec::new(),
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

    pub fn restricted(&self) -> bool {
        self.restricted
    }

    pub fn filtered_object(&self, time: u32, object: usize) -> bool {
        match &self.visibility {
            Visibility::All => true,
            Visibility::Objects { mask, frames } => frames
                .get(time as usize)
                .filter(|base| **base != ABSENT_ROW)
                .and_then(|base| base.checked_add(u32::try_from(object).ok()?))
                .is_some_and(|row| mask.contains(row as usize)),
            Visibility::Tracks(mask) => {
                let Some(id) = self.id_of(time, object) else {
                    return false;
                };
                self.tracks
                    .as_deref()
                    .and_then(|set| set.track_at(time, id))
                    .is_some_and(|track| mask.contains(track))
            }
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

    fn id_of(&self, time: u32, object: usize) -> Option<grana_object::ObjectId> {
        self.frames
            .get(time as usize)
            .and_then(Option::as_ref)
            .map(Frame::shared)?
            .id_of(object)
    }

    fn hidden_object(&self, time: u32, object: usize) -> bool {
        self.hidden
            .get(time as usize)
            .and_then(Option::as_ref)
            .and_then(|mask| mask.get(object))
            .copied()
            .unwrap_or(false)
    }

    fn admits(&self, time: u32, object: usize) -> bool {
        self.filtered_object(time, object) && !self.hidden_object(time, object)
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
        admitted && !self.hidden_object(time, object)
    }
}
