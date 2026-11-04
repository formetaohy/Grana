#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Raster {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert!(
            width > 0 && height > 0,
            "a raster covers at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * 4,
            "a raster holds four bytes per pixel"
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Self {
        assert!(
            width > 0 && height > 0,
            "a raster covers at least one pixel"
        );
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = color;
        }
        Self::new(width, height, pixels)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub(crate) fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(
            x < self.width && y < self.height,
            "a pixel lies inside its raster"
        );
        let index = (x + self.width * y) as usize * 4;
        self.pixels[index..index + 4]
            .try_into()
            .expect("a pixel holds four channels")
    }

    pub fn paste(&mut self, source: &Raster, at: [u32; 2]) {
        assert!(
            at[0] + source.width <= self.width && at[1] + source.height <= self.height,
            "a pasted raster fits inside its target"
        );
        for y in 0..source.height {
            for x in 0..source.width {
                let above = source.pixel(x, y);
                let slot = ((at[0] + x) + self.width * (at[1] + y)) as usize * 4;
                let below: [u8; 4] = self.pixels[slot..slot + 4]
                    .try_into()
                    .expect("a pixel holds four channels");
                self.pixels[slot..slot + 4].copy_from_slice(&over(below, above));
            }
        }
    }

    pub fn paste_premultiplied(&mut self, source: &Raster, at: [u32; 2]) {
        assert!(
            at[0] + source.width <= self.width && at[1] + source.height <= self.height,
            "a pasted raster fits inside its target"
        );
        for y in 0..source.height {
            for x in 0..source.width {
                let above = source.pixel(x, y);
                let slot = ((at[0] + x) + self.width * (at[1] + y)) as usize * 4;
                let below: [u8; 4] = self.pixels[slot..slot + 4]
                    .try_into()
                    .expect("a pixel holds four channels");
                self.pixels[slot..slot + 4].copy_from_slice(&over_premultiplied(below, above));
            }
        }
    }

    pub fn scaled(&self, width: u32, height: u32) -> Raster {
        assert!(
            width > 0 && height > 0,
            "a raster covers at least one pixel"
        );
        if [width, height] == self.size() {
            return self.clone();
        }
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        if width <= self.width && height <= self.height {
            self.reduce(&mut pixels, width, height);
        } else {
            self.stretch(&mut pixels, width, height);
        }
        Raster::new(width, height, pixels)
    }

    fn reduce(&self, out: &mut [u8], width: u32, height: u32) {
        let scale_x = f64::from(self.width) / f64::from(width);
        let scale_y = f64::from(self.height) / f64::from(height);
        for y in 0..height {
            let top = f64::from(y) * scale_y;
            let bottom = top + scale_y;
            let first = top.floor() as u32;
            let last = (bottom.ceil() as u32).min(self.height);
            for x in 0..width {
                let left = f64::from(x) * scale_x;
                let right = left + scale_x;
                let start = left.floor() as u32;
                let end = (right.ceil() as u32).min(self.width);
                let mut alpha = 0.0f64;
                let mut weight = 0.0f64;
                let mut sum = [0.0f64; 3];
                for sy in first..last {
                    let rows = (f64::from(sy + 1).min(bottom) - f64::from(sy).max(top)).max(0.0);
                    if rows <= 0.0 {
                        continue;
                    }
                    for sx in start..end {
                        let columns =
                            (f64::from(sx + 1).min(right) - f64::from(sx).max(left)).max(0.0);
                        if columns <= 0.0 {
                            continue;
                        }
                        let sample = self.pixel(sx, sy);
                        let coverage = f64::from(sample[3]) / 255.0;
                        let area = rows * columns;
                        alpha += coverage * area;
                        weight += area;
                        for channel in 0..3 {
                            sum[channel] += f64::from(linear(sample[channel])) * coverage * area;
                        }
                    }
                }
                let index = (x + width * y) as usize * 4;
                if alpha > 0.0 {
                    for channel in 0..3 {
                        out[index + channel] = encode((sum[channel] / alpha) as f32);
                    }
                    out[index + 3] = (alpha / weight * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    fn stretch(&self, out: &mut [u8], width: u32, height: u32) {
        let scale_x = self.width as f64 / f64::from(width);
        let scale_y = self.height as f64 / f64::from(height);
        for y in 0..height {
            let source_y =
                ((f64::from(y) + 0.5) * scale_y - 0.5).clamp(0.0, f64::from(self.height - 1));
            let top = source_y.floor() as u32;
            let bottom = (top + 1).min(self.height - 1);
            let vertical = source_y - f64::from(top);
            for x in 0..width {
                let source_x =
                    ((f64::from(x) + 0.5) * scale_x - 0.5).clamp(0.0, f64::from(self.width - 1));
                let left = source_x.floor() as u32;
                let right = (left + 1).min(self.width - 1);
                let horizontal = source_x - f64::from(left);
                let corners = [
                    (self.pixel(left, top), (1.0 - horizontal) * (1.0 - vertical)),
                    (self.pixel(right, top), horizontal * (1.0 - vertical)),
                    (self.pixel(left, bottom), (1.0 - horizontal) * vertical),
                    (self.pixel(right, bottom), horizontal * vertical),
                ];
                let index = (x + width * y) as usize * 4;
                out[index..index + 4].copy_from_slice(&blend(&corners));
            }
        }
    }
}

fn blend(corners: &[([u8; 4], f64); 4]) -> [u8; 4] {
    let mut alpha = 0.0f64;
    let mut sum = [0.0f64; 3];
    for (pixel, weight) in corners {
        let coverage = f64::from(pixel[3]) / 255.0;
        alpha += coverage * weight;
        for channel in 0..3 {
            sum[channel] += f64::from(linear(pixel[channel])) * coverage * weight;
        }
    }
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut pixel = [0u8; 4];
    for channel in 0..3 {
        pixel[channel] = encode((sum[channel] / alpha) as f32);
    }
    pixel[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    pixel
}

pub fn composite(under: &Raster, over: &Raster) -> Raster {
    assert_eq!(
        under.size(),
        over.size(),
        "a composited layer covers its scene"
    );
    let mut raster = under.clone();
    raster.paste(over, [0, 0]);
    raster
}

pub fn composite_premultiplied(under: &Raster, over: &Raster) -> Raster {
    assert_eq!(
        under.size(),
        over.size(),
        "a composited layer covers its scene"
    );
    let mut raster = under.clone();
    raster.paste_premultiplied(over, [0, 0]);
    raster
}

pub(crate) fn over(below: [u8; 4], above: [u8; 4]) -> [u8; 4] {
    if above[3] == 255 {
        return above;
    }
    if above[3] == 0 {
        return below;
    }
    let front = f32::from(above[3]) / 255.0;
    let back = f32::from(below[3]) / 255.0;
    let alpha = front + back * (1.0 - front);
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut pixel = [0u8; 4];
    for channel in 0..3 {
        let value = (linear(above[channel]) * front
            + linear(below[channel]) * back * (1.0 - front))
            / alpha;
        pixel[channel] = encode(value);
    }
    pixel[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    pixel
}

fn over_premultiplied(below: [u8; 4], above: [u8; 4]) -> [u8; 4] {
    if above[3] == 255 {
        return above;
    }
    if above[3] == 0 {
        return below;
    }
    let front = f32::from(above[3]) / 255.0;
    let back = f32::from(below[3]) / 255.0;
    let covered = back * (1.0 - front);
    let alpha = front + covered;
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut pixel = [0u8; 4];
    for channel in 0..3 {
        let premultiplied = if front > 0.0 {
            linear_value(f32::from(above[channel]) / 255.0 / front) * front
        } else {
            0.0
        };
        let value = (premultiplied + linear(below[channel]) * covered) / alpha;
        pixel[channel] = encode(value);
    }
    pixel[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    pixel
}

fn linear(value: u8) -> f32 {
    linear_value(f32::from(value) / 255.0)
}

fn linear_value(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}
