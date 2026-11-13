use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::localization::{ClusterSpec, LocalizationFrame};
use grana_object::Spot;
use grana_scene::{ObjectFrame, ObjectSetId, Revision};

use crate::event::Event;

pub struct LocalizationClusterRequest {
    set: ObjectSetId,
    revision: Revision,
    frames: Vec<Option<Arc<ObjectFrame>>>,
    visible: Vec<Option<Vec<bool>>>,
    spec: ClusterSpec,
    grouped: bool,
}

impl LocalizationClusterRequest {
    pub fn new(
        set: ObjectSetId,
        revision: Revision,
        frames: Vec<Option<Arc<ObjectFrame>>>,
        visible: Vec<Option<Vec<bool>>>,
        spec: ClusterSpec,
        grouped: bool,
    ) -> Self {
        assert!(
            !frames.is_empty(),
            "a molecule clustering names the frames of its range"
        );
        assert_eq!(
            frames.len(),
            visible.len(),
            "a molecule clustering marks every frame it ranges over"
        );
        Self {
            set,
            revision,
            frames,
            visible,
            spec,
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
        frames,
        visible,
        spec,
        grouped,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let empty: &[Spot] = &[];
    let held = (0..frames.len())
        .map(|index| {
            let spots = frames[index]
                .as_deref()
                .and_then(ObjectFrame::spots)
                .map_or(empty, |set| set.spots());
            LocalizationFrame::new(spots, visible[index].as_deref())
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
