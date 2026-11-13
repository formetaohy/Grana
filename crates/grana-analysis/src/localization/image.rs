use super::LocalizationFrame;
use super::depth::LocalizationDepth;
use super::grid::Grid;
use super::group::Groups;
use super::report::LocalizationGeometry;
use super::tone::{Histogram, Tone};

const SIDE: usize = 512;

const HIGHLIGHT: f32 = 0.999;

pub struct LocalizationRenderer<'a> {
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    groups: Groups,
    cells: Cells,
    tone: Option<Tone>,
    progress: &'a dyn Fn(usize, usize) -> bool,
    layer: usize,
    band: usize,
    row: usize,
    surveyed: usize,
    buffer: Vec<u8>,
}

impl<'a> LocalizationRenderer<'a> {
    pub fn new(
        frames: &[LocalizationFrame<'_>],
        geometry: LocalizationGeometry,
        depth: LocalizationDepth,
        groups: Groups,
        progress: &'a dyn Fn(usize, usize) -> bool,
    ) -> Result<Self, String> {
        if !geometry.renderable() {
            let [width, height] = geometry.size();
            return Err(format!(
                "the localization image would span {width}脳{height} pixels; raise the pixel size"
            ));
        }
        let cells = Cells::build(frames, geometry, depth, &groups, progress)?;
        Ok(Self {
            geometry,
            depth,
            groups,
            cells,
            tone: None,
            progress,
            layer: 0,
            band: 0,
            row: 0,
            surveyed: 0,
            buffer: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.geometry.size()[0]
    }

    pub fn height(&self) -> u32 {
        self.geometry.size()[1]
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn survey(&mut self) -> Result<f32, String> {
        let mut maximum = 0.0f32;
        for layer in 0..self.depth.layers() {
            self.visit_layer(layer, |grid| {
                maximum = maximum.max(grid.maximum());
            })?;
        }
        let highlight = if maximum > 0.0 {
            let mut histogram = Histogram::new(maximum);
            for layer in 0..self.depth.layers() {
                self.visit_layer(layer, |grid| histogram.push(grid.values()))?;
            }
            histogram.highlight(HIGHLIGHT)
        } else {
            0.0
        };
        let pixel = self.geometry.pixel();
        self.at_density(highlight / (pixel * pixel));
        Ok(self.density())
    }

    pub fn at_density(&mut self, density: f32) {
        self.tone = Some(Tone::new(density, self.geometry.pixel()));
    }

    pub fn density(&self) -> f32 {
        self.tone.as_ref().map_or(0.0, Tone::density)
    }

    pub fn begin(&mut self, layer: u32) -> Result<(), String> {
        if layer >= self.depth.layers() {
            return Err(format!(
                "the localization image holds {} slices and slice {layer} was asked for",
                self.depth.layers()
            ));
        }
        self.layer = layer as usize;
        self.band = 0;
        self.row = 0;
        self.buffer.clear();
        Ok(())
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
        if self.band >= self.cells.rows {
            return Err("the localization image rendered past its rows".to_string());
        }
        let width = self.geometry.size()[0] as usize;
        let height = self.geometry.size()[1] as usize;
        let start = self.band * SIDE;
        let rows = (height - start).min(SIDE);
        let mut buffer = std::mem::take(&mut self.buffer);
        buffer.clear();
        buffer.resize(width * rows * 4, 0);
        if let Some(tone) = self.tone.as_ref() {
            self.paint(start, rows, width, &mut buffer, tone);
        } else {
            self.buffer = buffer;
            return Err("the localization image renders before its density is known".to_string());
        }
        self.buffer = buffer;
        self.band += 1;
        self.row = 0;
        let bands = self.cells.rows;
        let layers = self.depth.layers() as usize;
        if !(self.progress)(self.layer * bands + self.band, layers * bands) {
            return Err("the localization image render was cancelled".to_string());
        }
        Ok(())
    }

    fn paint(&self, start: usize, rows: usize, width: usize, buffer: &mut [u8], tone: &Tone) {
        let border = self.geometry.border() as usize;
        let reach = border.div_ceil(SIDE) as i64;
        for column in 0..self.cells.columns {
            let column_start = column * SIDE;
            let mut grid = self.cells.grid(column, start, rows, &self.geometry);
            self.cells.add(
                &mut grid,
                &self.groups,
                self.layer,
                column,
                self.band,
                reach,
            );
            grid.blur(self.geometry.blur());
            let grid_width = grid.size()[0] as usize;
            let painted = (width - column_start).min(SIDE);
            for line in 0..rows {
                let target = (line * width + column_start) * 4;
                for across in 0..painted {
                    let index = (line + border) * grid_width + across + border;
                    let [red, green, blue] = tone.shade(grid.values()[index], grid.chroma(index));
                    let at = target + across * 4;
                    buffer[at..at + 4].copy_from_slice(&[red, green, blue, 255]);
                }
            }
        }
    }

    fn visit_layer(&mut self, layer: u32, mut visit: impl FnMut(&Grid)) -> Result<(), String> {
        let height = self.geometry.size()[1] as usize;
        let reach = (self.geometry.border() as usize).div_ceil(SIDE) as i64;
        let mut band = 0usize;
        while band * SIDE < height {
            let start = band * SIDE;
            let rows = (height - start).min(SIDE);
            for column in 0..self.cells.columns {
                let mut grid = self.cells.grid(column, start, rows, &self.geometry);
                self.cells
                    .add(&mut grid, &self.groups, layer as usize, column, band, reach);
                grid.blur(self.geometry.blur());
                visit(&grid);
            }
            band += 1;
        }
        self.surveyed += 1;
        if !(self.progress)(self.surveyed, 2 * self.depth.layers() as usize) {
            return Err("the localization image render was cancelled".to_string());
        }
        Ok(())
    }
}

struct Cells {
    columns: usize,
    rows: usize,
    chromatic: bool,
    layers: Vec<Layer>,
}

struct Layer {
    starts: Vec<u32>,
    centers: Vec<[f32; 2]>,
    tints: Vec<u8>,
}

impl Cells {
    fn build(
        frames: &[LocalizationFrame<'_>],
        geometry: LocalizationGeometry,
        depth: LocalizationDepth,
        groups: &Groups,
        progress: &dyn Fn(usize, usize) -> bool,
    ) -> Result<Self, String> {
        let width = geometry.size()[0] as usize;
        let height = geometry.size()[1] as usize;
        let columns = width.div_ceil(SIDE);
        let rows = height.div_ceil(SIDE);
        let cells = columns * rows;
        let layers = depth.layers() as usize;
        let chromatic = groups.chromatic();
        let mut cursors = vec![0u32; cells * layers];
        for (done, frame) in frames.iter().enumerate() {
            for (index, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let center = spot.center();
                let tile = tile_of([center[0], center[1]], &geometry, columns);
                let at = depth.layer_of(center[2]) as usize * cells + tile;
                cursors[at] = cursors[at].checked_add(1).ok_or_else(|| {
                    "the localizations crowd a tile with more points than a raster addresses"
                        .to_string()
                })?;
            }
            if !progress(done + 1, frames.len()) {
                return Err("the localization image render was cancelled".to_string());
            }
        }
        let mut layers_out = Vec::with_capacity(layers);
        for layer in 0..layers {
            let mut starts = Vec::with_capacity(cells + 1);
            starts.push(0u32);
            let mut held = 0u32;
            for cell in 0..cells {
                held = held
                    .checked_add(cursors[layer * cells + cell])
                    .ok_or_else(|| {
                        "a localization slice holds more points than a raster addresses".to_string()
                    })?;
                starts.push(held);
            }
            for cell in 0..cells {
                cursors[layer * cells + cell] = starts[cell];
            }
            layers_out.push(Layer {
                starts,
                centers: vec![[0.0; 2]; held as usize],
                tints: vec![0u8; held as usize],
            });
        }
        for frame in frames {
            for (index, spot) in frame.spots().iter().enumerate() {
                if !frame.visible(index) {
                    continue;
                }
                let center = spot.center();
                let tile = tile_of([center[0], center[1]], &geometry, columns);
                let layer = depth.layer_of(center[2]) as usize;
                let at = layer * cells + tile;
                let cursor = cursors[at] as usize;
                cursors[at] += 1;
                let held = &mut layers_out[layer];
                held.centers[cursor] = [center[0], center[1]];
                held.tints[cursor] = if chromatic {
                    groups.tint_slot(spot.group())
                } else {
                    0
                };
            }
        }
        Ok(Self {
            columns,
            rows,
            chromatic,
            layers: layers_out,
        })
    }

    fn grid(
        &self,
        column: usize,
        start: usize,
        rows: usize,
        geometry: &LocalizationGeometry,
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

    fn add(
        &self,
        grid: &mut Grid,
        groups: &Groups,
        layer: usize,
        column: usize,
        band: usize,
        reach: i64,
    ) {
        let Some(held) = self.layers.get(layer) else {
            return;
        };
        for row in -reach..=reach {
            let band = band as i64 + row;
            if !(0..self.rows as i64).contains(&band) {
                continue;
            }
            for across in -reach..=reach {
                let column = column as i64 + across;
                if !(0..self.columns as i64).contains(&column) {
                    continue;
                }
                let tile = band as usize * self.columns + column as usize;
                let from = held.starts[tile] as usize;
                let to = held.starts[tile + 1] as usize;
                for index in from..to {
                    let tint = self.chromatic.then(|| groups.slot_tint(held.tints[index]));
                    grid.add(held.centers[index], 1.0, tint);
                }
            }
        }
    }
}

fn tile_of(center: [f32; 2], geometry: &LocalizationGeometry, columns: usize) -> usize {
    let pixel = geometry.pixel();
    let origin = geometry.origin();
    let width = geometry.size()[0] as usize;
    let height = geometry.size()[1] as usize;
    let column = (((center[0] - origin[0]) / pixel) as usize).min(width - 1) / SIDE;
    let row = (((center[1] - origin[1]) / pixel) as usize).min(height - 1) / SIDE;
    row * columns + column
}
