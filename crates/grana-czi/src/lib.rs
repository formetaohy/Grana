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

use std::sync::OnceLock;

use grana_store::Pool;

pub use error::Error;
pub use image::{Image, Scene, dataset_name, scene_count, scenes};

pub(crate) type Cache = grana_store::LruCache<(u64, u64), Vec<u8>>;

pub(crate) fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Cache::pooled(Pool::Region, |samples: &Vec<u8>| samples.len() as u64))
}
