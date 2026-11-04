use grana_text::Fonts;

use crate::Raster;

const GAP: u32 = 12;
const MARGIN: u32 = 24;
const BAND: u32 = 42;
const CAPTION: u32 = 50;
const BACKGROUND: [u8; 4] = [18, 20, 24, 255];
const RULE: [u8; 4] = [38, 42, 50, 255];
const TITLE: [u8; 4] = [232, 236, 242, 255];
const DETAIL: [u8; 4] = [138, 146, 158, 255];
const HEADING: [u8; 4] = [246, 248, 252, 255];

const TITLE_SIZE: f32 = 16.0;
const DETAIL_SIZE: f32 = 13.0;
const HEADING_SIZE: f32 = 19.0;
const WEIGHT: f32 = 500.0;

pub struct Cell<'a> {
    pub image: &'a Raster,
    pub title: &'a str,
    pub detail: &'a str,
}

pub struct Group<'a> {
    pub name: &'a str,
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

    pub fn size(&self, groups: &[Group<'_>]) -> [u32; 2] {
        let columns = self.columns(groups);
        let rows = groups
            .iter()
            .map(|group| self.rows(columns, group.cells.len()))
            .sum::<u32>();
        let height =
            groups.len() as u32 * (BAND + GAP) + rows * (self.tile[1] + CAPTION + GAP) + 2 * MARGIN;
        let width = 2 * MARGIN + columns * self.tile[0] + (columns - 1) * GAP;
        [width, height.max(BAND + 2 * MARGIN)]
    }

    pub fn compose(&self, fonts: &mut Fonts, groups: &[Group<'_>]) -> Raster {
        let size = self.size(groups);
        let columns = self.columns(groups);
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
            let rule = Raster::solid(size[0] - 2 * MARGIN, 1, RULE);
            sheet.paste(&rule, [MARGIN, y + BAND - 1]);
            y += BAND + GAP;
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

fn fitted(tile: &Raster, cell: [u32; 2]) -> Raster {
    let scale = (f64::from(cell[0]) / f64::from(tile.width()))
        .min(f64::from(cell[1]) / f64::from(tile.height()));
    let width = ((f64::from(tile.width()) * scale).round() as u32).clamp(1, cell[0]);
    let height = ((f64::from(tile.height()) * scale).round() as u32).clamp(1, cell[1]);
    tile.scaled(width, height)
}
