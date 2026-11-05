use grana_object::RelationFrame;

use crate::column::{Column, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationField {
    Distance,
    Inside,
    Partner,
    Contact,
}

impl RelationField {
    pub const ALL: [Self; 4] = [Self::Distance, Self::Inside, Self::Partner, Self::Contact];

    pub const CONTINUOUS: [Self; 3] = [Self::Distance, Self::Inside, Self::Contact];

    pub fn index(self) -> u8 {
        match self {
            Self::Distance => 0,
            Self::Inside => 1,
            Self::Partner => 2,
            Self::Contact => 3,
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
            Self::Contact => "contact",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Distance => "distance_um",
            Self::Inside => "inside",
            Self::Partner => "partner",
            Self::Contact => "contact_um2",
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Distance => Unit::Micrometer,
            Self::Inside => Unit::Percent,
            Self::Partner => Unit::Count,
            Self::Contact => Unit::SquareMicrometer,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Distance => 3,
            Self::Inside => 2,
            Self::Partner => 0,
            Self::Contact => 3,
        }
    }

    pub fn column(self, key: &str, target: &str, shift: i32) -> Column {
        let label = if shift == 0 {
            format!("{target} {}", self.label())
        } else {
            format!("{target} {} · t{shift:+}", self.label())
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
        self.measure(frame, object)
    }

    pub fn measure(self, frame: &RelationFrame, object: usize) -> f32 {
        assert!(
            object < frame.objects(),
            "the relation measures the object it is read from"
        );
        match self {
            Self::Distance => frame.distance(object),
            Self::Inside => 100.0 * frame.inside(object),
            Self::Partner => frame.partner(object) as f32,
            Self::Contact => frame.contact(object),
        }
    }

    fn missing(self) -> f32 {
        match self {
            Self::Distance => f32::INFINITY,
            Self::Inside | Self::Partner | Self::Contact => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RelationRow<'a> {
    key: &'a str,
    target: &'a str,
    shift: i32,
    frame: Option<&'a RelationFrame>,
}

impl<'a> RelationRow<'a> {
    pub fn new(
        key: &'a str,
        target: &'a str,
        shift: i32,
        frame: Option<&'a RelationFrame>,
    ) -> Self {
        assert!(
            !key.trim().is_empty(),
            "a relation row carries a stable key"
        );
        Self {
            key,
            target,
            shift,
            frame,
        }
    }

    pub fn key(self) -> &'a str {
        self.key
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

pub fn relation_keys(key: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for field in RelationField::ALL {
        let base = format!("{key}_{}", field.key());
        keys.push(base.clone());
        for aggregate in crate::table::Aggregate::ALL {
            keys.push(format!("{base}_{}", aggregate.key()));
        }
    }
    keys
}
