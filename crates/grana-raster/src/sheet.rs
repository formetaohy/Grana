use grana_text::Fonts;

use crate::Raster;
use crate::text::measure;

const GAP: u32 = 12;
const MARGIN: u32 = 24;
const BAND: u32 = 42;
const CAPTION: u32 = 50;
const TABLE: u32 = 8;
const TABLE_ROW: u32 = 22;
const TABLE_HEADER: u32 = 24;
const TABLE_LABEL: u32 = 132;
const TABLE_CELL: u32 = 320;
const TABLE_GAP: u32 = 10;
const BACKGROUND: [u8; 4] = [18, 20, 24, 255];
const RULE: [u8; 4] = [38, 42, 50, 255];
const TITLE: [u8; 4] = [232, 236, 242, 255];
const DETAIL: [u8; 4] = [138, 146, 158, 255];
const HEADING: [u8; 4] = [246, 248, 252, 255];

const TITLE_SIZE: f32 = 16.0;
const DETAIL_SIZE: f32 = 13.0;
const HEADING_SIZE: f32 = 19.0;
const WEIGHT: f32 = 500.0;
const HEADER_WEIGHT: f32 = 600.0;

pub struct Cell<'a> {
    pub image: &'a Raster,
    pub title: &'a str,
    pub detail: &'a str,
    pub accent: Option<[u8; 4]>,
}

pub struct Row {
    pub label: String,
    pub values: Vec<String>,
}

pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

pub struct Group<'a> {
    pub name: &'a str,
    pub table: Option<&'a Table>,
    pub cells: Vec<Cell<'a>>,
}

pub struct Sheet {
    tile: [u32; 2],
    max_columns: u32,
}

impl Sheet {
    pub fn new(tile: [u32; 2], max_columns: u32) -> Self {
        assert!(
            tile[0] > 0 && tile[1] > 0,
            "a quality sheet holds at least one pixel per tile"
        );
        assert!(
            max_columns > 0,
            "a quality sheet lays out at least one column"
        );
        Self { tile, max_columns }
    }

