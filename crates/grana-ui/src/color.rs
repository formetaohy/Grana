#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl std::hash::Hash for Color {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.red.to_bits().hash(state);
        self.green.to_bits().hash(state);
        self.blue.to_bits().hash(state);
        self.alpha.to_bits().hash(state);
    }
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);

    pub const fn rgb(red: f32, green: f32, blue: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 1.0,
        }
    }

    pub const fn rgba(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub const fn gray(value: f32) -> Self {
        Self::rgb(value, value, value)
    }

    pub const fn bytes(red: u8, green: u8, blue: u8) -> Self {
        Self::rgba(
            red as f32 / 255.0,
            green as f32 / 255.0,
            blue as f32 / 255.0,
            1.0,
        )
    }

    pub fn bytes_alpha(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self::bytes(red, green, blue).with_alpha(alpha as f32 / 255.0)
    }

    pub fn white_alpha(alpha: f32) -> Self {
        Self::WHITE.with_alpha(alpha)
    }

    pub fn black_alpha(alpha: f32) -> Self {
        Self::BLACK.with_alpha(alpha)
    }

    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn amplify(self, factor: f32) -> Self {
        self.with_alpha(self.alpha * factor)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            red: self.red + (other.red - self.red) * t,
            green: self.green + (other.green - self.green) * t,
            blue: self.blue + (other.blue - self.blue) * t,
            alpha: self.alpha + (other.alpha - self.alpha) * t,
        }
    }

    pub fn over(self, background: Self) -> Self {
        let alpha = self.alpha + background.alpha * (1.0 - self.alpha);
        if alpha <= 0.0 {
            return Self::TRANSPARENT;
        }
        let scale = 1.0 / alpha;
        Self {
            red: (self.red * self.alpha + background.red * background.alpha * (1.0 - self.alpha))
                * scale,
            green: (self.green * self.alpha
                + background.green * background.alpha * (1.0 - self.alpha))
                * scale,
            blue: (self.blue * self.alpha
                + background.blue * background.alpha * (1.0 - self.alpha))
                * scale,
            alpha,
        }
    }

    pub fn luminance(self) -> f32 {
        0.2126 * self.red + 0.7152 * self.green + 0.0722 * self.blue
    }

    pub fn saturate(self, amount: f32) -> Self {
        let luminance = self.luminance();
        Self {
            red: luminance + (self.red - luminance) * amount,
            green: luminance + (self.green - luminance) * amount,
            blue: luminance + (self.blue - luminance) * amount,
            alpha: self.alpha,
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    pub fn from_rgb_f32(rgb: [f32; 3]) -> Self {
        Self::rgb(rgb[0], rgb[1], rgb[2])
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}
