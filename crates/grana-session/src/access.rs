use crate::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Author,
    Viewer,
}

impl Access {
    pub fn author(self) -> bool {
        matches!(self, Self::Author)
    }

    pub fn viewer(self) -> bool {
        matches!(self, Self::Viewer)
    }

    pub fn permits(self, command: &Command) -> bool {
        self.author() || !command.authors()
    }
}
