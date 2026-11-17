use std::sync::mpsc;

use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, Device, Extent3d, MapMode, Origin3d,
    PollType, Queue, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

const COPY_ALIGNMENT: u64 = 256;

const PICTURE_FORMAT: TextureFormat = crate::target::WIDE_FORMAT;

const PICTURE_BYTES: u32 = 16;

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

    pub fn decode(bytes: &[u8], size: [u32; 2], stride: usize) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a decoded image covers at least one pixel"
        );
        let width = size[0] as usize;
        let height = size[1] as usize;
        let row = width * PICTURE_BYTES as usize;
        assert!(
            stride >= row && bytes.len() >= stride * height,
            "a decoded image covers every pixel of its rows"
        );
        let mut pixels = Vec::with_capacity(width * height * 4);
        if stride == row {
            sample(&mut pixels, &bytes[..row * height]);
        } else {
            for line in 0..height {
                let start = line * stride;
                sample(&mut pixels, &bytes[start..start + row]);
            }
        }
        Self { size, pixels }
    }
}

fn sample(pixels: &mut Vec<u16>, bytes: &[u8]) {
    for value in bytes.as_chunks::<4>().0 {
        let value = f32::from_le_bytes(*value);
        pixels.push((value.clamp(0.0, 1.0) * 65535.0).round() as u16);
    }
}

pub fn stride(width: u32) -> u64 {
    assert!(width > 0, "a picture holds at least one column");
    let row = u64::from(width) * u64::from(PICTURE_BYTES);
    row.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT
}

pub fn target(device: &Device, size: [u32; 2]) -> Texture {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a picture target covers at least one pixel"
    );
    device.create_texture(&TextureDescriptor {
        label: Some("grana.picture"),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: PICTURE_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

pub fn read16(device: &Device, queue: &Queue, texture: &Texture, size: [u32; 2]) -> Image16 {
    let stride = stride(size[0]);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("grana.image"),
        size: stride * u64::from(size[1]),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("grana.image.copy"),
    });
    stage(&mut encoder, &buffer, texture, size, stride);
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
    let image = {
        let mapped = slice
            .get_mapped_range()
            .expect("the mapped image buffer stays mapped");
        Image16::decode(&mapped, size, stride as usize)
    };
    buffer.unmap();
    image
}

pub(crate) fn stage(
    encoder: &mut CommandEncoder,
    buffer: &Buffer,
    texture: &Texture,
    size: [u32; 2],
    stride: u64,
) {
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u32::try_from(stride).expect("a captured row fits u32")),
                rows_per_image: Some(size[1]),
            },
        },
        Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}
