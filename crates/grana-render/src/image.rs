use std::sync::mpsc;

use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Device, Extent3d, MapMode, Origin3d,
    PollType, Queue, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureFormat,
};

const COPY_ALIGNMENT: u32 = 256;

pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
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
}

pub struct Image16 {
    size: [u32; 2],
    pixels: Vec<u16>,
}

impl Image16 {
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }
}

pub fn read(
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    size: [u32; 2],
    format: TextureFormat,
) -> Image {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a captured image covers at least one pixel"
    );
    assert!(
        matches!(
            format,
            TextureFormat::Rgba8Unorm
                | TextureFormat::Rgba8UnormSrgb
                | TextureFormat::Bgra8Unorm
                | TextureFormat::Bgra8UnormSrgb
        ),
        "a captured image is an eight bit colour target"
    );
    let mut pixels = capture(device, queue, texture, size, 4);
    if matches!(
        format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    ) {
        let (pixels, _) = pixels.as_chunks_mut::<4>();
        for pixel in pixels {
            pixel.swap(0, 2);
        }
    }
    Image {
        width: size[0],
        height: size[1],
        pixels,
    }
}

pub fn read16(device: &Device, queue: &Queue, texture: &Texture, size: [u32; 2]) -> Image16 {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a captured wide image covers at least one pixel"
    );
    let bytes = capture(device, queue, texture, size, 16);
    let pixels = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .map(|value| (value.clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect();
    Image16 { size, pixels }
}

fn capture(
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    size: [u32; 2],
    bytes_per_pixel: u32,
) -> Vec<u8> {
    let row = size[0] * bytes_per_pixel;
    let stride = row.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT;
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("grana.image"),
        size: u64::from(stride) * u64::from(size[1]),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("grana.image.copy"),
    });
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size[1]),
            },
        },
        Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(PollType::wait_indefinitely())
        .expect("the graphics device failed while reading back an image");
    receiver
        .recv()
        .expect("the image mapping callback runs once")
        .expect("the image staging buffer is mappable");
    let mapped = slice
        .get_mapped_range()
        .expect("the mapped image buffer stays mapped");
    let mut pixels = Vec::with_capacity((row * size[1]) as usize);
    for line in 0..size[1] as usize {
        let start = line * stride as usize;
        pixels.extend_from_slice(&mapped[start..start + row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    pixels
}
