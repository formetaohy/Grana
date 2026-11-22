#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub started: u64,
    pub delivered: u64,
}
