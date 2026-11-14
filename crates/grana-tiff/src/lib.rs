mod archive;
mod codec;
mod error;
mod image;
mod ome;
mod page;
mod planes;
mod store;
mod tag;
mod write;

pub use archive::write_archive;
pub use error::Error;
pub use image::{Image, dataset_name};
pub use write::{Picture, Stack, Stream, write, write_stack, write_stream};

pub(crate) type Cache = grana_store::LruCache<(usize, usize, usize, usize), Vec<u8>>;

pub(crate) fn cache(limit: u64) -> Cache {
    Cache::new(limit, |data| data.len() as u64)
}
