use std::sync::mpsc;

use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, Device, Extent3d, MapMode, Origin3d,
    PollType, Queue, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

const COPY_ALIGNMENT: u64 = 256;

const PICTURE_FORMAT: TextureFormat = crate::target::WIDE_FORMAT;

const PICTURE_BYTES: u32 = 16;

pub struct Pixels<'a> {
    bytes: &'a [u8],
    size: [u32; 2],
    stride: usize,
}

impl<'a> Pixels<'a> {
    pub fn new(bytes: &'a [u8], size: [u32; 2], stride: usize) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a displayed picture covers at least one pixel"
        );
        let row = size[0] as usize * PICTURE_BYTES as usize;
        assert!(
            stride >= row && bytes.len() >= stride * size[1] as usize,
            "a displayed picture covers every pixel of its rows"
        );
        Self {
            bytes,
            size,
            stride,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn row(&self, line: u32) -> &'a [u8] {
        assert!(
            line < self.size[1],
            "a displayed picture row lies inside the picture"
        );
        let start = line as usize * self.stride;
        &self.bytes[start..start + self.size[0] as usize * PICTURE_BYTES as usize]
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

pub fn read<R>(
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    size: [u32; 2],
    read: impl FnOnce(Pixels<'_>) -> R,
) -> R {
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
    let value = {
        let mapped = slice
            .get_mapped_range()
            .expect("the mapped image buffer stays mapped");
        read(Pixels::new(&mapped, size, stride as usize))
    };
    buffer.unmap();
    value
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
                bytes_per_row: Some(u32::try_from(stride).expect("a displayed row fits u32")),
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
