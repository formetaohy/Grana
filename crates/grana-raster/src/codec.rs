use std::io::BufReader;
use std::path::{Path, PathBuf};

use crate::{Error, Raster};

pub fn read(path: &Path) -> Result<Raster, Error> {
    let file = std::fs::File::open(path).map_err(|error| Error::Read {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| Error::Read {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .expect("the image sizes its pixels")
    ];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| Error::Read {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    buffer.truncate(info.buffer_size());
    let pixels = expand(&buffer, info.color_type, path)?;
    Ok(Raster::new(info.width, info.height, pixels))
}

pub fn encode(raster: &Raster) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::new();
    let fault = |detail: String| Error::Encode { detail };
    {
        let mut encoder = png::Encoder::new(&mut buffer, raster.width(), raster.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder
            .write_header()
            .map_err(|error| fault(error.to_string()))?;
        writer
            .write_image_data(&straight(raster.pixels()))
            .map_err(|error| fault(error.to_string()))?;
        writer.finish().map_err(|error| fault(error.to_string()))?;
    }
    Ok(buffer)
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

pub fn write(path: &Path, raster: &Raster) -> Result<(), Error> {
    let bytes = encode(raster)?;
    let temporary = temporary_path(path);
    let fault = |detail: String| Error::Write {
        path: path.to_path_buf(),
        detail,
    };
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

fn expand(bytes: &[u8], color: png::ColorType, path: &Path) -> Result<Vec<u8>, Error> {
    let channels = match color {
        png::ColorType::Rgba => return Ok(bytes.to_vec()),
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => {
            return Err(Error::Unsupported {
                path: path.to_path_buf(),
                detail: "the palette is not expanded to eight bit channels".to_string(),
            });
        }
    };
    if !bytes.len().is_multiple_of(channels) {
        return Err(Error::Unsupported {
            path: path.to_path_buf(),
            detail: "the samples do not fill whole pixels".to_string(),
        });
    }
    let mut pixels = Vec::with_capacity(bytes.len() / channels * 4);
    for sample in bytes.chunks_exact(channels) {
        match channels {
            3 => pixels.extend_from_slice(&[sample[0], sample[1], sample[2], 255]),
            2 => pixels.extend_from_slice(&[sample[0], sample[0], sample[0], sample[1]]),
            _ => pixels.extend_from_slice(&[sample[0], sample[0], sample[0], 255]),
        }
    }
    Ok(pixels)
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    path.with_file_name(format!(".{name}.{}.part", std::process::id()))
}
