use std::path::{Path, PathBuf};

pub fn encode(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    assert_eq!(
        pixels.len(),
        (width as usize * height as usize * 4),
        "an exported image covers its size"
    );
    let mut buffer = Vec::new();
    {
        let write = |detail: String| format!("failed to encode the image: {detail}");
        let mut encoder = png::Encoder::new(&mut buffer, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder
            .write_header()
            .map_err(|error| write(error.to_string()))?;
        writer
            .write_image_data(&straight(pixels))
            .map_err(|error| write(error.to_string()))?;
        writer.finish().map_err(|error| write(error.to_string()))?;
    }
    Ok(buffer)
}

pub fn write_rgba(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    let bytes = encode(width, height, pixels)?;
    let temporary = temporary_path(path);
    let fault = |detail: String| format!("failed to write {}: {detail}", path.display());
    if let Err(error) = std::fs::write(&temporary, &bytes) {
        let _ = std::fs::remove_file(&temporary);
        return Err(fault(error.to_string()));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(fault(error.to_string()));
    }
    Ok(())
}

pub fn composite(under: &[u8], over: &[u8]) -> Vec<u8> {
    assert_eq!(under.len(), over.len(), "the overlay covers the scene");
    let mut pixels = Vec::with_capacity(under.len());
    for (below, above) in under.as_chunks::<4>().0.iter().zip(over.as_chunks::<4>().0) {
        let below_alpha = f32::from(below[3]) / 255.0;
        let above_alpha = f32::from(above[3]) / 255.0;
        let alpha = above_alpha + below_alpha * (1.0 - above_alpha);
        if alpha <= 0.0 {
            pixels.extend_from_slice(&[0; 4]);
            continue;
        }
        for channel in 0..3 {
            let value = (f32::from(above[channel])
                + f32::from(below[channel]) * below_alpha * (1.0 - above_alpha))
                / alpha;
            pixels.push(value.round().clamp(0.0, 255.0) as u8);
        }
        pixels.push((alpha * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    pixels
}

fn straight(pixels: &[u8]) -> Vec<u8> {
    let mut straight = pixels.to_vec();
    for pixel in straight.as_chunks_mut::<4>().0 {
        if pixel[3] == 0 {
            pixel[..3].fill(0);
        }
    }
    straight
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    path.with_file_name(format!(".{name}.{}.part", std::process::id()))
}
