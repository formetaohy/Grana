use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::localization::{ClusterSpec, LocalizationFilter, LocalizationFrame};
use grana_object::Spot;
use grana_scene::{ObjectSetId, Revision};

use super::localization_window::LocalizationWindow;
use crate::event::Event;

pub struct LocalizationClusterRequest {
    set: ObjectSetId,
    revision: Revision,
    window: LocalizationWindow,
    spec: ClusterSpec,
    filter: LocalizationFilter,
    grouped: bool,
}

impl LocalizationClusterRequest {
    pub fn new(
        set: ObjectSetId,
        revision: Revision,
        window: LocalizationWindow,
        spec: ClusterSpec,
        filter: LocalizationFilter,
        grouped: bool,
    ) -> Self {
        assert!(
            !window.is_empty(),
            "a molecule clustering names the frames of its range"
        );
        Self {
            set,
            revision,
            window,
            spec,
            filter,
            grouped,
        }
    }
}

pub fn localization_cluster(
    request: LocalizationClusterRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let LocalizationClusterRequest {
        set,
        revision,
        window,
        spec,
        filter,
        grouped,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let empty: &[Spot] = &[];
    let held = (0..window.len())
        .map(|index| {
            let spots = window
                .frame(index)
                .and_then(|frame| frame.spots())
                .map_or(empty, |set| set.spots());
            LocalizationFrame::new(spots, window.visible(index), filter)
        })
        .collect::<Vec<_>>();
    let mut report = |done: usize, total: usize| {
        sink(Event::Progress {
            time: 0,
            done,
            total,
        });
        !cancel.load(Ordering::Relaxed)
    };
    let Some(clusters) = grana_analysis::localization::cluster(&held, spec, grouped, &mut report)?
    else {
        sink(Event::Done);
        return Ok(());
    };
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    sink(Event::LocalizationClusters {
        set,
        revision,
        clusters: Box::new(clusters),
    });
    sink(Event::Done);
    Ok(())
}
