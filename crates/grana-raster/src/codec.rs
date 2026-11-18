use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

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
    grana_file::write(path, &bytes).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

type StreamRow<'a> = &'a mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>;

type Stream16Row<'a> = &'a mut dyn FnMut(u32, &mut [u16]) -> Result<(), String>;

pub fn write_stream(path: &Path, size: [u32; 2], row: StreamRow<'_>) -> Result<(), Error> {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a streamed image covers at least one pixel"
    );
    let write = |detail: String| Error::Write {
        path: path.to_path_buf(),
        detail,
    };
    let atomic = grana_file::Atomic::file(path).map_err(|error| write(error.to_string()))?;
    let file = std::fs::File::create(atomic.staging()).map_err(|error| write(error.to_string()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|error| write(error.to_string()))?;
    let mut stream = writer
        .stream_writer()
        .map_err(|error| write(error.to_string()))?;
    let mut buffer = vec![0u8; size[0] as usize * 4];
    for y in 0..size[1] {
        row(y, &mut buffer).map_err(|detail| Error::Encode { detail })?;
        stream
            .write_all(&buffer)
            .map_err(|error| write(error.to_string()))?;
    }
    stream.finish().map_err(|error| write(error.to_string()))?;
    atomic.commit().map_err(|error| write(error.to_string()))
}

pub fn encode16_stream(
    size: [u32; 2],
    samples: usize,
    row: Stream16Row<'_>,
) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::new();
    encode16_into(&mut buffer, size, samples, row)?;
    Ok(buffer)
}

pub fn write16_stream(
    path: &Path,
    size: [u32; 2],
    samples: usize,
    row: Stream16Row<'_>,
) -> Result<(), Error> {
    let write = |detail: String| Error::Write {
        path: path.to_path_buf(),
        detail,
    };
    let atomic = grana_file::Atomic::file(path).map_err(|error| write(error.to_string()))?;
    let file = std::fs::File::create(atomic.staging()).map_err(|error| write(error.to_string()))?;
    encode16_into(BufWriter::new(file), size, samples, row)
        .map_err(|error| write(error.to_string()))?;
    atomic.commit().map_err(|error| write(error.to_string()))
}

fn encode16_into(
    sink: impl Write,
    size: [u32; 2],
    samples: usize,
    row: Stream16Row<'_>,
) -> Result<(), Error> {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a sixteen bit image covers at least one pixel"
    );
    assert!(
        samples == 3 || samples == 4,
        "a sixteen bit image carries three or four samples per pixel"
    );
    let fault = |detail: String| Error::Encode { detail };
    let mut encoder = png::Encoder::new(sink, size[0], size[1]);
    encoder.set_color(if samples == 4 {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|error| fault(error.to_string()))?;
    let mut stream = writer
        .stream_writer()
        .map_err(|error| fault(error.to_string()))?;
    let mut samples_row = vec![0u16; size[0] as usize * samples];
    let mut buffer = vec![0u8; samples_row.len() * 2];
    for line in 0..size[1] {
        row(line, &mut samples_row).map_err(|detail| Error::Encode { detail })?;
        for (slot, sample) in buffer.as_chunks_mut::<2>().0.iter_mut().zip(&samples_row) {
            slot.copy_from_slice(&sample.to_be_bytes());
        }
        stream
            .write_all(&buffer)
            .map_err(|error| fault(error.to_string()))?;
    }
    stream.finish().map_err(|error| fault(error.to_string()))
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
