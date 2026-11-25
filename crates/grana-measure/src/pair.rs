use grana_object::RelationFrame;

use crate::column::{area_key, length_key};
use crate::csv;

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
        let columns = [
            "frame".to_string(),
            "time_s".to_string(),
            "source_set".to_string(),
            "target_set".to_string(),
            "source".to_string(),
            "target".to_string(),
            "contacting".to_string(),
            area_key("contact"),
            length_key("distance"),
            "inside_percent".to_string(),
            length_key("center"),
            "source_partners".to_string(),
        ];
        let columns = columns.iter().map(String::as_str).collect::<Vec<_>>();
        Self {
            interval,
            source: source.to_string(),
            target: target.to_string(),
            text: csv::line(&columns),
        }
    }

    pub fn push(&mut self, time: u32, frame: &RelationFrame) {
        for pair in frame.pairs() {
            let overlapping = frame
                .reading(pair.source())
                .map_or(0, grana_object::RelationReading::overlapping);
            self.text.push_str(&csv::line(&[
                &(time + 1).to_string(),
                &format!("{:.3}", time as f32 * self.interval),
                &self.source,
                &self.target,
                &pair.source().raw().to_string(),
                &pair.target().raw().to_string(),
                if pair.contact() > 0.0 { "1" } else { "0" },
                &format!("{:.3}", pair.contact()),
                &format!("{:.3}", pair.distance()),
                &format!("{:.2}", 100.0 * pair.inside()),
                &format!("{:.3}", pair.center()),
                &overlapping.to_string(),
            ]));
        }
    }

    pub fn text(self) -> String {
        self.text
    }
}
