use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelVersion {
    Cpsam,
    CpsamV2,
}

impl ModelVersion {
    pub const DEFAULT: Self = Self::CpsamV2;

    pub fn file(self) -> &'static str {
        match self {
            Self::Cpsam => "cpsam",
            Self::CpsamV2 => "cpsam_v2",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::Cpsam => "https://huggingface.co/mouseland/cellpose-sam/resolve/main/cpsam",
            Self::CpsamV2 => "https://huggingface.co/mouseland/cellpose-sam/resolve/main/cpsam_v2",
        }
    }

    pub fn length(self) -> u64 {
        match self {
            Self::Cpsam => 1_233_587_898,
            Self::CpsamV2 => 1_233_586_851,
        }
    }

    pub fn stored_length(self) -> u64 {
        match self {
            Self::Cpsam => 1_233_587_822,
            Self::CpsamV2 => 1_233_586_775,
        }
    }

    pub fn checksum(self) -> &'static str {
        match self {
            Self::Cpsam => "e1440429eb384f95afe32bcba6510f90d518eaedc917ede549bed6804004abe2",
            Self::CpsamV2 => "0f1cc3f7ecdd8a037a57c6c48d9d8921391be4cbce3fa9f13c3e3a2e1253c667",
        }
    }
}

pub(crate) fn matches(version: ModelVersion, path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| {
        metadata.len() == version.length() || metadata.len() == version.stored_length()
    })
}