    fn columns(&self, groups: &[Group<'_>]) -> u32 {
        let cells = groups.iter().map(|group| group.cells.len()).sum::<usize>();
        let columns = (cells as f64).sqrt().ceil() as u32;
        columns.clamp(1, self.max_columns)
    }

    fn rows(&self, columns: u32, count: usize) -> u32 {
        (count as u32).div_ceil(columns)
    }

    fn table_height(table: &Table) -> u32 {
        if table.rows.is_empty() || table.columns.is_empty() {
            return 0;
        }
        2 * TABLE + TABLE_HEADER + table.rows.len() as u32 * TABLE_ROW + GAP
    }

    pub fn size(&self, groups: &[Group<'_>]) -> [u32; 2] {
        let columns = self.columns(groups);
        let height = groups
            .iter()
            .map(|group| {
                BAND + GAP
                    + group.table.map_or(0, Self::table_height)
                    + self.rows(columns, group.cells.len()) * (self.tile[1] + CAPTION + GAP)
            })
            .sum::<u32>()
            + 2 * MARGIN;
        let width = 2 * MARGIN + columns * self.tile[0] + (columns - 1) * GAP;
        [width, height.max(BAND + 2 * MARGIN)]
    }

    pub fn compose(&self, fonts: &mut Fonts, groups: &[Group<'_>]) -> Raster {
        let size = self.size(groups);
        let columns = self.columns(groups);
        let width = size[0] - 2 * MARGIN;
        let mut sheet = Raster::solid(size[0], size[1], BACKGROUND);
        let mut y = MARGIN;
        for group in groups {
            sheet.text(
                fonts,
                group.name,
                [MARGIN, y + 10],
                HEADING_SIZE,
                WEIGHT,
                HEADING,
            );
            let rule = Raster::solid(width, 1, RULE);
            sheet.paste(&rule, [MARGIN, y + BAND - 1]);
            y += BAND + GAP;
            if let Some(table) = group.table {
                let height = Self::table_height(table);
                if height > 0 {
                    band(fonts, &mut sheet, table, width, [MARGIN, y]);
                    y += height;
                }
            }
            for (index, cell) in group.cells.iter().enumerate() {
                let column = index as u32 % columns;
                let row = index as u32 / columns;
                let x = MARGIN + column * (self.tile[0] + GAP);
                let top = y + row * (self.tile[1] + CAPTION + GAP);
                let fitted = fitted(cell.image, self.tile);
                let at = [
                    x + (self.tile[0] - fitted.width()) / 2,
                    top + (self.tile[1] - fitted.height()) / 2,
                ];
                sheet.paste(&fitted, at);
                if let Some(accent) = cell.accent {
                    let bar = Raster::solid(3, self.tile[1] + CAPTION - 8, accent);
                    sheet.paste(&bar, [x.saturating_sub(8), top + 4]);
                }
                sheet.text(
                    fonts,
                    cell.title,
                    [x, top + self.tile[1] + 6],
                    TITLE_SIZE,
                    WEIGHT,
                    TITLE,
                );
                if !cell.detail.is_empty() {
                    sheet.text(
                        fonts,
                        cell.detail,
                        [x, top + self.tile[1] + 6 + 20],
                        DETAIL_SIZE,
                        WEIGHT,
                        DETAIL,
                    );
                }
            }
            y += self.rows(columns, group.cells.len()) * (self.tile[1] + CAPTION + GAP);
        }
        sheet
    }
}

fn band(fonts: &mut Fonts, sheet: &mut Raster, table: &Table, width: u32, at: [u32; 2]) {
    let label = TABLE_LABEL.min(width / 3);
    let count = table.columns.len() as u32;
    let gap = TABLE_GAP;
    let span = width - label;
    let cell = span
        .saturating_sub(gap * count.saturating_sub(1))
        .checked_div(count)
        .unwrap_or(0)
        .min(TABLE_CELL);
    let column = |index: u32| at[0] + label + index * (cell + gap);
    for (index, name) in table.columns.iter().enumerate() {
        let text = clipped(fonts, name, cell, HEADER_WEIGHT);
        let ink = measure(fonts, &text, DETAIL_SIZE, HEADER_WEIGHT);
        let x = column(index as u32) + cell.saturating_sub(ink);
        sheet.text(
            fonts,
            &text,
            [x, at[1] + TABLE + 4],
            DETAIL_SIZE,
            HEADER_WEIGHT,
            HEADING,
        );
    }
    let rule = Raster::solid(width, 1, RULE);
    sheet.paste(&rule, [at[0], at[1] + TABLE + TABLE_HEADER - 1]);
    for (index, row) in table.rows.iter().enumerate() {
        let top = at[1] + TABLE + TABLE_HEADER + index as u32 * TABLE_ROW + 4;
        let text = clipped(fonts, &row.label, label.saturating_sub(gap), WEIGHT);
        sheet.text(fonts, &text, [at[0], top], DETAIL_SIZE, WEIGHT, DETAIL);
        for (slot, value) in row.values.iter().take(table.columns.len()).enumerate() {
            let text = clipped(fonts, value, cell, WEIGHT);
            let ink = measure(fonts, &text, DETAIL_SIZE, WEIGHT);
            let x = column(slot as u32) + cell.saturating_sub(ink);
            sheet.text(fonts, &text, [x, top], DETAIL_SIZE, WEIGHT, TITLE);
        }
    }
}

fn clipped(fonts: &mut Fonts, text: &str, width: u32, weight: f32) -> String {
    if measure(fonts, text, DETAIL_SIZE, weight) <= width {
        return text.to_string();
    }
    let mut clipped = String::new();
    for character in text.chars() {
        let mut candidate = clipped.clone();
        candidate.push(character);
        candidate.push('…');
        if measure(fonts, &candidate, DETAIL_SIZE, weight) > width {
            break;
        }
        clipped.push(character);
    }
    if clipped.is_empty() {
        return String::new();
    }
    clipped.push('…');
    clipped
}

fn fitted(tile: &Raster, cell: [u32; 2]) -> Raster {
    let scale = (f64::from(cell[0]) / f64::from(tile.width()))
        .min(f64::from(cell[1]) / f64::from(tile.height()));
    let width = ((f64::from(tile.width()) * scale).round() as u32).clamp(1, cell[0]);
    let height = ((f64::from(tile.height()) * scale).round() as u32).clamp(1, cell[1]);
    tile.scaled(width, height)
}
