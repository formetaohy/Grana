use std::sync::Arc;

use grana_display::{linear_to_srgb, srgb_to_linear};
use grana_image::{Image, Source};
use grana_raster::Bitmap;
use grana_volume::{Extent, Pyramid};

const PLANE_LIMIT: u64 = 8 << 20;

const EXPOSURE: f32 = 1.6;

pub fn size(fields: usize) -> u32 {
    match fields {
        0..=400 => 96,
        401..=900 => 64,
        _ => 48,
    }
}

pub fn field(source: &Source, size: u32) -> Result<Bitmap, String> {
    assert!(size > 0, "a preview covers at least one pixel");
    let image = Image::open_with_cache(source, None).map_err(|error| error.to_string())?;
    let pyramid = image.pyramid();
    let level = level(pyramid, size);
    let extent = pyramid.level(level).extent();
    let [width, height] = fit(extent, size);
    let mut pixels = vec![[0.0f32; 3]; width as usize * height as usize];
    for channel in 0..pyramid.channel_count() {
        let info = &pyramid.channels()[channel];
        let data = image
            .read_region(
                0,
                level,
                channel,
                &[0..extent.x(), 0..extent.y(), 0..extent.z()],
                &grana_warp::Warp::IDENTITY,
            )
            .map_err(|error| error.to_string())?;
        let projected = project(&data, extent, width, height);
        let tint = srgb_to_linear(info.tint());
        let window = info.window();
        for (pixel, value) in pixels.iter_mut().zip(projected) {
            let unit = window.unit(value).clamp(0.0, 1.0);
            for axis in 0..3 {
                pixel[axis] += tint[axis] * unit;
            }
        }
    }
    let mut bytes = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        for channel in pixel {
            let mapped = 1.0 - (-EXPOSURE * channel).exp();
            bytes.push((linear_to_srgb(mapped.clamp(0.0, 1.0)) * 255.0).round() as u8);
        }
        bytes.push(255);
    }
    Ok(Bitmap::new([width, height], Arc::from(bytes)))
}

fn level(pyramid: &Pyramid, size: u32) -> usize {
    let count = pyramid.level_count();
    let mut chosen = count - 1;
    for index in 0..count {
        let extent = pyramid.level(index).extent();
        if plane(extent) > PLANE_LIMIT {
            continue;
        }
        chosen = index;
        if extent.x() < size || extent.y() < size {
            break;
        }
    }
    chosen
}

fn plane(extent: Extent) -> u64 {
    u64::from(extent.x()) * u64::from(extent.y())
}

fn fit(extent: Extent, size: u32) -> [u32; 2] {
    let longest = extent.x().max(extent.y());
    if longest <= size {
        return [extent.x(), extent.y()];
    }
    let scale = f64::from(size) / f64::from(longest);
    [
        ((f64::from(extent.x()) * scale).round() as u32).max(1),
        ((f64::from(extent.y()) * scale).round() as u32).max(1),
    ]
}

fn project(data: &grana_volume::VoxelData, extent: Extent, width: u32, height: u32) -> Vec<f32> {
    let mut plane = vec![f32::NEG_INFINITY; width as usize * height as usize];
    for y in 0..height {
        let rows = band(y, height, extent.y());
        for x in 0..width {
            let columns = band(x, width, extent.x());
            let mut maximum = f32::NEG_INFINITY;
            for z in 0..extent.z() {
                for row in rows.clone() {
                    for column in columns.clone() {
                        let value = data.value(extent.index(column, row, z) as usize);
                        if value.is_finite() && value > maximum {
                            maximum = value;
                        }
                    }
                }
            }
            plane[(x + width * y) as usize] = maximum;
        }
    }
    plane
        .into_iter()
        .map(|value| if value.is_finite() { value } else { 0.0 })
        .collect()
}

fn band(index: u32, count: u32, extent: u32) -> std::ops::Range<u32> {
    let start = ((u64::from(index) * u64::from(extent)) / u64::from(count)) as u32;
    let end = (((u64::from(index) + 1) * u64::from(extent)) / u64::from(count)) as u32;
    start..end.max(start + 1).min(extent)
}
