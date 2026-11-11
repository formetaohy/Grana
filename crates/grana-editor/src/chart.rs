use grana_chart::{Chart, DistributionStyle, Gates, Heading};
use grana_measure::{Plot, Table};
use grana_session::SessionView;

use crate::view::column_label;

pub fn page(view: &SessionView<'_>, distribution: DistributionStyle) -> Option<Chart> {
    let measure = view.measure;
    let table = measure.table()?;
    let plot = measure.plot()?.clone();
    let columns = table.columns();
    let (title, y, x) = match &plot {
        Plot::Histogram(histogram) => (
            format!(
                "Histogram of {}",
                column_label(&columns[histogram.column()])
            ),
            "Y: Objects".to_string(),
            format!("X: {}", column_label(&columns[histogram.column()])),
        ),
        Plot::Scatter(scatter) => (
            format!(
                "{} against {}",
                column_label(&columns[scatter.y()]),
                column_label(&columns[scatter.x()])
            ),
            format!("Y: {}", column_label(&columns[scatter.y()])),
            format!("X: {}", column_label(&columns[scatter.x()])),
        ),
        Plot::Distribution(distribution) => (
            format!(
                "Distribution of {}",
                column_label(&columns[distribution.column()])
            ),
            format!("Y: {}", column_label(&columns[distribution.column()])),
            "X: Groups".to_string(),
        ),
    };
    let gate = match &plot {
        Plot::Histogram(histogram) => Gates {
            x: range(measure.filters(), histogram.column()),
            y: None,
        },
        Plot::Scatter(scatter) => Gates {
            x: range(measure.filters(), scatter.x()),
            y: range(measure.filters(), scatter.y()),
        },
        Plot::Distribution(distribution) => Gates {
            x: range(measure.filters(), distribution.column()),
            y: None,
        },
    };
    Some(Chart {
        heading: Heading::new(title, y, x).with_summary(caption(&plot, table)),
        plot,
        gate,
        highlight: None,
        distribution,
    })
}

fn range(filters: &[grana_measure::Filter], column: usize) -> Option<(f32, f32)> {
    filters
        .iter()
        .find(|filter| filter.column() == column)
        .map(|filter| (filter.minimum(), filter.maximum()))
}

fn caption(plot: &Plot, table: &Table) -> String {
    let total = table.rows();
    match plot {
        Plot::Histogram(histogram) => {
            let gated = histogram.summary().count() as usize;
            if gated < total {
                format!("{gated} of {total} objects gated")
            } else {
                format!("{total} objects")
            }
        }
        Plot::Scatter(scatter) => {
            let gated = scatter.x_summary().count() as usize;
            let correlation = scatter
                .correlation()
                .map_or_else(|| "r —".to_string(), |value| format!("r = {value:.3}"));
            if gated < total {
                format!("{gated} of {total} objects gated · {correlation}")
            } else {
                format!("{total} objects · {correlation}")
            }
        }
        Plot::Distribution(distribution) => {
            let groups = distribution.groups().len();
            if distribution.gated() < distribution.total() {
                format!(
                    "{groups} groups · {} of {} objects gated",
                    distribution.gated(),
                    distribution.total()
                )
            } else {
                format!("{groups} groups · {} objects", distribution.total())
            }
        }
    }
}
