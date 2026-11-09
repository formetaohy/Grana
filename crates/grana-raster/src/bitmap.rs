use std::sync::Arc;

use crate::Raster;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    size: [u32; 2],
    pixels: Arc<[u8]>,
    identity: u64,
}

impl Bitmap {
    pub fn new(size: [u32; 2], pixels: Arc<[u8]>) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a bitmap covers at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            size[0] as usize * size[1] as usize * 4,
            "a bitmap holds four bytes per pixel"
        );
        let identity = identity(size, &pixels);
        Self {
            size,
            pixels,
            identity,
        }
    }

    pub fn from_raster(raster: &Raster) -> Self {
        Self::new(raster.size(), Arc::from(raster.pixels()))
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn width(&self) -> u32 {
        self.size[0]
    }

    pub fn height(&self) -> u32 {
        self.size[1]
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn identity(&self) -> u64 {
        self.identity
    }

    pub fn raster(&self) -> Raster {
        Raster::new(self.size[0], self.size[1], self.pixels.to_vec())
    }
}

fn identity(size: [u32; 2], pixels: &[u8]) -> u64 {
    let mut hash = OFFSET;
    for value in size {
        hash ^= u64::from(value);
        hash = hash.wrapping_mul(PRIME);
    }
    for chunk in pixels.chunks(8) {
        let mut word = 0u64;
        for (index, byte) in chunk.iter().enumerate() {
            word |= u64::from(*byte) << (8 * index);
        }
        hash ^= word;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}
