#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Algorithm {
    Detection,
    Segmentation,
    Import,
    Interop,
    Attachment,
    Filter,
    Copy,
    Edit,
    Mask,
    Merge,
    Tracking,
    Colocalization,
}

const DETECTION: u32 = 1;
const SEGMENTATION: u32 = 1;
const IMPORT: u32 = 1;
const INTEROP: u32 = 1;
const ATTACHMENT: u32 = 1;
const FILTER: u32 = 1;
const COPY: u32 = 1;
const EDIT: u32 = 1;
const MASK: u32 = 1;
const MERGE: u32 = 1;
const TRACKING: u32 = 1;
const COLOCALIZATION: u32 = 1;

impl Algorithm {
    pub const ALL: [Self; 12] = [
        Self::Detection,
        Self::Segmentation,
        Self::Import,
        Self::Interop,
        Self::Attachment,
        Self::Filter,
        Self::Copy,
        Self::Edit,
        Self::Mask,
        Self::Merge,
        Self::Tracking,
        Self::Colocalization,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Detection => "detection",
            Self::Segmentation => "segmentation",
            Self::Import => "import",
            Self::Interop => "interop",
            Self::Attachment => "attachment",
            Self::Filter => "filter",
            Self::Copy => "copy",
            Self::Edit => "edit",
            Self::Mask => "mask",
            Self::Merge => "merge",
            Self::Tracking => "tracking",
            Self::Colocalization => "colocalization",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Detection => "Detection",
            Self::Segmentation => "Segmentation",
            Self::Import => "Label import",
            Self::Interop => "Interop import",
            Self::Attachment => "Intensity attachment",
            Self::Filter => "Filter",
            Self::Copy => "Copy",
            Self::Edit => "Surface edit",
            Self::Mask => "Mask paint",
            Self::Merge => "Merge",
            Self::Tracking => "Tracking",
            Self::Colocalization => "Colocalization",
        }
    }

    pub fn version(self) -> u32 {
        match self {
            Self::Detection => DETECTION,
            Self::Segmentation => SEGMENTATION,
            Self::Import => IMPORT,
            Self::Interop => INTEROP,
            Self::Attachment => ATTACHMENT,
            Self::Filter => FILTER,
            Self::Copy => COPY,
            Self::Edit => EDIT,
            Self::Mask => MASK,
            Self::Merge => MERGE,
            Self::Tracking => TRACKING,
            Self::Colocalization => COLOCALIZATION,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}
