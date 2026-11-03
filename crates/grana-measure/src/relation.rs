use grana_object::RelationFrame;

use crate::column::{Column, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationField {
    Distance,
    Inside,
    Partner,
}

impl RelationField {
    pub const ALL: [Self; 3] = [Self::Distance, Self::Inside, Self::Partner];

    pub fn index(self) -> u8 {
        match self {
            Self::Distance => 0,
            Self::Inside => 1,
            Self::Partner => 2,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Inside => "inside",
            Self::Partner => "partner",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Distance => "distance_um",
            Self::Inside => "inside",
            Self::Partner => "partner",
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Distance => Unit::Micrometer,
            Self::Inside => Unit::Percent,
            Self::Partner => Unit::Count,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Distance => 3,
            Self::Inside => 2,
            Self::Partner => 0,
        }
    }

    pub fn column(self, target: &str, key: &str, shift: i32) -> Column {
        let label = if shift == 0 {
            format!("{target} {}", self.label())
        } else {
            format!("{target} {} · t{shift}", self.label())
        };
        Column::new(
            label,
            format!("{key}_{}", self.key()),
            self.unit(),
            self.decimals(),
        )
    }

    pub fn value(self, row: RelationRow<'_>, object: usize) -> f32 {
        let Some(frame) = row.frame() else {
            return self.missing();
        };
        assert!(
            object < frame.objects(),
            "the relation measures the object it is read from"
        );
        match self {
            Self::Distance => frame.distance(object),
            Self::Inside => 100.0 * frame.inside(object),
            Self::Partner => frame.partner(object) as f32,
        }
    }

    fn missing(self) -> f32 {
        match self {
            Self::Distance => f32::INFINITY,
            Self::Inside | Self::Partner => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RelationRow<'a> {
    target: &'a str,
    shift: i32,
    frame: Option<&'a RelationFrame>,
}

impl<'a> RelationRow<'a> {
    pub fn new(target: &'a str, shift: i32, frame: Option<&'a RelationFrame>) -> Self {
        Self {
            target,
            shift,
            frame,
        }
    }

    pub fn target(self) -> &'a str {
        self.target
    }

    pub fn shift(self) -> i32 {
        self.shift
    }

    pub fn frame(self) -> Option<&'a RelationFrame> {
        self.frame
    }
}
