use grana_ui::Rect;

const TICK_LIMIT: usize = 24;
const TICK_EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    low: f32,
    high: f32,
    step: f32,
}

impl Axis {
    pub fn new(low: f32, high: f32, target: usize) -> Self {
        assert!(
            low.is_finite() && high.is_finite() && high >= low,
            "an axis spans a finite interval"
        );
        Self {
            low,
            high,
            step: step(low, high, target),
        }
    }

    pub fn low(self) -> f32 {
        self.low
    }

    pub fn high(self) -> f32 {
        self.high
    }

    pub fn ticks(self) -> impl Iterator<Item = f32> {
        let first = (self.low / self.step - TICK_EPSILON).ceil();
        let span = (self.high - self.low).max(0.0) / self.step;
        let count = (span.ceil().max(0.0) as usize + 2).min(TICK_LIMIT);
        let limit = self.high + self.step * 1.0e-4;
        (0..count)
            .map(move |offset| (first + offset as f32) * self.step)
            .take_while(move |value| *value <= limit)
    }

    pub fn x(self, frame: Rect, value: f32) -> f32 {
        let span = self.high - self.low;
        if span <= 0.0 {
            return frame.min.x;
        }
        frame.min.x + ((value - self.low) / span).clamp(0.0, 1.0) * frame.width()
    }

    pub fn y(self, frame: Rect, value: f32) -> f32 {
        let span = self.high - self.low;
        if span <= 0.0 {
            return frame.max.y;
        }
        frame.max.y - ((value - self.low) / span).clamp(0.0, 1.0) * frame.height()
    }

    pub fn value_at_x(self, frame: Rect, pixel: f32) -> f32 {
        let span = self.high - self.low;
        if span <= 0.0 {
            return self.low;
        }
        let position = ((pixel - frame.min.x) / frame.width().max(1.0)).clamp(0.0, 1.0);
        self.low + position * span
    }

    pub fn value_at_y(self, frame: Rect, pixel: f32) -> f32 {
        let span = self.high - self.low;
        if span <= 0.0 {
            return self.low;
        }
        let position = ((frame.max.y - pixel) / frame.height().max(1.0)).clamp(0.0, 1.0);
        self.low + position * span
    }

    pub fn label(self, value: f32) -> String {
        number(value, decimals(self.step))
    }
}

pub fn step(low: f32, high: f32, target: usize) -> f32 {
    let range = high - low;
    if !(range.is_finite() && range > 0.0) {
        return 1.0;
    }
    let raw = range / target.max(1) as f32;
    let magnitude = 10.0f32.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let selected = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    selected * magnitude
}

pub fn decimals(step: f32) -> usize {
    for places in 0..=4 {
        let scaled = step * 10f32.powi(places as i32);
        if (scaled - scaled.round()).abs() < 1.0e-3 * scaled.max(1.0) {
            return places;
        }
    }
    4
}

pub fn number(value: f32, places: usize) -> String {
    let unit = 10f32.powi(-(places as i32));
    let value = if value.abs() <= 0.5 * unit {
        0.0
    } else {
        value
    };
    if value.abs() >= 100_000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if value.abs() >= 10_000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.places$}")
    }
}
