const SEED: u32 = 0xffff_ffff;
const POLYNOMIAL: u32 = 0xedb8_8320;

const TABLE: [u32; 256] = table();

pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self(SEED)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0 = bytes.iter().fold(self.0, |state, byte| {
            TABLE[((state ^ u32::from(*byte)) & 0xff) as usize] ^ (state >> 8)
        });
    }

    pub fn finish(self) -> u32 {
        self.0 ^ SEED
    }
}

const fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                POLYNOMIAL ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}
