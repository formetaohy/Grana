use grana_raster::{Cell, Group, Raster, Sheet};
use grana_text::Fonts;

use super::{Plan, Row, SNAPSHOT};

pub(super) const SHEET: &str = "sheet.png";

pub(super) const MANIFEST: &str = "sheet.csv";

const TILE: [u32; 2] = [720, 450];

const COLUMNS: u32 = 3;

struct Entry {
    dataset: usize,
    group: String,
    title: String,
    detail: String,
    image: Raster,
}

pub(super) fn write(plan: &Plan, rows: &[Option<Row>]) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut cells = vec![None; plan.datasets.len()];
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            continue;
        };
        let dataset = &plan.datasets[index];
        let path = plan.output.join(dataset.folder()).join(SNAPSHOT);
        let Ok(image) = grana_raster::read(&path) else {
            continue;
        };
        entries.push(Entry {
            dataset: index,
            group: dataset.group().to_string(),
            title: dataset.name(),
            detail: format!("{} · {}", dataset.scene_label(), row.status),
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
    for (ordinal, entry) in entries.iter().enumerate() {
        cells[entry.dataset] = Some(ordinal + 1);
    }
    let groups = groups(&entries);
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

fn groups<'a>(entries: &'a [Entry]) -> Vec<Group<'a>> {
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
            })
            .collect();
        groups.push(Group { name, cells });
        start = end;
    }
    groups
}
