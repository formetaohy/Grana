use std::path::Path;

pub(crate) const FILE: &str = "cpsam";
pub(crate) const URL: &str = "https://huggingface.co/mouseland/cellpose-sam/resolve/main/cpsam";
pub(crate) const LENGTH: u64 = 1_233_587_898;
pub(crate) const CHECKSUM: &str =
    "e1440429eb384f95afe32bcba6510f90d518eaedc917ede549bed6804004abe2";

pub(crate) fn matches(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.len() == LENGTH)
}
