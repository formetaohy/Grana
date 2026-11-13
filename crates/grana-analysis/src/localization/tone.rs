const HIGHLIGHT_BINS: usize = 4096;

const ENTRIES: usize = 1024;

const STOPS: [(f32, [f32; 3]); 6] = [
    (0.00, [0.00, 0.00, 0.00]),
    (0.18, [0.13, 0.01, 0.24]),
    (0.40, [0.52, 0.05, 0.31]),
    (0.62, [0.91, 0.28, 0.12]),
    (0.84, [1.00, 0.72, 0.16]),
    (1.00, [1.00, 1.00, 0.95]),
];

pub(super) struct Tone {
    density: f32,
    scale: f32,
    table: [[u8; 3]; ENTRIES],
}

impl Tone {
    pub(super) fn new(density: f32, pixel: f32) -> Self {
        let highlight = density * pixel * pixel;
        let scale = if highlight > 0.0 {
            1.0 / highlight
        } else {
            0.0
        };
        Self {
            density,
            scale,
            table: table(),
        }
    }

    pub(super) fn density(&self) -> f32 {
        self.density
    }

    pub(super) fn level(&self, value: f32) -> f32 {
        (value * self.scale).clamp(0.0, 1.0).sqrt()
    }

    pub(super) fn shade(&self, value: f32, chroma: Option<[f32; 3]>) -> [u8; 3] {
        let level = self.level(value);
        match chroma {
            Some(chroma) => {
                let peak = chroma.iter().copied().fold(0.0f32, f32::max);
                if peak <= 0.0 {
                    return [0, 0, 0];
                }
                chroma.map(|component| {
                    (grana_display::linear_to_srgb(component / peak * level) * 255.0).round() as u8
                })
            }
            None => {
                let entry = (level * (ENTRIES - 1) as f32) as usize;
                self.table[entry.min(ENTRIES - 1)]
            }
        }
    }
}

pub(super) struct Histogram {
    bins: Vec<u64>,
    maximum: f32,
    count: u64,
}

impl Histogram {
    pub(super) fn new(maximum: f32) -> Self {
        Self {
            bins: vec![0; HIGHLIGHT_BINS],
            maximum,
            count: 0,
        }
    }

    pub(super) fn push(&mut self, values: &[f32]) {
        let scale = (HIGHLIGHT_BINS - 1) as f32 / self.maximum;
        for value in values {
            let bin = (value * scale) as usize;
            self.bins[bin.min(HIGHLIGHT_BINS - 1)] += 1;
        }
        self.count += values.len() as u64;
    }

    pub(super) fn highlight(&self, fraction: f32) -> f32 {
        if self.count == 0 || self.maximum <= 0.0 {
            return 0.0;
        }
        let scale = (HIGHLIGHT_BINS - 1) as f32 / self.maximum;
        let target = (fraction.clamp(0.0, 1.0) * self.count as f32) as u64;
        let mut held = 0u64;
        for (bin, count) in self.bins.iter().enumerate() {
            held += count;
            if held >= target {
                return (bin as f32 + 0.5) / scale;
            }
        }
        self.maximum
    }
}

fn table() -> [[u8; 3]; ENTRIES] {
    std::array::from_fn(|index| {
        let level = index as f32 / (ENTRIES - 1) as f32;
        let mut segment = 0usize;
        while segment + 2 < STOPS.len() && level > STOPS[segment + 1].0 {
            segment += 1;
        }
        let (low, from) = STOPS[segment];
        let (high, to) = STOPS[segment + 1];
        let weight = if high > low {
            ((level - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        [0, 1, 2].map(|axis| {
            let linear = from[axis] + weight * (to[axis] - from[axis]);
            (grana_display::linear_to_srgb(linear.clamp(0.0, 1.0)) * 255.0).round() as u8
        })
    })
}
