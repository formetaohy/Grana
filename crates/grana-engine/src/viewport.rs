use grana_display::{OutputSize, SurfaceSize};

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub size: [u32; 2],
    pub pixels_per_point: f32,
    pub region: [u32; 4],
}

impl Viewport {
    pub fn screen(surface: SurfaceSize, region: [u32; 4]) -> Self {
        Self {
            size: surface.physical(),
            pixels_per_point: surface.scale(),
            region,
        }
        .checked()
    }

    pub fn export(size: OutputSize) -> Self {
        let size = size.to_array();
        Self {
            size,
            pixels_per_point: 1.0,
            region: [0, 0, size[0], size[1]],
        }
        .checked()
    }

    pub fn points(&self) -> [f32; 4] {
        [
            self.region[0] as f32 / self.pixels_per_point,
            self.region[1] as f32 / self.pixels_per_point,
            self.region[2] as f32 / self.pixels_per_point,
            self.region[3] as f32 / self.pixels_per_point,
        ]
    }

    pub fn aspect(&self) -> f32 {
        self.region[2].max(1) as f32 / self.region[3].max(1) as f32
    }

    fn checked(self) -> Self {
        assert!(
            self.size[0] > 0 && self.size[1] > 0,
            "a viewport covers at least one pixel"
        );
        let Self { region, size, .. } = self;
        assert!(
            region[2] > 0 && region[3] > 0,
            "a view lies on a positive surface"
        );
        assert!(
            region[0] + region[2] <= size[0] && region[1] + region[3] <= size[1],
            "the view lies on the surface"
        );
        assert!(
            self.pixels_per_point > 0.0,
            "a screen holds a positive pixel density"
        );
        self
    }
}
