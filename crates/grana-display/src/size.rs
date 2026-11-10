#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputSize {
    width: u32,
    height: u32,
}

impl OutputSize {
    pub const ASPECT: [u32; 2] = [16, 9];
    pub const DEFAULT_WIDTH: u32 = 1920;
    pub const MIN: u32 = 64;
    pub const MAX: u32 = 16384;

    pub fn of(width: u32, height: u32) -> Self {
        assert!(
            width > 0 && height > 0,
            "an output image covers at least one pixel"
        );
        Self { width, height }
    }

    pub fn of_width(width: u32) -> Self {
        let height = (u64::from(width) * u64::from(Self::ASPECT[1]) / u64::from(Self::ASPECT[0]))
            .max(1) as u32;
        Self::of(width, height)
    }

    pub fn even(self) -> Self {
        Self::of(even(self.width), even(self.height))
    }

    pub fn scaled(self, width: u32) -> Self {
        let height =
            (u64::from(width) * u64::from(self.height) / u64::from(self.width)).max(1) as u32;
        Self::of(width, height)
    }

    pub fn clamped(self) -> Self {
        Self::of(
            self.width.clamp(Self::MIN, Self::MAX),
            self.height.clamp(Self::MIN, Self::MAX),
        )
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub fn to_array(self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

fn even(value: u32) -> u32 {
    (value.max(2) / 2) * 2
}
