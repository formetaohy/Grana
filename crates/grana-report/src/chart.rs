use grana_chart::{
    Axis as Scale, Category, Comparison as ComparisonChart, ComparisonNote, Curve, CurvePoint,
    DistributionStyle, Heading, Page, SeriesChart, SeriesPage, Sheet,
};
use grana_i18n::Language;

use crate::comparison::{Comparison, ComparisonReport};
use crate::quality::Metric;
use crate::significance::Significance;

const NOTE_LIMIT: usize = 3;

const MEMBER_LIMIT: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    Conditions,
    Frames,
}

impl Axis {
    pub const ALL: [Self; 2] = [Self::Conditions, Self::Frames];

    pub fn index(self) -> u8 {
        match self {
            Self::Conditions => 0,
            Self::Frames => 1,
        }
    }
}

pub struct Request<'a> {
    pub rows: &'a [Comparison],
    pub tests: &'a [Significance],
    pub metric: Metric,
    pub axis: Axis,
    pub group: Option<&'a str>,
    pub key: Option<&'a str>,
    pub style: DistributionStyle,
    pub language: Language,
}

pub struct Charted {
    pub page: Page,
    pub slots: Vec<(String, String)>,
}

pub fn chart(request: Request<'_>) -> Option<Charted> {
    let Request {
        rows,
        tests,
        metric,
        axis,
        group,
        key,
        style,
        language,
    } = request;
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
    let slots = visible
        .iter()
        .map(|row| (row.group.clone(), row.key.clone()))
        .collect::<Vec<_>>();
    let group_count = distinct(visible.iter().map(|row| row.group.as_str()));
    let set_count = distinct(visible.iter().map(|row| row.key.as_str()));
    let title = grana_i18n::measure::comparison_title(language, metric_name(language, metric));
    let y = format!("Y: {}", metric_axis(language, metric));
    let heading = |x: String, summary: String| {
        Heading::new(title.clone(), y.clone(), x)
            .with_summary(summary)
            .with_empty(grana_i18n::measure::no_groups_compare(language))
            .with_language(language)
    };
    match axis {
        Axis::Conditions => {
            let categories = visible
                .iter()
                .map(|row| Category {
                    label: category_label(row, group_count, set_count),
                    values: row
                        .samples
                        .iter()
                        .filter_map(|sample| metric.sample(sample))
                        .map(|value| value as f32)
                        .collect(),
                })
                .collect::<Vec<_>>();
            let notes = comparison_notes(&slots, tests, metric);
            Some(Charted {
                page: Page::Comparison(Box::new(ComparisonChart {
                    heading: heading(
                        grana_i18n::measure::x_conditions(language).to_string(),
                        grana_i18n::measure::comparison_summary(language, visible.len()),
                    ),
                    categories,
                    notes,
                    style,
                })),
                slots,
            })
        }
        Axis::Frames => {
            let curves = visible
                .iter()
                .map(|row| {
                    Curve::new(
                        category_label(row, group_count, set_count),
                        row.frames
                            .iter()
                            .enumerate()
                            .filter_map(|(time, frames)| {
                                let frame = frames[metric.index()]?;
                                Some(CurvePoint {
                                    x: time as f32 + 1.0,
                                    value: frame.mean as f32,
                                    lower: frame.lower as f32,
                                    upper: frame.upper as f32,
                                })
                            })
                            .collect(),
                    )
                    .with_members(members(row, metric))
                })
                .collect::<Vec<_>>();
            let mut low = f32::INFINITY;
            let mut high = f32::NEG_INFINITY;
            let mut frames = 2u32;
            for curve in &curves {
                frames = frames.max(
                    curve
                        .points
                        .iter()
                        .map(|point| point.x as u32)
                        .max()
                        .unwrap_or(0),
                );
                for point in &curve.points {
                    low = low.min(point.lower);
                    high = high.max(point.upper);
                }
            }
            if !(low.is_finite() && high.is_finite()) {
                low = 0.0;
                high = 1.0;
            } else if high <= low {
                let padding = (0.5 * low.abs()).max(0.5);
                low -= padding;
                high += padding;
            }
            let notes = frame_notes(&visible, tests, metric);
            Some(Charted {
                page: Page::Series(Box::new(SeriesPage {
                    heading: heading(
                        grana_i18n::measure::x_frames(language).to_string(),
                        grana_i18n::measure::comparison_frames_summary(
                            language,
                            curves.len(),
                            frames,
                        ),
                    ),
                    chart: SeriesChart {
                        x: Scale::frames(1.0, frames as f32, 5),
                        y: Scale::new(low, high, 4),
                        curves,
                        bands: Vec::new(),
                        notes,
                    },
                    style,
                })),
                slots,
            })
        }
    }
}

pub fn sheet(
    report: &ComparisonReport,
    style: DistributionStyle,
    language: Language,
) -> Option<Page> {
    let mut pages = Vec::new();
    for (key, kind) in sets_of(&report.groups) {
        let rows = report
            .groups
            .iter()
            .filter(|row| row.key == key && row.kind == kind)
            .cloned()
            .collect::<Vec<_>>();
        for metric in Metric::ALL {
            if !rows.iter().any(|row| {
                row.samples
                    .iter()
                    .any(|sample| metric.summary_of(sample).is_some())
            }) {
                continue;
            }
            if let Some(charted) = chart(Request {
                rows: &rows,
                tests: &report.tests,
                metric,
                axis: Axis::Conditions,
                group: None,
                key: Some(&key),
                style,
                language,
            }) {
                pages.push(charted.page);
            }
            let frames = rows.iter().any(|row| {
                row.frames
                    .iter()
                    .any(|frames| frames[metric.index()].is_some())
            });
            if frames
                && let Some(charted) = chart(Request {
                    rows: &rows,
                    tests: &report.tests,
                    metric,
                    axis: Axis::Frames,
                    group: None,
                    key: Some(&key),
                    style,
                    language,
                })
            {
                pages.push(charted.page);
            }
        }
    }
    (!pages.is_empty()).then(|| Page::Sheet(Box::new(Sheet::new(pages, 2))))
}

