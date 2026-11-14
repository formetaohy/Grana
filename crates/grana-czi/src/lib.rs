mod binary;
mod codec;
mod directory;
mod error;
mod header;
mod image;
mod layout;
mod metadata;
mod plane;
mod segment;
mod store;

pub use error::Error;
pub use image::{Image, Scene, dataset_name, scene_count, scenes};

pub(crate) type Cache = grana_store::LruCache<u64, Vec<u8>>;

pub(crate) fn cache(limit: u64) -> Cache {
    Cache::new(limit, |data| data.len() as u64)
}
