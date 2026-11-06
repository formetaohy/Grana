#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Nikon,
    Leica,
}

impl Family {
    pub(crate) fn of(id: &str) -> Option<Self> {
        match id {
            openreadout_nd2::FORMAT_ID => Some(Self::Nikon),
            openreadout_lif::FORMAT_ID => Some(Self::Leica),
            _ => None,
        }
    }
}
