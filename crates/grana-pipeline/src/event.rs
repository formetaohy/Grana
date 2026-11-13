use grana_analysis::localization::{Clusters, LocalizationReport};
use grana_measure::{ColocSeries, ObjectColoc};
use grana_object::{RelationFrame, TrackSet};
use grana_optics::MeasuredPsf;
use grana_scene::{Drift, ObjectFrame, ObjectSetId, Registration, Revision};
use grana_volume::Region;

pub enum Event {
    Progress {
        time: u32,
        done: usize,
        total: usize,
    },
    Frame {
        time: u32,
        frame: ObjectFrame,
    },
    Localization {
        pixel: f32,
        width: u32,
        height: u32,
        volume: bool,
        photons: bool,
        drift: Drift,
    },
    LocalizationReport {
        set: ObjectSetId,
        revision: Revision,
        report: Box<LocalizationReport>,
    },
    LocalizationClusters {
        set: ObjectSetId,
        revision: Revision,
        clusters: Box<Clusters>,
    },
    Tracks(TrackSet),
    Relation {
        time: u32,
        frame: RelationFrame,
    },
    Coloc {
        series: Box<ColocSeries>,
        region: Region,
    },
    ObjectColoc {
        coloc: Box<ObjectColoc>,
    },
    Drift(Drift),
    Registration(Registration),
    Calibration {
        source: usize,
        psf: Box<MeasuredPsf>,
        beads: usize,
        iterations: u32,
    },
    Done,
    Failed(String),
}
