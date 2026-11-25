use grana_object::{ObjectId, RelationFrame};

use crate::column::{Column, Unit, area_key, length_key};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelationField {
    Distance,
    Inside,
    Partner,
    Contact,
    Center,
    Overlapping,
}

impl RelationField {
    pub const ALL: [Self; 6] = [
        Self::Distance,
        Self::Inside,
        Self::Partner,
        Self::Contact,
        Self::Center,
        Self::Overlapping,
    ];

    pub const CONTINUOUS: [Self; 5] = [
        Self::Distance,
        Self::Inside,
        Self::Contact,
        Self::Center,
        Self::Overlapping,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Distance => 0,
            Self::Inside => 1,
            Self::Partner => 2,
            Self::Contact => 3,
            Self::Center => 4,
            Self::Overlapping => 5,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Center => "center distance",
            Self::Inside => "inside",
            Self::Partner => "partner",
            Self::Contact => "contact",
            Self::Overlapping => "partners",
        }
    }

    pub fn key(self) -> String {
        match self {
            Self::Distance => length_key("distance"),
            Self::Center => length_key("center"),
            Self::Inside => "inside".to_string(),
            Self::Partner => "partner".to_string(),
            Self::Contact => area_key("contact"),
            Self::Overlapping => "partners".to_string(),
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Distance | Self::Center => Unit::Micrometer,
            Self::Inside => Unit::Percent,
            Self::Partner | Self::Overlapping => Unit::Count,
            Self::Contact => Unit::SquareMicrometer,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Distance | Self::Center => 3,
            Self::Inside => 2,
            Self::Partner | Self::Overlapping => 0,
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

    pub fn value(self, row: RelationRow<'_>, source: ObjectId) -> f32 {
        let Some(frame) = row.frame() else {
            return f32::INFINITY;
        };
        self.measure(frame, source)
    }

    pub fn measure(self, frame: &RelationFrame, source: ObjectId) -> f32 {
        let Some(reading) = frame.reading(source) else {
            return f32::INFINITY;
        };
        if !reading.distance().is_finite() {
            return f32::INFINITY;
        }
        match self {
            Self::Distance => reading.distance(),
            Self::Center => reading.center(),
            Self::Inside => 100.0 * reading.inside(),
            Self::Partner => reading
                .partner()
                .map_or(f32::INFINITY, |id| id.raw() as f32),
            Self::Contact => reading.contact(),
            Self::Overlapping => reading.overlapping() as f32,
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
