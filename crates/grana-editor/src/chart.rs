use grana_chart::{Category, Chart, Comparison, DistributionStyle, Gates, Heading, Note, Page};
use grana_i18n::Language;
use grana_measure::{Plot, Table};
use grana_session::{BatchComparison, BatchSignificance, SessionView};

use crate::state::{ComparisonMetric, Workspace};
use crate::view::column_label;

pub fn page(
    view: &SessionView<'_>,
    distribution: DistributionStyle,
    language: Language,
) -> Option<Page> {
    plot_page(view, distribution, language).map(|chart| Page::Plot(Box::new(chart)))
}

pub fn comparison_page(view: &SessionView<'_>, workspace: &Workspace) -> Option<Page> {
    let language = workspace.language;
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
        language,
    )?;
    built.chart.heading.title =
        grana_i18n::measure::comparison_title(language, metric_name(language, metric));
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
    language: Language,
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
    let heading = Heading::new(
        "",
        format!("Y: {}", metric_axis(language, metric)),
        grana_i18n::measure::x_conditions(language),
    )
    .with_summary(grana_i18n::measure::comparison_summary(
        language,
        visible.len(),
    ))
    .with_empty(grana_i18n::measure::no_groups_compare(language))
    .with_language(language);
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

fn plot_page(
    view: &SessionView<'_>,
    distribution: DistributionStyle,
    language: Language,
) -> Option<Chart> {
    let measure = view.measure;
    let table = measure.table()?;
    let plot = measure.plot()?.clone();
    let columns = table.columns();
    let (title, y, x) = match &plot {
        Plot::Histogram(histogram) => (
            grana_i18n::measure::histogram_title(
                language,
                &column_label(language, &columns[histogram.column()]),
            ),
            grana_i18n::measure::y_objects(language).to_string(),
            format!(
                "X: {}",
                column_label(language, &columns[histogram.column()])
            ),
        ),
        Plot::Scatter(scatter) => (
            grana_i18n::measure::scatter_title(
                language,
                &column_label(language, &columns[scatter.y()]),
                &column_label(language, &columns[scatter.x()]),
            ),
            format!("Y: {}", column_label(language, &columns[scatter.y()])),
            format!("X: {}", column_label(language, &columns[scatter.x()])),
        ),
        Plot::Distribution(distribution) => (
            grana_i18n::measure::distribution_title(
                language,
                &column_label(language, &columns[distribution.column()]),
            ),
            format!(
                "Y: {}",
                column_label(language, &columns[distribution.column()])
            ),
            grana_i18n::measure::x_groups(language).to_string(),
        ),
        Plot::Trend(trend) => (
            grana_i18n::measure::trend_title(
                language,
                &column_label(language, &columns[trend.column()]),
            ),
            format!("Y: {}", column_label(language, &columns[trend.column()])),
            grana_i18n::measure::x_frame(language).to_string(),
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
        heading: Heading::new(title, y, x)
            .with_summary(caption(&plot, table, language))
            .with_empty(empty_text(language, &plot))
            .with_language(language),
        plot,
        gate,
        highlight: None,
        distribution,
    })
}

fn empty_text(language: Language, plot: &Plot) -> &'static str {
    match plot {
        Plot::Histogram(_) | Plot::Scatter(_) | Plot::Distribution(_) => {
            grana_i18n::measure::no_objects(language)
        }
        Plot::Trend(_) => grana_i18n::measure::no_frames_trend(language),
    }
}

fn range(filters: &[grana_measure::Filter], column: usize) -> Option<(f32, f32)> {
    filters
        .iter()
        .find(|filter| filter.column() == column)
        .map(|filter| (filter.minimum(), filter.maximum()))
}

fn caption(plot: &Plot, table: &Table, language: Language) -> String {
    let total = table.rows() as u64;
    match plot {
        Plot::Histogram(histogram) => {
            let gated = histogram.summary().count();
            if gated < total {
                grana_i18n::measure::gated_objects(language, gated, total)
            } else {
                grana_i18n::measure::objects(language, total)
            }
        }
        Plot::Scatter(scatter) => {
            let gated = scatter.x_summary().count();
            let correlation = scatter
                .correlation()
                .map_or_else(|| "r —".to_string(), |value| format!("r = {value:.3}"));
            if gated < total {
                format!(
                    "{} · {correlation}",
                    grana_i18n::measure::gated_objects(language, gated, total)
                )
            } else {
                format!(
                    "{} · {correlation}",
                    grana_i18n::measure::objects(language, total)
                )
            }
        }
        Plot::Distribution(distribution) => {
            let groups = grana_i18n::measure::groups(language, distribution.groups().len());
            if distribution.gated() < distribution.total() {
                format!(
                    "{groups} · {}",
                    grana_i18n::measure::gated_objects(
                        language,
                        distribution.gated(),
                        distribution.total()
                    )
                )
            } else {
                format!(
                    "{groups} · {}",
                    grana_i18n::measure::objects(language, distribution.total())
                )
            }
        }
        Plot::Trend(trend) => {
            if trend.gated() < trend.total() {
                grana_i18n::measure::gated_values_over_frames(
                    language,
                    trend.gated(),
                    trend.total(),
                    trend.frames(),
                )
            } else {
                grana_i18n::measure::values_over_frames(language, trend.total(), trend.frames())
            }
        }
    }
}

fn metric_name(language: Language, metric: ComparisonMetric) -> &'static str {
    match metric {
        ComparisonMetric::Objects => grana_i18n::batch::metric_objects(language),
        ComparisonMetric::Volume => grana_i18n::batch::metric_volume(language),
        ComparisonMetric::Area => grana_i18n::batch::metric_area(language),
        ComparisonMetric::Size => grana_i18n::batch::metric_size(language),
        ComparisonMetric::Intensity => grana_i18n::batch::metric_intensity(language),
    }
}

fn metric_axis(language: Language, metric: ComparisonMetric) -> String {
    let name = metric_name(language, metric);
    match metric.unit() {
        "" => name.to_string(),
        unit => format!("{name} ({unit})"),
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
