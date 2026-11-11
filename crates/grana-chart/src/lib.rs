mod axis;
mod chart;
mod comparison;
mod decor;
mod distribution;
mod grid;
mod histogram;
mod joint;
mod layout;
mod scatter;
mod series;
mod sheet;
mod style;

pub use axis::{Axis, decimals, number, step};
pub use chart::{Chart, HEIGHT, Heading, Page, View, WIDTH};
pub use comparison::{Category, Comparison, Note as ComparisonNote};
pub use decor::{Brush, BrushKind, Decor, Gates, Highlight};
pub use distribution::paint as distribution;
pub use histogram::paint as histogram;
pub use joint::Joint;
pub use layout::{Insets, cell_centre, cell_size, group_at, group_centre, slot};
pub use scatter::paint as scatter;
pub use series::{
    Band, BandPoint, Curve, CurvePoint, Note, SeriesChart, SeriesPage, paint as series,
};
pub use sheet::Sheet;
pub use style::{DistributionStyle, Metrics, Style};
