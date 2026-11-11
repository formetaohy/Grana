use grana_chart::{Category, Chart, Comparison, DistributionStyle, Gates, Heading, Note, Page};
use grana_measure::{Plot, Table};
use grana_session::{BatchComparison, BatchSignificance, SessionView};

use crate::state::{ComparisonMetric, Workspace};
use crate::view::column_label;

pub fn page(view: &SessionView<'_>, distribution: DistributionStyle) -> Option<Page> {
    plot_page(view, distribution).map(|chart| Page::Plot(Box::new(chart)))
}

pub fn comparison_page(view: &SessionView<'_>, workspace: &Workspace) -> Option<Page> {
    let metric = workspace.comparison_metric;
    let rows = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.comparison);
    let tests = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.significance);
    let mut built = comparison_chart(
        rows,
        tests,
        metric,
        workspace.comparison_group.as_deref(),
        workspace.comparison_key.as_deref(),
        workspace.distribution_style,
    )?;
    built.chart.heading.title = format!("Comparison of {}", metric.name());
    Some(Page::Comparison(Box::new(built.chart)))
}

pub struct ComparisonChart {
    pub chart: Comparison,
    pub slots: Vec<(String, String)>,
}

pub(crate) fn comparison_chart(
    rows: &[BatchComparison],
    tests: &[BatchSignificance],
    metric: ComparisonMetric,
    group: Option<&str>,
    key: Option<&str>,
    style: DistributionStyle,
) -> Option<ComparisonChart> {
    let groups = group_names(rows);
    let group = group.filter(|name| groups.iter().any(|candidate| candidate == name));
    let sets = set_names(rows, group);
    let key = key.filter(|name| sets.iter().any(|(candidate, _)| candidate == name));
    let visible = rows
        .iter()
        .filter(|row| group.is_none_or(|group| group == row.group))
        .filter(|row| key.is_none_or(|key| key == row.key))
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return None;
    }
    let categories = visible
        .iter()
        .map(|row| Category {
            label: category_label(row, groups.len(), sets.len()),
            values: row
                .samples
                .iter()
                .filter_map(|sample| metric.sample(sample))
                .map(|value| value as f32)
                .collect(),
        })
        .collect::<Vec<_>>();
    let slots = visible
        .iter()
        .map(|row| (row.group.clone(), row.key.clone()))
        .collect::<Vec<_>>();
    let notes = comparison_notes(&slots, tests, metric);
    let heading =
        Heading::new("", format!("Y: {}", metric_axis(metric)), "X: Conditions").with_summary(
            format!("{} categories · images are the samples", visible.len()),
        );
    Some(ComparisonChart {
        chart: Comparison {
            heading,
            categories,
            notes,
            style,
        },
        slots,
    })
}

fn plot_page(view: &SessionView<'_>, distribution: DistributionStyle) -> Option<Chart> {
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
        Plot::Trend(trend) => (
            format!("Trend of {}", column_label(&columns[trend.column()])),
            format!("Y: {}", column_label(&columns[trend.column()])),
            "X: Frame".to_string(),
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
        Plot::Trend(trend) => Gates {
            x: None,
            y: range(measure.filters(), trend.column()),
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
        Plot::Trend(trend) => {
            let frames = trend.frames();
            if trend.gated() < trend.total() {
                format!(
                    "{} of {} values over {frames} frames",
                    trend.gated(),
                    trend.total()
                )
            } else {
                format!("{} values over {frames} frames", trend.total())
            }
        }
    }
}

fn metric_axis(metric: ComparisonMetric) -> String {
    match metric.unit() {
        "" => metric.name().to_string(),
        unit => format!("{} ({unit})", metric.name()),
    }
}

fn category_label(row: &BatchComparison, groups: usize, sets: usize) -> String {
    match (groups, sets) {
        (1, _) => row.name.clone(),
        (_, 1) => row.group.clone(),
        _ => format!("{} · {}", row.group, row.name),
    }
}

fn comparison_notes(
    slots: &[(String, String)],
    tests: &[BatchSignificance],
    metric: ComparisonMetric,
) -> Vec<Note> {
    let mut notes = tests
        .iter()
        .filter(|test| test.metric == metric && test.significant())
        .filter_map(|test| {
            let from = slots
                .iter()
                .position(|(group, key)| group == &test.group_a && key == &test.key)?;
            let to = slots
                .iter()
                .position(|(group, key)| group == &test.group_b && key == &test.key)?;
            if from == to {
                return None;
            }
            Some((
                (from.abs_diff(to), from.min(to), from.max(to)),
                Note {
                    from,
                    to,
                    mark: test.mark().to_string(),
                },
            ))
        })
        .collect::<Vec<_>>();
    notes.sort_by_key(|(order, _)| *order);
    notes
        .into_iter()
        .take(NOTE_LIMIT)
        .map(|(_, note)| note)
        .collect()
}

pub(crate) fn group_names(rows: &[BatchComparison]) -> Vec<String> {
    let mut names = Vec::new();
    for row in rows {
        if !names.contains(&row.group) {
            names.push(row.group.clone());
        }
    }
    names
}

pub(crate) fn set_names(rows: &[BatchComparison], group: Option<&str>) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for row in rows {
        if group.is_some_and(|group| group != row.group) {
            continue;
        }
        if !names.iter().any(|(key, _)| key == &row.key) {
            names.push((row.key.clone(), row.name.clone()));
        }
    }
    names
}

const NOTE_LIMIT: usize = 3;
