use std::path::Path;

use grana_raster::{Cell, Group, Raster, Row, Sheet, Table};
use grana_text::Fonts;

use super::quality::Comparison;
use super::{Plan, Row as ResultRow, SNAPSHOT};

pub(super) const SHEET: &str = "sheet.png";

pub(super) const MANIFEST: &str = "sheet.csv";

const TILE: [u32; 2] = [720, 450];

const COLUMNS: u32 = 3;

const METRICS: [&str; 6] = [
    "datasets",
    "objects",
    "volume µm³",
    "area µm²",
    "size µm",
    "intensity",
];

struct Entry {
    dataset: usize,
    group: String,
    title: String,
    detail: String,
    accent: [u8; 4],
    image: Raster,
    plots: Vec<Raster>,
}

pub(super) fn write(
    plan: &Plan,
    rows: &[Option<ResultRow>],
    comparison: &[Comparison],
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut cells = vec![None; plan.datasets.len()];
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            continue;
        };
        let dataset = &plan.datasets[index];
        let folder = plan.output.join(dataset.folder());
        let path = folder.join(SNAPSHOT);
        let Ok(image) = grana_raster::read(&path) else {
            continue;
        };
        entries.push(Entry {
            dataset: index,
            group: dataset.group().to_string(),
            title: dataset.name(),
            detail: format!("{} · {}", dataset.scene_label(), row.status),
            accent: status_color(row.status),
            image,
            plots: colocalizations(&folder),
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
        ordinal += entries[start..end]
            .iter()
            .map(|entry| entry.plots.len())
            .sum::<usize>();
        start = end;
    }
    let tables = tables(comparison);
    let groups = groups(&entries, &tables);
    let mut fonts = Fonts::system();
    let sheet = Sheet::new(TILE, COLUMNS).compose(&mut fonts, &groups);
    grana_raster::write(&plan.output.join(SHEET), &sheet).map_err(String::from)?;
    let mut lines = vec![grana_measure::csv::line(&[
        "cell", "group", "dataset", "scene", "status",
    ])];
    for (index, dataset) in plan.datasets.iter().enumerate() {
        let status = rows[index].as_ref().map_or("skipped", |row| row.status);
        let cell = cells[index].map_or_else(String::new, |cell| cell.to_string());
        lines.push(grana_measure::csv::line(&[
            &cell,
            dataset.group(),
            &dataset.name(),
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
                tables.push((
                    entry.group.clone(),
                    Table {
                        columns: Vec::new(),
                        rows: METRICS
                            .iter()
                            .map(|label| Row {
                                label: (*label).to_string(),
                                values: Vec::new(),
                            })
                            .collect(),
                    },
                ));
                tables.len() - 1
            }
        };
        let table = &mut tables[position].1;
        table.columns.push(entry.name.clone());
        let values = [
            entry.datasets.to_string(),
            entry
                .objects
                .map_or_else(|| "-".to_string(), |value| format!("{value:.1}")),
            deviated(entry.volume, entry.volume_deviation, 3),
            deviated(entry.area, entry.area_deviation, 3),
            deviated(entry.size, entry.size_deviation, 3),
            deviated(entry.intensity, entry.intensity_deviation, 2),
        ];
        for (row, value) in table.rows.iter_mut().zip(values) {
            row.values.push(value);
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
        if entries[start..end]
            .iter()
            .any(|entry| !entry.plots.is_empty())
        {
            let cells = entries[start..end]
                .iter()
                .flat_map(|entry| {
                    entry.plots.iter().map(|plot| Cell {
                        image: plot,
                        title: &entry.title,
                        detail: "colocalization",
                        accent: Some(entry.accent),
                    })
                })
                .collect();
            groups.push(Group {
                name: format!("{name} · colocalization"),
                table: None,
                cells,
            });
        }
        start = end;
    }
    groups
}

fn colocalizations(folder: &Path) -> Vec<Raster> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("coloc-") && name.ends_with(".png")
            })
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| grana_raster::read(path).ok())
        .collect()
}

fn status_color(status: &str) -> [u8; 4] {
    match status {
        "ok" => [86, 190, 126, 255],
        "resumed" => [96, 156, 240, 255],
        "failed" => [232, 96, 96, 255],
        _ => [120, 126, 138, 255],
    }
}
