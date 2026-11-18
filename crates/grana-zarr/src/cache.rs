use std::sync::Arc;

use zarrs::array::chunk_cache::{
    ChunkCache, ChunkCacheLruSizeLimit, ChunkCacheTypeDecoded, ChunkCacheTypePartialDecoder,
};
use zarrs::array::{Array, ArrayError, ArrayShardedExt, ArraySubset, CodecOptions, ElementOwned};
use zarrs::storage::ReadableStorageTraits;

const BUDGET: u64 = 128 << 20;
const MINIMUM: u64 = 4 << 20;

pub(crate) enum Cache {
    Partial(ChunkCacheLruSizeLimit<ChunkCacheTypePartialDecoder>),
    Decoded(ChunkCacheLruSizeLimit<ChunkCacheTypeDecoded>),
}

pub(crate) fn per_level(levels: usize) -> u64 {
    (BUDGET / levels.max(1) as u64).max(MINIMUM)
}

impl Cache {
    pub(crate) fn open(array: Arc<Array<dyn ReadableStorageTraits>>, bytes: u64) -> Self {
        if array.is_sharded() {
            Self::Partial(ChunkCacheLruSizeLimit::new(array, bytes))
        } else {
            Self::Decoded(ChunkCacheLruSizeLimit::new(array, bytes))
        }
    }

    pub(crate) fn array(&self) -> Arc<Array<dyn ReadableStorageTraits>> {
        match self {
            Self::Partial(cache) => cache.array(),
            Self::Decoded(cache) => cache.array(),
        }
    }

    pub(crate) fn retrieve<T: ElementOwned>(
        &self,
        subset: &ArraySubset,
        options: &CodecOptions,
    ) -> Result<Vec<T>, ArrayError> {
        match self {
            Self::Partial(cache) => cache.retrieve_array_subset(subset, options),
            Self::Decoded(cache) => cache.retrieve_array_subset(subset, options),
        }
    }
}
