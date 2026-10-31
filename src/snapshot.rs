use std::io::BufWriter;
use std::path::Path;

use grana_render::Image;

pub fn write(path: &Path, image: &Image) -> Result<(), String> {
    let write = |detail: String| format!("failed to write {}: {detail}", path.display());
    let file = std::fs::File::create(path).map_err(|error| write(error.to_string()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|error| write(error.to_string()))?;
    writer
        .write_image_data(image.pixels())
        .map_err(|error| write(error.to_string()))?;
    writer.finish().map_err(|error| write(error.to_string()))
}
