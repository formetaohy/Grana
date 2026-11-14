use std::sync::Arc;

use grana_object::{LocalizationFilter, Spot};

use super::Point;
use super::UNGROUPED;

#[derive(Clone)]
pub struct LocalizationFrame {
    spots: Arc<[Spot]>,
    visible: Option<Arc<Vec<bool>>>,
    filter: LocalizationFilter,
}

impl LocalizationFrame {
    pub fn new(
        spots: Arc<[Spot]>,
        visible: Option<Arc<Vec<bool>>>,
        filter: LocalizationFilter,
    ) -> Self {
        if let Some(visible) = &visible {
            assert_eq!(
                visible.len(),
                spots.len(),
                "a localization frame marks every spot it holds"
            );
        }
        Self {
            spots,
            visible,
            filter,
        }
    }

    pub fn spots(&self) -> &[Spot] {
        &self.spots
    }

    pub(super) fn visible(&self, index: usize) -> bool {
        let spot = &self.spots[index];
        (self
            .visible
            .as_ref()
            .is_none_or(|mask| mask.get(index).copied().unwrap_or(false)))
            && self.filter.keeps(spot.quality(), spot.sigma())
    }

    pub(super) fn points(&self, grouped: bool) -> Vec<Point> {
        self.spots
            .iter()
            .enumerate()
            .filter(|(index, _)| self.visible(*index))
            .map(|(_, spot)| {
                let center = spot.center();
                Point {
                    center: [center[0], center[1]],
                    group: spot.group().filter(|_| grouped).unwrap_or(UNGROUPED),
                }
            })
            .collect()
    }
}
