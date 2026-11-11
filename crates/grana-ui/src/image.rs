use std::collections::HashMap;

use grana_raster::Bitmap;
use wgpu::{
    Device, Extent3d, FilterMode, Queue, Sampler, SamplerDescriptor, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

const PADDING: u32 = 1;
const SIZE: u32 = 3072;

#[derive(Clone, Copy, Debug)]
struct Shelf {
    top: u32,
    height: u32,
    cursor: u32,
}

struct Upload {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

pub struct ImageAtlas {
    device: Option<Device>,
    texture: Option<Texture>,
    view: Option<TextureView>,
    sampler: Option<Sampler>,
    shelves: Vec<Shelf>,
    slots: HashMap<u64, [f32; 4]>,
    uploads: Vec<Upload>,
}

impl ImageAtlas {
    pub fn new(device: &Device) -> Self {
        let placeholder = device.create_texture(&TextureDescriptor {
            label: Some("grana.ui.images.placeholder"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = placeholder.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.ui.images.sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            device: Some(device.clone()),
            texture: None,
            view: Some(view),
            sampler: Some(sampler),
            shelves: Vec::new(),
            slots: HashMap::new(),
            uploads: Vec::new(),
        }
    }

    pub(crate) fn detached() -> Self {
        Self {
            device: None,
            texture: None,
            view: None,
            sampler: None,
            shelves: Vec::new(),
            slots: HashMap::new(),
            uploads: Vec::new(),
        }
    }

    pub fn view(&self) -> &TextureView {
        self.view
            .as_ref()
            .expect("an image atlas is bound to a device")
    }

    pub fn sampler(&self) -> &Sampler {
        self.sampler
            .as_ref()
            .expect("an image atlas is bound to a device")
    }

    pub fn slot(&mut self, bitmap: &Bitmap) -> Option<[f32; 4]> {
        self.device.as_ref()?;
        if let Some(slot) = self.slots.get(&bitmap.identity()) {
            return Some(*slot);
        }
        self.reserve()?;
        let [width, height] = bitmap.size();
        let tile = (width + PADDING * 2, height + PADDING * 2);
        if tile.0 > SIZE || tile.1 > SIZE {
            return None;
        }
        let (x, y) = self.allocate(tile.0, tile.1)?;
        let mut data = vec![0u8; (tile.0 * tile.1) as usize * 4];
        for row in 0..height {
            let source = (row * width) as usize * 4;
            let destination = (((row + PADDING) * tile.0 + PADDING) as usize) * 4;
            data[destination..destination + width as usize * 4]
                .copy_from_slice(&bitmap.pixels()[source..source + width as usize * 4]);
        }
        self.uploads.push(Upload {
            x,
            y,
            width: tile.0,
            height: tile.1,
            data,
        });
        let slot = [
            (x + PADDING) as f32 / SIZE as f32,
            (y + PADDING) as f32 / SIZE as f32,
            (x + PADDING + width) as f32 / SIZE as f32,
            (y + PADDING + height) as f32 / SIZE as f32,
        ];
        self.slots.insert(bitmap.identity(), slot);
        Some(slot)
    }

    pub fn flush(&mut self, queue: &Queue) {
        let Some(texture) = self.texture.as_ref() else {
            return;
        };
        for upload in self.uploads.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.x,
                        y: upload.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.width * 4),
                    rows_per_image: Some(upload.height),
                },
                Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn reserve(&mut self) -> Option<()> {
        if self.texture.is_some() {
            return Some(());
        }
        let device = self.device.as_ref()?;
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.ui.images"),
            size: Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.view = Some(texture.create_view(&TextureViewDescriptor::default()));
        self.texture = Some(texture);
        Some(())
    }

    fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        for shelf in self.shelves.iter_mut() {
            if shelf.height >= height && shelf.cursor + width + PADDING <= SIZE {
                let x = shelf.cursor;
                shelf.cursor += width + PADDING;
                return Some((x, shelf.top));
            }
        }
        let top = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.top + shelf.height);
        if top + height > SIZE {
            return None;
        }
        self.shelves.push(Shelf {
            top,
            height,
            cursor: width + PADDING,
        });
        Some((0, top))
    }
}
