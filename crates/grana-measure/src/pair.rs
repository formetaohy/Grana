use grana_object::RelationFrame;

use crate::csv;

const COLUMNS: [&str; 12] = [
    "frame",
    "time_s",
    "source_set",
    "target_set",
    "source",
    "target",
    "contacting",
    "contact_um2",
    "distance_um",
    "inside_percent",
    "center_um",
    "source_partners",
];

pub struct PairExport {
    interval: f32,
    source: String,
    target: String,
    text: String,
}

impl PairExport {
    pub fn file_name(source: &str, target: &str, shift: i32) -> String {
        let offset = match shift {
            0 => String::new(),
            shift => format!("-t{shift:+}"),
        };
        format!("relations-{source}-{target}{offset}.csv")
    }

    pub fn new(interval: f32, source: &str, target: &str) -> Self {
        assert!(
            interval.is_finite() && interval > 0.0,
            "a pair export names a positive frame interval"
        );
        assert!(
            !source.trim().is_empty() && !target.trim().is_empty(),
            "a pair export names both object sets"
        );
        Self {
            interval,
            source: source.to_string(),
            target: target.to_string(),
            text: csv::line(&COLUMNS),
        }
    }

    pub fn push(&mut self, time: u32, frame: &RelationFrame) {
        let objects = frame.objects();
        for pair in frame.pairs() {
            let source = pair.source() as usize;
            let target = pair.target() as usize;
            assert!(source < objects, "a relation pair names its source object");
            self.text.push_str(&csv::line(&[
                &(time + 1).to_string(),
                &format!("{:.3}", time as f32 * self.interval),
                &self.source,
                &self.target,
                &(source + 1).to_string(),
                &(target + 1).to_string(),
                if pair.contact() > 0.0 { "1" } else { "0" },
                &format!("{:.3}", pair.contact()),
                &format!("{:.3}", pair.distance()),
                &format!("{:.2}", 100.0 * pair.inside()),
                &format!("{:.3}", pair.center()),
                &frame.overlapping(source).to_string(),
            ]));
        }
    }

    pub fn text(self) -> String {
        self.text
    }
}