pub fn group_names(rows: &[Comparison]) -> Vec<String> {
    let mut names = Vec::new();
    for row in rows {
        if !names.contains(&row.group) {
            names.push(row.group.clone());
        }
    }
    names
}

pub fn set_names(rows: &[Comparison], group: Option<&str>) -> Vec<(String, String)> {
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

fn sets_of(rows: &[Comparison]) -> Vec<(String, String)> {
    let mut sets = Vec::new();
    for row in rows {
        if !sets
            .iter()
            .any(|(key, kind)| key == &row.key && kind == &row.kind)
        {
            sets.push((row.key.clone(), row.kind.clone()));
        }
    }
    sets
}

fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    let mut seen = Vec::new();
    for value in values {
        if !seen.contains(&value) {
            seen.push(value);
        }
    }
    seen.len()
}

fn category_label(row: &Comparison, groups: usize, sets: usize) -> String {
    match (groups, sets) {
        (1, _) => row.name.clone(),
        (_, 1) => row.group.clone(),
        _ => format!("{} · {}", row.group, row.name),
    }
}

fn members(row: &Comparison, metric: Metric) -> Vec<Vec<CurvePoint>> {
    let stride = row.samples.len().div_ceil(MEMBER_LIMIT).max(1);
    row.samples
        .iter()
        .step_by(stride)
        .map(|sample| {
            sample
                .frames
                .iter()
                .filter_map(|(time, values)| {
                    let value = values[metric.index()]?;
                    Some(CurvePoint {
                        x: *time as f32 + 1.0,
                        value: value.mean as f32,
                        lower: value.mean as f32,
                        upper: value.mean as f32,
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|points| points.len() > 1)
        .collect()
}

fn comparison_notes(
    slots: &[(String, String)],
    tests: &[Significance],
    metric: Metric,
) -> Vec<ComparisonNote> {
    let mut notes = tests
        .iter()
        .filter(|test| test.frame.is_none() && test.metric == metric && test.significant())
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
                ComparisonNote {
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

fn frame_notes(
    rows: &[&Comparison],
    tests: &[Significance],
    metric: Metric,
) -> Vec<grana_chart::Note> {
    let mut pairs = Vec::<(usize, usize, Vec<(u32, f64, &'static str)>)>::new();
    for test in tests.iter().filter(|test| test.metric == metric) {
        let Some(frame) = test.frame else {
            continue;
        };
        if !test.significant() {
            continue;
        }
        if !rows
            .iter()
            .any(|row| row.key == test.key && row.kind == test.kind)
        {
            continue;
        }
        let Some(from) = rows.iter().position(|row| row.group == test.group_a) else {
            continue;
        };
        let Some(to) = rows.iter().position(|row| row.group == test.group_b) else {
            continue;
        };
        let point = (frame, test.q(), test.mark());
        match pairs
            .iter_mut()
            .find(|(left, right, _)| *left == from && *right == to)
        {
            Some(entry) => entry.2.push(point),
            None => pairs.push((from, to, vec![point])),
        }
    }
    pairs.sort_by(|left, right| {
        let best = |pairs: &Vec<(u32, f64, &'static str)>| {
            pairs
                .iter()
                .map(|point| point.1)
                .fold(f64::INFINITY, f64::min)
        };
        best(&left.2).total_cmp(&best(&right.2))
    });
    let mut notes = Vec::new();
    for (from, to, mut points) in pairs.into_iter().take(NOTE_LIMIT) {
        points.sort_by_key(|point| point.0);
        let mut index = 0;
        while index < points.len() {
            let start = index;
            while index + 1 < points.len() && points[index + 1].0 == points[index].0 + 1 {
                index += 1;
            }
            let run = &points[start..=index];
            let frame = run[run.len() / 2].0;
            let mark = run
                .iter()
                .min_by(|left, right| left.1.total_cmp(&right.1))
                .map(|point| point.2)
                .expect("a significant stretch holds a test");
            if let Some(y) = frame_peak(rows, from, to, metric, frame) {
                notes.push(grana_chart::Note {
                    x: frame as f32 + 1.0,
                    y,
                    mark: mark.to_string(),
                });
            }
            index += 1;
        }
    }
    notes
}

fn frame_peak(
    rows: &[&Comparison],
    from: usize,
    to: usize,
    metric: Metric,
    frame: u32,
) -> Option<f32> {
    let mut peak = f32::NEG_INFINITY;
    for index in [from, to] {
        if let Some(value) = rows[index]
            .frames
            .get(frame as usize)
            .and_then(|frames| frames[metric.index()])
            .map(|frame| frame.mean as f32)
        {
            peak = peak.max(value);
        }
    }
    peak.is_finite().then_some(peak)
}

fn metric_name(language: Language, metric: Metric) -> &'static str {
    match metric {
        Metric::Objects => grana_i18n::batch::metric_objects(language),
        Metric::Volume => grana_i18n::batch::metric_volume(language),
        Metric::Area => grana_i18n::batch::metric_area(language),
        Metric::Size => grana_i18n::batch::metric_size(language),
        Metric::Intensity => grana_i18n::batch::metric_intensity(language),
    }
}

fn metric_axis(language: Language, metric: Metric) -> String {
    let name = metric_name(language, metric);
    match metric.unit() {
        "" => name.to_string(),
        unit => format!("{name} ({unit})"),
    }
}
