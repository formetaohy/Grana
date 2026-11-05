use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0xcbf2_9ce4_8422_2325;
const SPREAD: u64 = 0x517c_c1b7_2722_0a95;

pub(crate) type Map<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

#[derive(Clone, Copy)]
pub(crate) struct FastHasher(u64);

impl Default for FastHasher {
    fn default() -> Self {
        Self(SEED)
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*byte)).wrapping_mul(SPREAD);
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(SPREAD);
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn finish(&self) -> u64 {
        let mut value = self.0;
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        value ^ (value >> 33)
    }
}
