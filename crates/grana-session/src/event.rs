use grana_measure::ColocSeries;
use grana_object::TrackSet;
use grana_scene::ObjectFrame;
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
    Coloc {
        series: Box<ColocSeries>,
        region: Region,
    },
    Done,
    Failed(String),
}

pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}
