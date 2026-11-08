use grana_measure::ColocSeries;
use grana_object::{RelationFrame, TrackSet};
use grana_optics::MeasuredPsf;
use grana_scene::{Drift, ObjectFrame};
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
    Tracks(TrackSet),
    Relation {
        time: u32,
        frame: RelationFrame,
    },
    Coloc {
        series: Box<ColocSeries>,
        region: Region,
    },
    Drift(Drift),
    Calibration {
        source: usize,
        psf: Box<MeasuredPsf>,
        beads: usize,
        iterations: u32,
    },
    Done,
    Failed(String),
}
