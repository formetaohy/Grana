use grana_raster::{Cell, Group, Raster, Row, Sheet, Table};
use grana_report::{Comparison, ComparisonReport, Metric, Significance};
use grana_text::Fonts;

use super::{Plan, Row as ResultRow};
pub(super) const SHEET: &str = "sheet.png";

pub(super) const SIGNIFICANCE: &str = "significance.png";

pub(super) const MANIFEST: &str = "sheet.csv";

const TILE: [u32; 2] = [720, 450];

const COLUMNS: u32 = 3;

const SIGNIFICANCE_TILE: [u32; 2] = [960, 450];

struct Entry {
    dataset: usize,
    group: String,
    title: String,
    detail: String,
    accent: [u8; 4],
    image: Raster,
}

pub(super) fn write(
    plan: &Plan,
    rows: &[Option<ResultRow>],
    comparison: &ComparisonReport,
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut cells = vec![None; plan.datasets.len()];
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            continue;
        };
        let dataset = &plan.datasets[index];
        let folder = plan.output.join(dataset.folder());
        let Some(file) = super::pipeline::capture_files(&plan.protocol, "snapshot", Some("png"))
            .into_iter()
            .next()
        else {
            continue;
        };
        let Ok(image) = grana_raster::read(&folder.join(file)) else {
            continue;
        };
        entries.push(Entry {
            dataset: index,
            group: dataset.group().to_string(),
            title: dataset.title(),
            detail: format!("{} · {}", dataset.scene_label(), row.status),
            accent: status_color(row.status),
            image,
        });
    }
    if entries.is_empty() {
        return Ok(());
    }
    entries.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then(left.dataset.cmp(&right.dataset))
    });
    let mut ordinal = 1;
    let mut start = 0usize;
    while start < entries.len() {
        let end = entries[start..]
            .iter()
            .position(|entry| entry.group != entries[start].group)
            .map_or(entries.len(), |offset| start + offset);
        for entry in &entries[start..end] {
            cells[entry.dataset] = Some(ordinal);
            ordinal += 1;
        }
        start = end;
    }
    let tables = tables(&comparison.groups);
    let groups = groups(&entries, &tables);
    let mut fonts = Fonts::system();
    let sheet = Sheet::new(TILE, COLUMNS).compose(&mut fonts, &groups);
    grana_raster::write(&plan.output.join(SHEET), &sheet).map_err(String::from)?;
    let mut lines = vec![grana_measure::csv::line(&[
        "cell", "group", "dataset", "well", "row", "column", "field", "scene", "status",
    ])];
    let index = |value: Option<usize>| value.map_or_else(String::new, |value| value.to_string());
    for (index_of, dataset) in plan.datasets.iter().enumerate() {
        let status = rows[index_of].as_ref().map_or("skipped", |row| row.status);
        let cell = cells[index_of].map_or_else(String::new, |cell| cell.to_string());
        lines.push(grana_measure::csv::line(&[
            &cell,
            dataset.group(),
            &dataset.title(),
            dataset.plate_label().unwrap_or_default(),
            &index(dataset.well_row()),
            &index(dataset.well_column()),
            &index(dataset.well_field()),
            &dataset.scene_label(),
            status,
        ]));
    }
    std::fs::write(plan.output.join(MANIFEST), lines.concat())
        .map_err(|error| format!("failed to write the quality sheet manifest: {error}"))
}

fn tables(comparison: &[Comparison]) -> Vec<(String, Table)> {
    let mut tables = Vec::<(String, Table)>::new();
    for entry in comparison {
        let position = match tables.iter().position(|(group, _)| *group == entry.group) {
            Some(position) => position,
            None => {
                let rows = std::iter::once("images".to_string())
                    .chain(std::iter::once("objects".to_string()))
                    .chain(Metric::ALL.map(metric_label))
                    .map(|label| Row {
                        label,
                        values: Vec::new(),
                    })
                    .collect();
                tables.push((
                    entry.group.clone(),
                    Table {
                        columns: Vec::new(),
                        rows,
                    },
                ));
                tables.len() - 1
            }
        };
        let table = &mut tables[position].1;
        table.columns.push(entry.name.clone());
        let mut values = vec![entry.images.to_string(), entry.objects.to_string()];
        values.extend(Metric::ALL.map(|metric| {
            deviated(
                metric.value(entry),
                metric.deviation(entry),
                metric.decimals(),
            )
        }));
        for (row, value) in table.rows.iter_mut().zip(values) {
            row.values.push(value);
        }
    }
    tables
}

