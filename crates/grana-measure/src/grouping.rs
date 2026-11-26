use crate::classifier::SCORE_KEY;
use crate::column::{Column, Unit};
use crate::measured::Measured;
use crate::metric::Measurement;
use crate::sample::Sample;
use crate::table::Table;

pub const CLASS_KEY: &str = "class";
pub const CLASS_LABEL: &str = "class";
pub const CLASS_MINIMUM: u32 = 2;
pub const CLASS_MAXIMUM: u32 = 64;

pub fn class_column() -> Column {
    Column::new(CLASS_LABEL, CLASS_KEY, Unit::Count, 0).possibly_absent()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Grouping {
    Threshold(Threshold),
    Quantile(Quantile),
}

impl Default for Grouping {
    fn default() -> Self {
        Self::Threshold(Threshold::DEFAULT)
    }
}

impl Grouping {
    pub fn column(&self) -> Column {
        class_column()
    }

    pub fn codes(&self, table: &Table, keys: &[String]) -> Option<Vec<Measured>> {
        let source = match self {
            Self::Threshold(_) => SCORE_KEY.to_string(),
            Self::Quantile(quantile) => quantile.measurement.key(keys),
        };
        let values = table.materialize(table.column(&source)?);
        Some(match self {
            Self::Threshold(threshold) => values
                .iter()
                .map(|value| {
                    value
                        .present()
                        .map_or(Measured::ABSENT, |score| threshold.measured(score))
                })
                .collect(),
            Self::Quantile(quantile) => quantile.codes(&values)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Threshold(f32);

impl Threshold {
    pub const DEFAULT: Self = Self(0.5);

    pub fn new(value: f32) -> Option<Self> {
        (value.is_finite() && 0.0 < value && value < 1.0).then_some(Self(value))
    }

    pub fn value(self) -> f32 {
        self.0
    }

    fn measured(self, score: f32) -> Measured {
        Measured::new(if score >= self.0 { 1.0 } else { 0.0 }, 1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quantile {
    measurement: Measurement,
    classes: u32,
}

impl Quantile {
    pub fn new(measurement: Measurement, classes: u32) -> Option<Self> {
        (CLASS_MINIMUM..=CLASS_MAXIMUM)
            .contains(&classes)
            .then_some(Self {
                measurement,
                classes,
            })
    }

    pub fn measurement(&self) -> Measurement {
        self.measurement
    }

    pub fn classes(&self) -> u32 {
        self.classes
    }

    fn codes(&self, values: &[Measured]) -> Option<Vec<Measured>> {
        let sample = Sample::new(
            values
                .iter()
                .filter_map(|value| value.present())
                .map(f64::from)
                .collect(),
        )?;
        let classes = f64::from(self.classes);
        let edges = (1..self.classes)
            .map(|order| sample.quantile(100.0 * f64::from(order) / classes))
            .collect::<Vec<_>>();
        Some(
            values
                .iter()
                .map(|value| {
                    value.present().map_or(Measured::ABSENT, |value| {
                        let code = edges.partition_point(|edge| *edge < f64::from(value));
                        Measured::new(code as f32, 1)
                    })
                })
                .collect(),
        )
    }
}
