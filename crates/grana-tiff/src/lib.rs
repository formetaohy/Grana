mod archive;
mod codec;
mod color;
mod directory;
mod error;
mod image;
mod layout;
mod page;
mod planes;
mod sample;
mod store;
mod tag;

pub use archive::write_archive;
pub use error::Error;
pub use image::{Image, Series, dataset_name, series_count, series_names};

pub(crate) type Cache =
    grana_store::LruCache<(usize, usize, usize, u32, usize), grana_volume::VoxelData>;

pub(crate) fn cache(limit: u64) -> Cache {
    Cache::new(limit, grana_volume::VoxelData::bytes)
}
