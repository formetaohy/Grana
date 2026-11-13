use super::LocalizationFrame;
use super::grid::Grid;
use super::group::Groups;
use super::report::LocalizationGeometry;
use super::tone::Tone;

const SIDE: usize = 512;

pub struct LocalizationRenderer<'a> {
    geometry: LocalizationGeometry,
    tone: Tone,
    groups: Groups,
    tiles: Tiles,
    progress: &'a mut dyn FnMut(usize, usize) -> bool,
    band: usize,
    row: usize,
    buffer: Vec<u8>,
}

impl<'a> LocalizationRenderer<'a> {
    pub fn new(
        frames: &[LocalizationFrame<'_>],
        geometry: LocalizationGeometry,
        density: f32,
        groups: Groups,
        progress: &'a mut dyn FnMut(usize, usize) -> bool,
    ) -> Result<Self, String> {
        assert!(
            density.is_finite() && density >= 0.0,
            "a localization image carries a non-negative density"
        );
        if !geometry.renderable() {
            let [width, height] = geometry.size();
            return Err(format!(
                "the localization image would span {width}×{height} pixels; raise the pixel size"
            ));
        }
        let tiles = Tiles::build(frames, geometry, &groups);
        let tone = Tone::new(density, geometry.pixel());
        Ok(Self {
            geometry,
            tone,
            groups,
            tiles,
            progress,
            band: 0,
            row: 0,
            buffer: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.geometry.size()[0]
    }

    pub fn height(&self) -> u32 {
        self.geometry.size()[1]
    }

    pub fn row(&mut self, row: &mut [u8]) -> Result<(), String> {
        assert_eq!(
            row.len(),
            self.width() as usize * 4,
            "a rendered localization row holds four bytes per pixel"
        );
        let width = self.width() as usize;
        if self.row * width * 4 >= self.buffer.len() {
            self.render()?;
        }
        let start = self.row * width * 4;
        row.copy_from_slice(&self.buffer[start..start + row.len()]);
        self.row += 1;
        Ok(())
    }

    pub fn row_rgb(&mut self, row: &mut [u8]) -> Result<(), String> {
        assert_eq!(
            row.len(),
            self.width() as usize * 3,
            "an opaque localization row holds three bytes per pixel"
        );
        let width = self.width() as usize;
        if self.row * width * 4 >= self.buffer.len() {
            self.render()?;
        }
        let start = self.row * width * 4;
        let (pixels, remainder) = self.buffer[start..start + width * 4].as_chunks::<4>();
        debug_assert!(remainder.is_empty());
        let (targets, waste) = row.as_chunks_mut::<3>();
        debug_assert!(waste.is_empty());
        for (target, pixel) in targets.iter_mut().zip(pixels) {
            target.copy_from_slice(&pixel[..3]);
        }
        self.row += 1;
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        if self.band >= self.tiles.rows {
            return Err("the localization image rendered past its rows".to_string());
        }
        let width = self.geometry.size()[0] as usize;
        let height = self.geometry.size()[1] as usize;
        let start = self.band * SIDE;
        let rows = (height - start).min(SIDE);
        let mut buffer = std::mem::take(&mut self.buffer);
        buffer.clear();
        buffer.resize(width * rows * 4, 0);
        for column in 0..self.tiles.columns {
            self.paint(column, start, rows, width, &mut buffer);
        }
        self.buffer = buffer;
        self.band += 1;
        self.row = 0;
        if !(self.progress)(self.band, self.tiles.rows) {
            return Err("the localization image render was cancelled".to_string());
        }
        Ok(())
    }

    fn paint(&self, column: usize, start: usize, rows: usize, width: usize, buffer: &mut [u8]) {
        let border = self.geometry.border() as usize;
        let column_start = column * SIDE;
        let mut grid = self.tiles.grid(column, start, rows, self.geometry);
        let reach = border.div_ceil(SIDE) as i64;
        for held in -reach..=reach {
            let row = self.band as i64 + held;
            if !(0..self.tiles.rows as i64).contains(&row) {
                continue;
            }
            for across in -reach..=reach {
                let column = column as i64 + across;
                if !(0..self.tiles.columns as i64).contains(&column) {
                    continue;
                }
                let tile = row as usize * self.tiles.columns + column as usize;
                for index in self.tiles.starts[tile]..self.tiles.starts[tile + 1] {
                    let tint = self
                        .groups
                        .chromatic()
                        .then(|| self.groups.slot_tint(self.tiles.tints[index]));
                    grid.add(self.tiles.centers[index], 1.0, tint);
                }
            }
        }
        grid.blur(self.geometry.blur());
        let grid_width = grid.size()[0] as usize;
        let painted = (width - column_start).min(SIDE);
        for line in 0..rows {
            let target = (line * width + column_start) * 4;
            for across in 0..painted {
                let index = (line + border) * grid_width + across + border;
                let [red, green, blue] = self.tone.shade(grid.values()[index], grid.chroma(index));
                let at = target + across * 4;
                buffer[at..at + 4].copy_from_slice(&[red, green, blue, 255]);
            }
        }
    }
}

struct Tiles {
    columns: usize,
    rows: usize,
    chromatic: bool,
    starts: Vec<usize>,
    centers: Vec<[f32; 2]>,
    tints: Vec<u8>,
}

impl Tiles {
    fn build(
        frames: &[LocalizationFrame<'_>],
        geometry: LocalizationGeometry,
        groups: &Groups,
    ) -> Self {
        let width = geometry.size()[0] as usize;
        let height = geometry.size()[1] as usize;
        let columns = width.div_ceil(SIDE);
        let rows = height.div_ceil(SIDE);
        let cells = columns * rows;
        let chromatic = groups.chromatic();
        let mut starts = vec![0usize; cells + 1];
        for frame in frames {
            for (index, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let center = spot.center();
                let tile = Self::tile([center[0], center[1]], geometry, columns);
                starts[tile + 1] += 1;
            }
        }
        let mut held = 0usize;
        for count in starts.iter_mut() {
            held += *count;
            *count = held;
        }
        let mut cursor = starts.clone();
        let mut centers = vec![[0.0; 2]; held];
        let mut tints = vec![0u8; held];
        for frame in frames {
            for (index, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let center = spot.center();
                let tile = Self::tile([center[0], center[1]], geometry, columns);
                let at = cursor[tile];
                cursor[tile] += 1;
                centers[at] = [center[0], center[1]];
                tints[at] = if chromatic {
                    groups.tint_slot(spot.group())
                } else {
                    0
                };
            }
        }
        Self {
            columns,
            rows,
            chromatic,
            starts,
            centers,
            tints,
        }
    }

    fn tile(center: [f32; 2], geometry: LocalizationGeometry, columns: usize) -> usize {
        let pixel = geometry.pixel();
        let origin = geometry.origin();
        let width = geometry.size()[0] as usize;
        let height = geometry.size()[1] as usize;
        let column = (((center[0] - origin[0]) / pixel) as usize).min(width - 1) / SIDE;
        let row = (((center[1] - origin[1]) / pixel) as usize).min(height - 1) / SIDE;
        row * columns + column
    }

    fn grid(
        &self,
        column: usize,
        start: usize,
        rows: usize,
        geometry: LocalizationGeometry,
    ) -> Grid {
        let border = geometry.border() as usize;
        let pixel = geometry.pixel();
        let origin = geometry.origin();
        let width = geometry.size()[0] as usize;
        let size = [
            ((width - column * SIDE).min(SIDE) + 2 * border) as u32,
            (rows + 2 * border) as u32,
        ];
        let corner = [
            origin[0] + (column * SIDE) as f32 * pixel - border as f32 * pixel,
            origin[1] + start as f32 * pixel - border as f32 * pixel,
        ];
        if self.chromatic {
            Grid::tinted(size, pixel, corner)
        } else {
            Grid::empty(size, pixel, corner)
        }
    }
}
