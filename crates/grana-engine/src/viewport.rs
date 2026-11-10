use grana_display::OutputSize;

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub size: [u32; 2],
    pub pixels_per_point: f32,
    pub region: [u32; 4],
}

impl Viewport {
    pub fn screen(scale: f32, size: [u32; 2]) -> Self {
        assert!(scale > 0.0, "a screen holds a positive pixel density");
        Self {
            size,
            pixels_per_point: scale,
            region: region(size),
        }
    }

    pub fn export(size: OutputSize) -> Self {
        let size = size.to_array();
        Self {
            size,
            pixels_per_point: 1.0,
            region: region(size),
        }
    }

    pub fn points(&self) -> [f32; 4] {
        [
            0.0,
            0.0,
            self.region[2] as f32 / self.pixels_per_point,
            self.region[3] as f32 / self.pixels_per_point,
        ]
    }

    pub fn aspect(&self) -> f32 {
        self.region[2].max(1) as f32 / self.region[3].max(1) as f32
    }
}

fn region(size: [u32; 2]) -> [u32; 4] {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a viewport covers at least one pixel"
    );
    [0, 0, size[0], size[1]]
}
