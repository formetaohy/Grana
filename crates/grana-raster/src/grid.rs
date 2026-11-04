use crate::Raster;

const GAP: u32 = 8;
const MARGIN: u32 = 12;
const BACKGROUND: [u8; 4] = [18, 20, 24, 255];
const MAX_COLUMNS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    tile: [u32; 2],
    columns: u32,
    gap: u32,
    margin: u32,
    background: [u8; 4],
}

impl Grid {
    pub fn new(tile: [u32; 2], count: usize) -> Self {
        assert!(
            tile[0] > 0 && tile[1] > 0,
            "a quality sheet holds at least one pixel per tile"
        );
        assert!(count > 0, "a quality sheet holds at least one tile");
        let columns = (0..count)
            .map(|index| index + 1)
            .find(|columns| columns * columns >= count)
            .unwrap_or(count) as u32;
        Self {
            tile,
            columns: columns.clamp(1, MAX_COLUMNS),
            gap: GAP,
            margin: MARGIN,
            background: BACKGROUND,
        }
    }

    pub fn tile(&self) -> [u32; 2] {
        self.tile
    }

    pub fn columns(&self) -> u32 {
        self.columns
    }

    pub fn rows(&self, count: usize) -> u32 {
        assert!(count > 0, "a quality sheet holds at least one tile");
        (count as u32).div_ceil(self.columns)
    }

    pub fn size(&self, count: usize) -> [u32; 2] {
        let rows = self.rows(count);
        [
            2 * self.margin + self.columns * self.tile[0] + (self.columns - 1) * self.gap,
            2 * self.margin + rows * self.tile[1] + (rows - 1) * self.gap,
        ]
    }

    pub fn cell(&self, index: usize) -> [u32; 2] {
        assert!(index < usize::MAX, "a cell lies inside its sheet");
        let column = index as u32 % self.columns;
        let row = index as u32 / self.columns;
        [
            self.margin + column * (self.tile[0] + self.gap),
            self.margin + row * (self.tile[1] + self.gap),
        ]
    }

    pub fn compose(&self, tiles: &[Raster]) -> Raster {
        let size = self.size(tiles.len());
        let mut sheet = Raster::solid(size[0], size[1], self.background);
        for (index, tile) in tiles.iter().enumerate() {
            let fitted = fitted(tile, self.tile);
            let cell = self.cell(index);
            let at = [
                cell[0] + (self.tile[0] - fitted.width()) / 2,
                cell[1] + (self.tile[1] - fitted.height()) / 2,
            ];
            sheet.paste(&fitted, at);
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
