use grana_chart::{Chart, DistributionStyle, Gates, Heading, Page};
use grana_i18n::Language;
use grana_measure::{Plot, Table};
use grana_session::SessionView;

use crate::state::Workspace;
use crate::view::column_label;

pub(crate) use grana_report::{
    Request as ComparisonRequest, chart as comparison_chart, group_names, set_names,
};

pub fn page(
    view: &SessionView<'_>,
    distribution: DistributionStyle,
    language: Language,
) -> Option<Page> {
    plot_page(view, distribution, language).map(|chart| Page::Plot(Box::new(chart)))
}

pub fn comparison_page(view: &SessionView<'_>, workspace: &Workspace) -> Option<Page> {
    let rows = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.comparison);
    let tests = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.significance);
    comparison_chart(ComparisonRequest {
        rows,
        tests,
        metric: workspace.comparison_metric,
        axis: workspace.comparison_axis,
        group: workspace.comparison_group.as_deref(),
        key: workspace.comparison_key.as_deref(),
        style: workspace.distribution_style,
        language: workspace.language,
    })
    .map(|built| built.page)
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
        Plot::Ripley(_) => (
            grana_i18n::measure::ripley_title(language).to_string(),
            grana_i18n::measure::y_ripley(language).to_string(),
            grana_i18n::measure::x_radius(language).to_string(),
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
        Plot::Ripley(_) => Gates::default(),
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
        Plot::Ripley(_) => grana_i18n::measure::no_spatial(language),
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
        Plot::Ripley(ripley) => {
            grana_i18n::measure::ripley_summary(language, ripley.frames(), ripley.objects())
        }
    }
}
