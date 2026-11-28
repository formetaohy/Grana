#[derive(Clone, Copy, Debug)]
pub struct Digest(u64);

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest {
    const OFFSET: u64 = 0x6a09_e667_f3bc_c909;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn mix(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(Self::PRIME).rotate_left(23);
    }

    pub fn real(&mut self, value: f32) {
        self.mix(u64::from(value.to_bits()));
    }

    pub fn flag(&mut self, value: bool) {
        self.mix(u64::from(value));
    }

    pub fn finish(self) -> u64 {
        let value = self.0 ^ (self.0 >> 31);
        value.wrapping_mul(0xff51_afd7_ed55_8ccd) ^ (value >> 29)
    }
}
