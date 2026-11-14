use crate::child::Child;
use crate::metric::Measurement;
use crate::relation::RelationRow;
use crate::table::Objects;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spots,
    Surfaces,
    Filaments,
    Cells,
}

impl Kind {
    pub fn measurements(self, channels: usize) -> Vec<Measurement> {
        match self {
            Self::Spots => Measurement::spots(channels),
            Self::Surfaces => Measurement::surfaces(channels),
            Self::Filaments => Measurement::filaments(channels),
            Self::Cells => Measurement::cells(channels),
        }
    }
}

pub struct Frame<'a> {
    time: u32,
    objects: Objects<'a>,
    relations: &'a [RelationRow<'a>],
    children: &'a [Child<'a>],
}

impl<'a> Frame<'a> {
    pub fn new(
        time: u32,
        objects: Objects<'a>,
        relations: &'a [RelationRow<'a>],
        children: &'a [Child<'a>],
    ) -> Self {
        Self {
            time,
            objects,
            relations,
            children,
        }
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn objects(&self) -> Objects<'a> {
        self.objects
    }

    pub fn relations(&self) -> &'a [RelationRow<'a>] {
        self.relations
    }

    pub fn children(&self) -> &'a [Child<'a>] {
        self.children
    }
}

pub trait FrameSource: Send + Sync {
    fn frames(&self) -> u32;

    fn kind(&self) -> Kind;

    fn channels(&self) -> &[String];

    fn relations(&self) -> Vec<RelationRow<'_>>;

    fn children(&self) -> Vec<Child<'_>>;

    fn visit(&self, time: u32, visit: &mut dyn FnMut(Frame<'_>));

    fn visit_all(&self, visit: &mut dyn FnMut(Frame<'_>)) {
        for time in 0..self.frames() {
            self.visit(time, visit);
        }
    }
}
