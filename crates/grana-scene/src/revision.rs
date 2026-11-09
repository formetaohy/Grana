#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Self = Self(0);
    pub const FIRST: Self = Self(1);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn next(self) -> Self {
        let raw = self.0.wrapping_add(1);
        Self(if raw == 0 { 1 } else { raw })
    }
}

#[derive(Default)]
pub struct Revisions {
    held: Revision,
}

impl Revisions {
    pub const fn new() -> Self {
        Self {
            held: Revision::ZERO,
        }
    }

    pub fn mint(&mut self) -> Revision {
        self.held = self.held.next();
        self.held
    }

    pub fn current(&self) -> Revision {
        self.held
    }
}
