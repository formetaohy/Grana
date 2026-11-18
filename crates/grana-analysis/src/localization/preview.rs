use std::sync::Arc;

use grana_display::{linear_to_srgb, srgb_component_to_linear};

use super::LocalizationDepth;
use super::report::LocalizationGeometry;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationPreview {
    origin: [f32; 2],
    extent: [f32; 2],
    size: [u32; 2],
    depth: LocalizationDepth,
    layer: u32,
    pixels: Arc<[u8]>,
}

impl LocalizationPreview {
    pub fn of_layer(
        geometry: LocalizationGeometry,
        depth: LocalizationDepth,
        layer: u32,
        rgb: &[u8],
    ) -> Self {
        let size = geometry.size();
        Self::of_raster(
            geometry.origin(),
            geometry.pixel(),
            [size[0], size[1]],
            depth,
            layer,
            rgb,
        )
    }

    pub fn of_raster(
        origin: [f32; 2],
        pixel: f32,
        size: [u32; 2],
        depth: LocalizationDepth,
        layer: u32,
        rgb: &[u8],
    ) -> Self {
        assert!(
            layer < depth.layers(),
            "a localization preview names one of its layers"
        );
        assert_eq!(
            rgb.len(),
            size[0] as usize * size[1] as usize * 3,
            "a localization preview downsamples one rendered layer"
        );
        let step = size[0].max(size[1]).div_ceil(super::PREVIEW).max(1);
        let coarse = [size[0].div_ceil(step), size[1].div_ceil(step)];
        let mut pixels = vec![0u8; coarse[0] as usize * coarse[1] as usize * 4];
        for row in 0..coarse[1] {
            let top = row * step;
            let bottom = (top + step).min(size[1]);
            for column in 0..coarse[0] {
                let left = column * step;
                let right = (left + step).min(size[0]);
                let mut sum = [0.0f32; 3];
                for line in top..bottom {
                    for across in left..right {
                        let at = ((line * size[0] + across) * 3) as usize;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total +=
                                srgb_component_to_linear(f32::from(rgb[at + channel]) / 255.0);
                        }
                    }
                }
                let count = ((right - left) * (bottom - top)) as f32;
                let at = ((row * coarse[0] + column) * 4) as usize;
                for (channel, total) in sum.iter().enumerate() {
                    let linear = (total / count).clamp(0.0, 1.0);
                    pixels[at + channel] =
                        (linear_to_srgb(linear) * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                pixels[at + 3] = 255;
            }
        }
        Self {
            origin,
            extent: [size[0] as f32 * pixel, size[1] as f32 * pixel],
            size: coarse,
            depth,
            layer,
            pixels: pixels.into(),
        }
    }

    pub fn of_bitmap(
        origin: [f32; 2],
        extent: [f32; 2],
        size: [u32; 2],
        depth: LocalizationDepth,
        layer: u32,
        pixels: Arc<[u8]>,
    ) -> Self {
        assert!(
            layer < depth.layers(),
            "a localization preview names one of its layers"
        );
        assert_eq!(
            pixels.len(),
            size[0] as usize * size[1] as usize * 4,
            "a localization preview holds four bytes per pixel"
        );
        Self {
            origin,
            extent,
            size,
            depth,
            layer,
            pixels,
        }
    }

    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    pub fn extent(&self) -> [f32; 2] {
        self.extent
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn layer(&self) -> u32 {
        self.layer
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn shared(&self) -> Arc<[u8]> {
        Arc::clone(&self.pixels)
    }
}
