#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn bytes(self) -> [u8; 32] {
        self.0
    }
}
