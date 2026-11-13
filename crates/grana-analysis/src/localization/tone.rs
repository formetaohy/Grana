use super::grid::Grid;
use super::report::LocalizationImage;

const HIGHLIGHT: f32 = 0.999;

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
    scale: f32,
    table: [[u8; 3]; ENTRIES],
}

impl Tone {
    pub(super) fn density(grid: &Grid) -> f32 {
        let highlight = {
            let held = grid.percentile(HIGHLIGHT);
            if held > 0.0 { held } else { grid.maximum() }
        };
        let pixel = grid.pixel();
        highlight / (pixel * pixel)
    }

    pub(super) fn new(density: f32, pixel: f32) -> Self {
        let highlight = density * pixel * pixel;
        let scale = if highlight > 0.0 {
            1.0 / highlight
        } else {
            0.0
        };
        Self {
            scale,
            table: table(),
        }
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

pub(super) fn tone(grid: &Grid) -> (LocalizationImage, f32) {
    let density = Tone::density(grid);
    let tone = Tone::new(density, grid.pixel());
    let size = grid.size();
    let mut pixels = Vec::with_capacity(grid.values().len() * 4);
    for (index, value) in grid.values().iter().enumerate() {
        let [red, green, blue] = tone.shade(*value, grid.chroma(index));
        pixels.extend_from_slice(&[red, green, blue, 255]);
    }
    (LocalizationImage::new(size, pixels), density)
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
