use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use grana_ui::Color;

#[derive(Clone, Copy)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn scaled(self, scale: f32) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
            width: self.width * scale,
            height: self.height * scale,
        }
    }
}

pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn fill(&mut self, color: Color) {
        let bytes = bytes(color);
        for pixel in self.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&bytes);
        }
    }

    pub fn rounded(&mut self, area: Area, radius: f32, color: Color) {
        self.shade(area, color, |x, y| {
            let distance = rounded_distance(area, radius, x, y);
            (0.5 - distance).clamp(0.0, 1.0)
        });
    }

    pub fn ring(&mut self, area: Area, radius: f32, thickness: f32, color: Color) {
        let inner = Area::new(
            area.x + thickness,
            area.y + thickness,
            area.width - 2.0 * thickness,
            area.height - 2.0 * thickness,
        );
        self.shade(area, color, |x, y| {
            let outer = (0.5 - rounded_distance(area, radius, x, y)).clamp(0.0, 1.0);
            let hole = (0.5 - rounded_distance(inner, (radius - thickness).max(0.0), x, y))
                .clamp(0.0, 1.0);
            outer - hole
        });
    }

    pub fn circle(&mut self, center: (f32, f32), radius: f32, color: Color) {
        let area = Area::new(
            center.0 - radius,
            center.1 - radius,
            2.0 * radius,
            2.0 * radius,
        );
        self.shade(area, color, |x, y| {
            let distance = ((x - center.0).powi(2) + (y - center.1).powi(2)).sqrt() - radius;
            (0.5 - distance).clamp(0.0, 1.0)
        });
    }

    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), thickness: f32, color: Color) {
        let half = thickness * 0.5;
        let area = Area::new(
            from.0.min(to.0) - half,
            from.1.min(to.1) - half,
            (from.0 - to.0).abs() + thickness,
            (from.1 - to.1).abs() + thickness,
        );
        let direction = (to.0 - from.0, to.1 - from.1);
        let length = direction.0 * direction.0 + direction.1 * direction.1;
        self.shade(area, color, |x, y| {
            let offset = (x - from.0, y - from.1);
            let position =
                ((offset.0 * direction.0 + offset.1 * direction.1) / length).clamp(0.0, 1.0);
            let nearest = (
                from.0 + direction.0 * position,
                from.1 + direction.1 * position,
            );
            let distance = ((x - nearest.0).powi(2) + (y - nearest.1).powi(2)).sqrt() - half;
            (0.5 - distance).clamp(0.0, 1.0)
        });
    }

    pub fn glow(&mut self, center: (f32, f32), radius: f32, color: Color) {
        let area = Area::new(
            center.0 - radius,
            center.1 - radius,
            2.0 * radius,
            2.0 * radius,
        );
        let bounds = self.bounds(area);
        for y in bounds.1..bounds.3 {
            for x in bounds.0..bounds.2 {
                let distance = ((x as f32 + 0.5 - center.0).powi(2)
                    + (y as f32 + 0.5 - center.1).powi(2))
                .sqrt();
                let falloff = (1.0 - distance / radius).clamp(0.0, 1.0);
                if falloff > 0.0 {
                    self.blend(x, y, color.with_alpha(color.alpha * falloff * falloff));
                }
            }
        }
    }

    pub fn blit(&mut self, image: &[u8], size: u32, at: (i32, i32)) {
        for y in 0..size {
            for x in 0..size {
                let index = ((y * size + x) * 4) as usize;
                let alpha = image[index + 3];
                if alpha == 0 {
                    continue;
                }
                let target = (at.0 + x as i32, at.1 + y as i32);
                if target.0 < 0
                    || target.1 < 0
                    || target.0 >= self.width as i32
                    || target.1 >= self.height as i32
                {
                    continue;
                }
                let color =
                    Color::bytes_alpha(image[index], image[index + 1], image[index + 2], alpha);
                self.blend(target.0 as u32, target.1 as u32, color);
            }
        }
    }

    pub fn set(&mut self, x: u32, y: u32, color: Color) {
        let index = ((y * self.width + x) * 4) as usize;
        self.pixels[index..index + 4].copy_from_slice(&bytes(color));
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(io::Error::other)?;
        writer
            .write_image_data(&self.pixels)
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn shade(&mut self, area: Area, color: Color, coverage: impl Fn(f32, f32) -> f32) {
        let bounds = self.bounds(area);
        for y in bounds.1..bounds.3 {
            for x in bounds.0..bounds.2 {
                let covered = coverage(x as f32 + 0.5, y as f32 + 0.5);
                if covered > 0.0 {
                    self.blend(x, y, color.with_alpha(color.alpha * covered));
                }
            }
        }
    }

    fn bounds(&self, area: Area) -> (u32, u32, u32, u32) {
        let left = area.x.floor().clamp(0.0, self.width as f32) as u32;
        let top = area.y.floor().clamp(0.0, self.height as f32) as u32;
        let right = (area.x + area.width).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (area.y + area.height).ceil().clamp(0.0, self.height as f32) as u32;
        (left, top, right, bottom)
    }

    fn blend(&mut self, x: u32, y: u32, color: Color) {
        let index = ((y * self.width + x) * 4) as usize;
        let source = color.to_array();
        let source_rgb = [source[0], source[1], source[2]];
        let target = [
            self.pixels[index] as f32 / 255.0,
            self.pixels[index + 1] as f32 / 255.0,
            self.pixels[index + 2] as f32 / 255.0,
            self.pixels[index + 3] as f32 / 255.0,
        ];
        let target_rgb = [target[0], target[1], target[2]];
        let alpha = source[3] + target[3] * (1.0 - source[3]);
        if alpha <= 0.0 {
            self.pixels[index..index + 4].fill(0);
            return;
        }
        for (channel, (source_channel, target_channel)) in
            source_rgb.iter().zip(target_rgb.iter()).enumerate()
        {
            let value = (source_channel * source[3]
                + target_channel * target[3] * (1.0 - source[3]))
                / alpha;
            self.pixels[index + channel] = (value * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        self.pixels[index + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

pub fn bytes(color: Color) -> [u8; 4] {
    [
        (color.red * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.green * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.blue * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.alpha * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn rounded_distance(area: Area, radius: f32, x: f32, y: f32) -> f32 {
    let limit = (area.width * 0.5).min(area.height * 0.5);
    let radius = radius.clamp(0.0, limit);
    let center = (area.x + area.width * 0.5, area.y + area.height * 0.5);
    let across = ((x - center.0).abs() - (area.width * 0.5 - radius)).max(0.0);
    let down = ((y - center.1).abs() - (area.height * 0.5 - radius)).max(0.0);
    (across * across + down * down).sqrt() - radius
}
