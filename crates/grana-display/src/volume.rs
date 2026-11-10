#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VolumeMode {
    #[default]
    Blend,
    Maximum,
    Isosurface,
}

impl VolumeMode {
    pub const ALL: [Self; 3] = [Self::Blend, Self::Maximum, Self::Isosurface];

    pub fn name(self) -> &'static str {
        match self {
            Self::Blend => "blend",
            Self::Maximum => "maximum",
            Self::Isosurface => "isosurface",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Blend => "Blend",
            Self::Maximum => "MIP",
            Self::Isosurface => "Iso",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Blend => 0,
            Self::Maximum => 1,
            Self::Isosurface => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeStyle {
    mode: VolumeMode,
    isovalue: f32,
}

impl VolumeStyle {
    pub const DEFAULT: Self = Self {
        mode: VolumeMode::Blend,
        isovalue: 0.5,
    };

    pub fn new(mode: VolumeMode, isovalue: f32) -> Self {
        assert!(
            isovalue.is_finite() && (0.0..=1.0).contains(&isovalue),
            "an isovalue lies in the unit interval"
        );
        Self { mode, isovalue }
    }

    pub fn mode(self) -> VolumeMode {
        self.mode
    }

    pub fn isovalue(self) -> f32 {
        self.isovalue
    }

    pub fn with_mode(self, mode: VolumeMode) -> Self {
        Self { mode, ..self }
    }

    pub fn with_isovalue(self, isovalue: f32) -> Self {
        Self::new(self.mode, isovalue)
    }
}

impl Default for VolumeStyle {
    fn default() -> Self {
        Self::DEFAULT
    }
}
