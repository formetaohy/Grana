#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub position: f32,
    pub color: [f32; 3],
    pub opacity: f32,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferFunction {
    stops: Vec<Stop>,
}

impl TransferFunction {
    pub fn new(mut stops: Vec<Stop>) -> Self {
        assert!(
            !stops.is_empty(),
            "a transfer function needs at least one stop"
        );
        stops.sort_by(|left, right| {
            left.position
                .partial_cmp(&right.position)
                .expect("stop positions are finite")
        });
        Self { stops }
    }

    pub fn fluorescence() -> Self {
        Self::new(vec![
            Stop::new(0.00, [0.02, 0.01, 0.08], 0.00),
            Stop::new(0.20, [0.20, 0.03, 0.30], 0.06),
            Stop::new(0.40, [0.65, 0.08, 0.25], 0.20),
            Stop::new(0.60, [0.95, 0.40, 0.10], 0.42),
            Stop::new(0.80, [1.00, 0.78, 0.25], 0.66),
            Stop::new(1.00, [1.00, 1.00, 0.95], 1.00),
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

    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }

    pub fn lut(&self, size: usize) -> Vec<[f32; 4]> {
        assert!(size >= 2, "a lookup table needs at least two entries");
        let last = (size - 1) as f32;
        (0..size)
            .map(|index| self.sample(index as f32 / last))
            .collect()
    }

    fn sample(&self, position: f32) -> [f32; 4] {
        let stops = &self.stops;
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
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn srgb_to_linear(srgb: [f32; 3]) -> [f32; 3] {
    srgb.map(|channel| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    })
}
