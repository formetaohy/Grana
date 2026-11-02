#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Current,
}

impl Scope {
    pub const ALL: [Self; 2] = [Self::All, Self::Current];

    pub fn index(self) -> u8 {
        match self {
            Self::All => 0,
            Self::Current => 1,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Current => "current",
        }
    }
}
