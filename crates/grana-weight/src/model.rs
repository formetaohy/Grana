pub use grana_model::{License, Model as ModelVersion};

pub(crate) struct Asset {
    pub(crate) version: ModelVersion,
    pub(crate) url: &'static str,
    pub(crate) length: u64,
    pub(crate) stored_length: u64,
    pub(crate) checksum: &'static str,
}

const CATALOG: &[Asset] = &[
    Asset {
        version: ModelVersion::Cpsam,
        url: "https://huggingface.co/mouseland/cellpose-sam/resolve/main/cpsam",
        length: 1_233_587_898,
        stored_length: 1_233_587_822,
        checksum: "e1440429eb384f95afe32bcba6510f90d518eaedc917ede549bed6804004abe2",
    },
    Asset {
        version: ModelVersion::CpsamV2,
        url: "https://huggingface.co/mouseland/cellpose-sam/resolve/main/cpsam_v2",
        length: 1_233_586_851,
        stored_length: 1_233_586_775,
        checksum: "0f1cc3f7ecdd8a037a57c6c48d9d8921391be4cbce3fa9f13c3e3a2e1253c667",
    },
];

pub(crate) fn asset(version: ModelVersion) -> Option<&'static Asset> {
    version
        .licensed()
        .then(|| CATALOG.iter().find(|asset| asset.version == version))
        .flatten()
}

pub fn license(version: ModelVersion) -> Option<License> {
    asset(version).map(|asset| asset.version.license())
}
