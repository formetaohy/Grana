mod bins;
mod distribution;
mod histogram;
mod scatter;
mod summary;
mod trend;

use crate::mask::Mask;
use crate::spatial::{NearestNeighbour, Ripley, Window};
use crate::table::Table;

pub use distribution::{DENSITY_POINTS, Distribution, DistributionGroup, DistributionSeries};
pub use histogram::Histogram;
pub use scatter::Scatter;
pub use summary::Summary;
pub use trend::{Population, Trend, TrendPoint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureView {
    Table,
    Histogram,
    Scatter,
    Distribution,
    Trend,
    Ripley,
    NearestNeighbour,
}

impl MeasureView {
    pub const ALL: [Self; 7] = [
        Self::Table,
        Self::Histogram,
        Self::Scatter,
        Self::Distribution,
        Self::Trend,
        Self::Ripley,
        Self::NearestNeighbour,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Table => 0,
            Self::Histogram => 1,
            Self::Scatter => 2,
            Self::Distribution => 3,
            Self::Trend => 4,
            Self::Ripley => 5,
            Self::NearestNeighbour => 6,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Histogram => "histogram",
            Self::Scatter => "scatter",
            Self::Distribution => "distribution",
            Self::Trend => "trend",
            Self::Ripley => "ripley",
            Self::NearestNeighbour => "nearest-neighbour",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.name() == key)
    }

    pub fn plot(self) -> Option<Self> {
        match self {
            Self::Table => None,
            Self::Histogram
            | Self::Scatter
            | Self::Distribution
            | Self::Trend
            | Self::Ripley
            | Self::NearestNeighbour => Some(self),
        }
    }

    pub fn spatial(self) -> bool {
        matches!(self, Self::Ripley | Self::NearestNeighbour)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotAxis {
    X,
    Y,
}

impl PlotAxis {
    pub const ALL: [Self; 2] = [Self::X, Self::Y];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plot {
    Histogram(Histogram),
    Scatter(Scatter),
    Distribution(Distribution),
    Trend(Trend),
    Ripley(Ripley),
    NearestNeighbour(NearestNeighbour),
}

impl Plot {
    pub fn view(&self) -> MeasureView {
        match self {
            Self::Histogram(_) => MeasureView::Histogram,
            Self::Scatter(_) => MeasureView::Scatter,
            Self::Distribution(_) => MeasureView::Distribution,
            Self::Trend(_) => MeasureView::Trend,
            Self::Ripley(_) => MeasureView::Ripley,
            Self::NearestNeighbour(_) => MeasureView::NearestNeighbour,
        }
    }
}

impl Table {
    pub fn plot(
        &self,
        view: MeasureView,
        axes: [usize; 2],
        gated: &Mask,
        window: Window,
        paired: Option<&Table>,
    ) -> Option<Plot> {
        let columns = self.columns().len();
        assert!(
            axes.iter().all(|axis| *axis < columns),
            "a plot names columns of its table"
        );
        assert_eq!(
            gated.rows(),
            self.rows(),
            "a plot gates the rows of its table"
        );
        match view {
            MeasureView::Table => None,
            MeasureView::Histogram => Some(Plot::Histogram(self.histogram(axes[0], gated))),
            MeasureView::Scatter => Some(Plot::Scatter(self.scatter(axes[0], axes[1], gated))),
            MeasureView::Distribution => {
                Some(Plot::Distribution(self.distribution(axes[0], gated)))
            }
            MeasureView::Trend => Some(Plot::Trend(self.trend(axes[0], gated))),
            MeasureView::Ripley => {
                let source = self.gated_points(gated);
                let target = paired.map(|table| table.gated_points(&Mask::all(table.rows())));
                Some(Plot::Ripley(crate::spatial::ripley(
                    &source,
                    target.as_deref(),
                    window,
                )?))
            }
            MeasureView::NearestNeighbour => {
                let source = self.gated_points(gated);
                let target = paired.map(|table| table.gated_points(&Mask::all(table.rows())));
                Some(Plot::NearestNeighbour(crate::spatial::nearest_neighbour(
                    &source,
                    target.as_deref(),
                    window,
                )?))
            }
        }
    }
}

pub(crate) fn extent(table: &Table, column: usize) -> (f32, f32) {
    let Some((low, high)) = table.extent(column) else {
        return (0.0, 1.0);
    };
    if high > low {
        return (low, high);
    }
    let padding = (0.5 * low.abs()).max(0.5);
    (low - padding, high + padding)
}
