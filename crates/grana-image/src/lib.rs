mod align;
mod error;
mod fingerprint;
mod image;
mod metric;
mod reader;
mod region;
mod series;
mod slice;
mod source;

pub use error::Error;
pub use fingerprint::FINGERPRINT_COVERAGE;
pub use grana_zarr::{Decode, Field, Label, Labels, Plate, Transfer, Well};
pub use image::Image;
pub use metric::Metric;
pub use reader::{
    Format, dataset_name, is_image, is_remote, plate, plate_at, remote_series_group,
    resolve_series, series_count, series_group, series_names,
};
pub use series::Series;
pub use source::Source;

pub fn cache_root() -> Option<std::path::PathBuf> {
    slice::default_root()
}

pub fn clear_cache() -> std::io::Result<u64> {
    cache_root().map_or(Ok(0), |root| slice::clear(&root))
}

pub fn clear_cache_at(root: &std::path::Path) -> std::io::Result<u64> {
    slice::clear(root)
}