fn metric_label(metric: Metric) -> String {
    match metric.unit() {
        "" => metric.name().to_string(),
        unit => format!("{} {unit}", metric.name()),
    }
}

pub(super) fn significance(plan: &Plan, tests: &[Significance]) -> Result<(), String> {
    let tests = tests
        .iter()
        .filter(|test| test.frame.is_none())
        .collect::<Vec<_>>();
    let mut tables = significance_tables(&tests);
    if tables.is_empty() {
        return Ok(());
    }
    for (_, table) in &mut tables {
        for row in &mut table.rows {
            while row.values.len() < table.columns.len() {
                row.values.push("-".to_string());
            }
        }
    }
    let groups = tables
        .iter()
        .map(|(name, table)| Group {
            name: name.clone(),
            table: Some(table),
            cells: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut fonts = Fonts::system();
    let sheet = Sheet::grid(SIGNIFICANCE_TILE, 1).compose(&mut fonts, &groups);
    grana_raster::write(&plan.output.join(SIGNIFICANCE), &sheet).map_err(String::from)
}

fn significance_tables(tests: &[&Significance]) -> Vec<(String, Table)> {
    let mut tables = Vec::<(String, Table)>::new();
    for test in tests {
        let name = if test.name == test.kind {
            test.name.clone()
        } else {
            format!("{} · {}", test.name, test.kind)
        };
        let index = tables
            .iter()
            .position(|(other, _)| *other == name)
            .unwrap_or_else(|| {
                let rows = Metric::ALL
                    .map(|metric| Row {
                        label: metric_label(metric),
                        values: Vec::new(),
                    })
                    .into_iter()
                    .collect();
                tables.push((
                    name.clone(),
                    Table {
                        columns: Vec::new(),
                        rows,
                    },
                ));
                tables.len() - 1
            });
        let table = &mut tables[index].1;
        let column = format!("{} vs {}", test.group_a, test.group_b);
        let slot = match table.columns.iter().position(|other| *other == column) {
            Some(slot) => slot,
            None => {
                table.columns.push(column);
                table.columns.len() - 1
            }
        };
        let row = test.metric.index();
        let cell = format!(
            "{} q={} r={} g={}",
            test.mark(),
            test.q_text(),
            test.rank_q_text(),
            test.effect
                .map_or_else(|| "-".to_string(), |effect| format!("{effect:.2}"))
        );
        let values = &mut table.rows[row].values;
        while values.len() < slot {
            values.push(String::new());
        }
        if values.len() == slot {
            values.push(cell);
        } else {
            values[slot] = cell;
        }
    }
    tables
}

fn deviated(mean: Option<f64>, deviation: Option<f64>, decimals: usize) -> String {
    match (mean, deviation) {
        (Some(mean), Some(deviation)) => {
            format!("{mean:.decimals$} ± {deviation:.decimals$}")
        }
        (Some(mean), None) => format!("{mean:.decimals$}"),
        _ => "-".to_string(),
    }
}

fn groups<'a>(entries: &'a [Entry], tables: &'a [(String, Table)]) -> Vec<Group<'a>> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    while start < entries.len() {
        let name = entries[start].group.as_str();
        let end = entries[start..]
            .iter()
            .position(|entry| entry.group != entries[start].group)
            .map_or(entries.len(), |offset| start + offset);
        let cells = entries[start..end]
            .iter()
            .map(|entry| Cell {
                image: &entry.image,
                title: &entry.title,
                detail: &entry.detail,
                accent: Some(entry.accent),
            })
            .collect::<Vec<_>>();
        let table = tables
            .iter()
            .find(|(group, _)| group == name)
            .map(|(_, table)| table);
        groups.push(Group {
            name: name.to_string(),
            table,
            cells,
        });
        start = end;
    }
    groups
}

fn status_color(status: &str) -> [u8; 4] {
    match status {
        "ok" => [86, 190, 126, 255],
        "resumed" => [96, 156, 240, 255],
        "failed" => [232, 96, 96, 255],
        _ => [120, 126, 138, 255],
    }
}
