use grana_raster::Grid;

use super::{Plan, Row, SNAPSHOT};

pub(super) const SHEET: &str = "sheet.png";

pub(super) const MANIFEST: &str = "sheet.csv";

const TILE: [u32; 2] = [800, 500];

pub(super) fn write(plan: &Plan, rows: &[Option<Row>]) -> Result<(), String> {
    let mut tiles = Vec::new();
    let mut cells = vec![None; plan.datasets.len()];
    for (index, row) in rows.iter().enumerate() {
        if row.is_none() {
            continue;
        }
        let path = plan
            .output
            .join(plan.datasets[index].folder())
            .join(SNAPSHOT);
        let Ok(tile) = grana_raster::read(&path) else {
            continue;
        };
        cells[index] = Some(tiles.len() + 1);
        tiles.push(tile);
    }
    if tiles.is_empty() {
        return Ok(());
    }
    let sheet = Grid::new(TILE, tiles.len()).compose(&tiles);
    grana_raster::write(&plan.output.join(SHEET), &sheet).map_err(String::from)?;
    let mut lines = vec![grana_measure::csv::line(&[
        "cell", "group", "dataset", "status",
    ])];
    for (index, dataset) in plan.datasets.iter().enumerate() {
        let status = rows[index].as_ref().map_or("skipped", |row| row.status);
        let cell = cells[index].map_or_else(String::new, |cell| cell.to_string());
        lines.push(grana_measure::csv::line(&[
            &cell,
            dataset.group(),
            &dataset.name(),
            status,
        ]));
    }
    std::fs::write(plan.output.join(MANIFEST), lines.concat())
        .map_err(|error| format!("failed to write the quality sheet manifest: {error}"))
}
