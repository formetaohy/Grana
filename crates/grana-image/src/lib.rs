mod align;
mod cache;
mod error;
mod fingerprint;
mod image;
mod level;
mod region;
mod series;
mod source;

pub use cache::default_root as cache_root;
pub use error::Error;
pub use grana_zarr::{Label, Labels};
pub use image::{
    Format, Image, dataset_name, is_image, is_remote, remote_series_group, resolve_series,
    series_count, series_group, series_names,
};
pub use series::Series;
pub use source::Source;

pub fn clear_cache() -> std::io::Result<u64> {
    cache::default_root().map_or(Ok(0), |root| cache::clear(&root))
}

pub fn clear_cache_at(root: &std::path::Path) -> std::io::Result<u64> {
    cache::clear(root)
}
