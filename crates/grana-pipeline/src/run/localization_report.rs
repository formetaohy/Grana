use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::localization::{LocalizationFrame, LocalizationSpec};
use grana_object::Spot;
use grana_scene::{ObjectSetId, Revision};
use grana_volume::Region;

use super::localization_window::LocalizationWindow;
use crate::event::Event;

pub struct LocalizationReportRequest {
    set: ObjectSetId,
    revision: Revision,
    window: LocalizationWindow,
    spec: LocalizationSpec,
    region: Option<Region>,
    pixel: f32,
    grouped: bool,
}

impl LocalizationReportRequest {
    pub fn new(
        set: ObjectSetId,
        revision: Revision,
        window: LocalizationWindow,
        spec: LocalizationSpec,
        region: Option<Region>,
        pixel: f32,
        grouped: bool,
    ) -> Self {
        assert!(
            !window.is_empty(),
            "a localization report names the frames of its range"
        );
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization report knows its positive camera pixel"
        );
        Self {
            set,
            revision,
            window,
            spec,
            region,
            pixel,
            grouped,
        }
    }
}

pub fn localization_report(
    request: LocalizationReportRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let LocalizationReportRequest {
        set,
        revision,
        window,
        spec,
        region,
        pixel,
        grouped,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let filter = spec.filter();
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
    let Some(found) = grana_analysis::localization::localization_report(
        &held,
        spec,
        pixel,
        grouped,
        region,
        &mut report,
    )?
    else {
        sink(Event::Done);
        return Ok(());
    };
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    sink(Event::LocalizationReport {
        set,
        revision,
        report: Box::new(found),
    });
    sink(Event::Done);
    Ok(())
}
