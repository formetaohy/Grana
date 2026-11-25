#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    position: f32,
    color: [f32; 3],
    opacity: f32,
}

impl Stop {
    pub fn new(position: f32, color: [f32; 3], opacity: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&position),
            "stop position must lie in the unit interval"
        );
        assert!(
            (0.0..=1.0).contains(&opacity),
            "stop opacity must lie in the unit interval"
        );
        assert!(
            color.iter().all(|channel| (0.0..=1.0).contains(channel)),
            "stop color must be an srgb triple"
        );
        Self {
            position,
            color,
            opacity,
        }
    }

    pub fn position(self) -> f32 {
        self.position
    }

    pub fn color(self) -> [f32; 3] {
        self.color
    }

    pub fn opacity(self) -> f32 {
        self.opacity
    }

    pub fn set_position(&mut self, position: f32) {
        assert!(
            (0.0..=1.0).contains(&position),
            "stop position must lie in the unit interval"
        );
        self.position = position;
    }

    pub fn set_color(&mut self, color: [f32; 3]) {
        assert!(
            color.iter().all(|channel| (0.0..=1.0).contains(channel)),
            "stop color must be an srgb triple"
        );
        self.color = color;
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        assert!(
            (0.0..=1.0).contains(&opacity),
            "stop opacity must lie in the unit interval"
        );
        self.opacity = opacity;
    }
}

const TABLE_LIMIT: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub enum TransferFunction {
    Ramp { stops: Vec<Stop> },
    Table { colors: Vec<[f32; 3]> },
}

impl TransferFunction {
    pub fn new(mut stops: Vec<Stop>) -> Self {
        assert!(
            !stops.is_empty(),
            "a transfer function needs at least one stop"
        );
        sort(&mut stops);
        Self::Ramp { stops }
    }

    pub fn tinted(tint: [f32; 3]) -> Self {
        assert!(
            tint.iter().all(|channel| (0.0..=1.0).contains(channel)),
            "a channel tint is an srgb triple"
        );
        let dim = |factor: f32| [tint[0] * factor, tint[1] * factor, tint[2] * factor];
        let lighten = |amount: f32| {
            [
                tint[0] + (1.0 - tint[0]) * amount,
                tint[1] + (1.0 - tint[1]) * amount,
                tint[2] + (1.0 - tint[2]) * amount,
            ]
        };
        Self::new(vec![
            Stop::new(0.00, [0.0, 0.0, 0.0], 0.00),
            Stop::new(0.25, dim(0.35), 0.04),
            Stop::new(0.50, dim(0.75), 0.22),
            Stop::new(0.75, tint, 0.55),
            Stop::new(1.00, lighten(0.45), 1.00),
        ])
    }

    pub fn grayscale() -> Self {
        Self::new(vec![
            Stop::new(0.00, [0.00, 0.00, 0.00], 0.00),
            Stop::new(0.30, [0.45, 0.45, 0.45], 0.10),
            Stop::new(0.70, [0.85, 0.85, 0.85], 0.45),
            Stop::new(1.00, [1.00, 1.00, 1.00], 1.00),
        ])
    }

    pub fn indexed(colors: impl Into<Vec<[f32; 3]>>) -> Self {
        let colors = colors.into();
        assert!(
            (2..=TABLE_LIMIT).contains(&colors.len()),
            "an indexed transfer function holds two to {TABLE_LIMIT} colours"
        );
        assert!(
            colors
                .iter()
                .flatten()
                .all(|channel| (0.0..=1.0).contains(channel)),
            "an indexed transfer function entry is an srgb triple"
        );
        Self::Table { colors }
    }

    pub fn stops(&self) -> Option<&[Stop]> {
        match self {
            Self::Ramp { stops } => Some(stops),
            Self::Table { .. } => None,
        }
    }

    pub fn colors(&self) -> &[[f32; 3]] {
        match self {
            Self::Ramp { .. } => &[],
            Self::Table { colors } => colors,
        }
    }

    pub fn ramp_mut(&mut self) -> &mut Vec<Stop> {
        match self {
            Self::Ramp { stops } => stops,
            Self::Table { .. } => {
                panic!("an indexed transfer function stops are fixed by its colour table")
            }
        }
    }

    pub fn add_stop(&mut self, stop: Stop) {
        let stops = self.ramp_mut();
        stops.push(stop);
        sort(stops);
    }

    pub fn remove_stop(&mut self, index: usize) {
        let stops = self.ramp_mut();
        assert!(
            stops.len() > 1,
            "a transfer function keeps at least one stop"
        );
        assert!(
            index < stops.len(),
            "the removed stop lies inside the function"
        );
        stops.remove(index);
    }

    pub fn lut(&self, size: usize) -> Vec<[f32; 4]> {
        assert!(size >= 2, "a lookup table needs at least two entries");
        match self {
            Self::Ramp { stops } => {
                let last = (size - 1) as f32;
                (0..size)
                    .map(|index| ramp_entry(stops, index as f32 / last))
                    .collect()
            }
            Self::Table { colors } => (0..size)
                .map(|index| {
                    let entry = index * colors.len() / size;
                    let [red, green, blue] = srgb_to_linear(colors[entry]);
                    [red, green, blue, f32::from(u8::from(entry > 0))]
                })
                .collect(),
        }
    }
}

fn ramp_entry(stops: &[Stop], position: f32) -> [f32; 4] {
    let right = stops.partition_point(|stop| stop.position < position);
    let (left, right) = match right {
        0 => (&stops[0], &stops[0]),
        index if index >= stops.len() => (
            stops.last().expect("a transfer function is never empty"),
            stops.last().expect("a transfer function is never empty"),
        ),
        index => (&stops[index - 1], &stops[index]),
    };
    let span = right.position - left.position;
    let t = if span > 0.0 {
        ((position - left.position) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let color = [
        lerp(left.color[0], right.color[0], t),
        lerp(left.color[1], right.color[1], t),
        lerp(left.color[2], right.color[2], t),
    ];
    let [red, green, blue] = srgb_to_linear(color);
    [red, green, blue, lerp(left.opacity, right.opacity, t)]
}

fn sort(stops: &mut [Stop]) {
    stops.sort_by(|left, right| {
        left.position
            .partial_cmp(&right.position)
            .expect("stop positions are finite")
    });
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    (from + (to - from) * t).clamp(0.0, 1.0)
}

pub fn srgb_to_linear(srgb: [f32; 3]) -> [f32; 3] {
    srgb.map(srgb_component_to_linear)
}

pub fn srgb_component_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}
