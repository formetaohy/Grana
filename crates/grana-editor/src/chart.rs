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
    let metrics = view.batch.as_ref().map_or(&[][..], |batch| batch.metrics);
    let rows = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.comparison);
    let tests = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.significance);
    let interactions = view
        .batch
        .as_ref()
        .map_or(&[][..], |batch| batch.interactions);
    let index = workspace
        .comparison_metric
        .as_ref()
        .and_then(|key| metrics.iter().position(|metric| metric.key() == key))
        .unwrap_or(0);
    let metric = metrics.get(index)?;
    comparison_chart(ComparisonRequest {
        rows,
        tests,
        interactions,
        metric,
        index,
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
        Plot::NearestNeighbour(_) => (
            grana_i18n::measure::nearest_title(language).to_string(),
            grana_i18n::measure::y_nearest(language).to_string(),
            grana_i18n::measure::x_radius(language).to_string(),
        ),
        Plot::Enrichment(_) => (
            grana_i18n::measure::enrichment_title(language).to_string(),
            grana_i18n::measure::y_index_class(language).to_string(),
            grana_i18n::measure::x_neighbour_class(language).to_string(),
        ),
    };
    let target = measure
        .target()
        .and_then(|id| view.scene.and_then(|scene| scene.objects.get(id)))
        .map(|set| set.name().to_string());
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
        Plot::Ripley(_) | Plot::NearestNeighbour(_) => Gates::default(),
        Plot::Enrichment(_) => Gates::default(),
    };
    Some(Chart {
        heading: Heading::new(title, y, x)
            .with_summary(caption(&plot, table, language, target.as_deref()))
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
        Plot::Ripley(_) | Plot::NearestNeighbour(_) => grana_i18n::measure::no_spatial(language),
        Plot::Enrichment(_) => grana_i18n::measure::no_classes(language),
    }
}

fn trend_unit(language: Language, population: grana_measure::Population) -> Option<&'static str> {
    match population {
        grana_measure::Population::Object => Some(grana_i18n::measure::unit_objects(language)),
        grana_measure::Population::Track => Some(grana_i18n::measure::unit_tracks(language)),
        grana_measure::Population::Molecule => Some(grana_i18n::measure::unit_molecules(language)),
        grana_measure::Population::Frame => None,
    }
}

fn range(filters: &[grana_measure::Filter], column: usize) -> Option<(f32, f32)> {
    filters
        .iter()
        .find(|filter| filter.column() == column)
        .map(|filter| (filter.minimum(), filter.maximum()))
}

fn caption(plot: &Plot, table: &Table, language: Language, target: Option<&str>) -> String {
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
            let (gated, total, frames) = (trend.gated(), trend.total(), trend.frames());
            match (trend_unit(language, trend.population()), gated < total) {
                (Some(unit), true) => {
                    grana_i18n::measure::gated_trend_caption(language, unit, gated, total, frames)
                }
                (Some(unit), false) => {
                    grana_i18n::measure::trend_caption(language, unit, total, frames)
                }
                (None, true) => {
                    grana_i18n::measure::gated_values_over_frames(language, gated, total, frames)
                }
                (None, false) => grana_i18n::measure::values_over_frames(language, total, frames),
            }
        }
        Plot::Ripley(ripley) => crate::plot::ripley_caption(language, ripley, target),
        Plot::NearestNeighbour(nearest) => crate::plot::nearest_caption(language, nearest, target),
        Plot::Enrichment(enrichment) => crate::plot::enrichment_caption(language, enrichment),
    }
}
