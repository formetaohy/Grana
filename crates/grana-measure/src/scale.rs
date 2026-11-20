use crate::column::Unit;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scale {
    Physical,
    #[default]
    Pixel,
}

impl Scale {
    pub const ALL: [Self; 2] = [Self::Physical, Self::Pixel];

    pub fn calibrated(self) -> bool {
        self == Self::Physical
    }

    pub fn length_key(self, name: &str) -> String {
        format!("{name}_{}", self.length_suffix())
    }

    pub fn area_key(self, name: &str) -> String {
        format!("{name}_{}", self.area_suffix())
    }

    pub fn volume_key(self, name: &str) -> String {
        format!("{name}_{}", self.volume_suffix())
    }

    pub fn speed_key(self, name: &str) -> String {
        format!("{name}_{}", self.speed_suffix())
    }

    pub fn length(self) -> Unit {
        match self {
            Self::Physical => Unit::Micrometer,
            Self::Pixel => Unit::Pixel,
        }
    }

    pub fn area(self) -> Unit {
        match self {
            Self::Physical => Unit::SquareMicrometer,
            Self::Pixel => Unit::SquarePixel,
        }
    }

    pub fn volume(self) -> Unit {
        match self {
            Self::Physical => Unit::CubicMicrometer,
            Self::Pixel => Unit::CubicPixel,
        }
    }

    pub fn speed(self) -> Unit {
        match self {
            Self::Physical => Unit::MicrometerPerSecond,
            Self::Pixel => Unit::PixelPerSecond,
        }
    }

    fn length_suffix(self) -> &'static str {
        match self {
            Self::Physical => "um",
            Self::Pixel => "px",
        }
    }

    fn area_suffix(self) -> &'static str {
        match self {
            Self::Physical => "um2",
            Self::Pixel => "px2",
        }
    }

    fn volume_suffix(self) -> &'static str {
        match self {
            Self::Physical => "um3",
            Self::Pixel => "px3",
        }
    }

    fn speed_suffix(self) -> &'static str {
        match self {
            Self::Physical => "um_s",
            Self::Pixel => "px_s",
        }
    }
}
