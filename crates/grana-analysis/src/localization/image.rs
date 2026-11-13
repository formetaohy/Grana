use super::LocalizationFrame;
use super::grid::Grid;
use super::report::LocalizationGeometry;
use super::tone::Tone;

const BAND_CELLS: usize = 1 << 21;

pub struct LocalizationRenderer<'a> {
    frames: &'a [LocalizationFrame<'a>],
    geometry: LocalizationGeometry,
    tone: Tone,
    progress: &'a mut dyn FnMut(usize, usize) -> bool,
    band: usize,
    band_rows: usize,
    pending: Vec<u8>,
    cursor: usize,
}

impl<'a> LocalizationRenderer<'a> {
    pub fn new(
        frames: &'a [LocalizationFrame<'a>],
        geometry: LocalizationGeometry,
        density: f32,
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
        let width = geometry.size()[0] as usize;
        let border = geometry.border() as usize;
        let height = geometry.size()[1] as usize;
        let band_rows = (BAND_CELLS / width).max(2 * border + 1).min(height);
        Ok(Self {
            frames,
            geometry,
            tone: Tone::new(density, geometry.pixel()),
            progress,
            band: 0,
            band_rows,
            pending: Vec::new(),
            cursor: 0,
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
        if self.cursor == self.pending.len() / row.len() {
            self.render()?;
        }
        let width = row.len();
        let start = self.cursor * width;
        row.copy_from_slice(&self.pending[start..start + width]);
        self.cursor += 1;
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let size = self.geometry.size();
        let width = size[0] as usize;
        let height = size[1] as usize;
        let pixel = self.geometry.pixel();
        let border = self.geometry.border() as usize;
        let start = self.band * self.band_rows;
        let end = (start + self.band_rows).min(height);
        let top = start as i64 - border as i64;
        let mut raster = Grid::empty(
            [size[0], (end - start + 2 * border) as u32],
            pixel,
            [
                self.geometry.origin()[0],
                (f64::from(self.geometry.origin()[1]) + top as f64 * f64::from(pixel)) as f32,
            ],
        );
        for frame in self.frames {
            for (index, spot) in frame.spots().iter().enumerate() {
                let visible = frame.visible(index);
                if !visible {
                    continue;
                }
                let center = spot.center();
                raster.add([center[0], center[1]], 1.0);
            }
        }
        raster.blur(self.geometry.blur());
        self.pending.clear();
        self.pending.reserve((end - start) * width * 4);
        for y in start..end {
            let offset = (y - start + border) * width;
            for value in &raster.values()[offset..offset + width] {
                let [red, green, blue] = self.tone.pixel(self.tone.level(*value));
                self.pending.extend_from_slice(&[red, green, blue, 255]);
            }
        }
        self.band += 1;
        self.cursor = 0;
        let total = height.div_ceil(self.band_rows);
        if !(self.progress)(self.band, total) {
            return Err("the localization image render was cancelled".to_string());
        }
        Ok(())
    }
}
