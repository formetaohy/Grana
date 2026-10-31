use std::collections::HashMap;

use wgpu::{
    Device, FilterMode, Queue, Sampler, SamplerDescriptor, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::font::{Fonts, GlyphRaster};
use crate::theme::TypeStyle;

const PADDING: u32 = 1;
const INITIAL_SIZE: u32 = 1024;
const MAXIMUM_SIZE: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    face: u16,
    id: u16,
    pixels: u16,
    weight: u16,
    hint: bool,
    subpixel_x: u8,
    subpixel_y: u8,
}

impl GlyphKey {
    fn new(face: usize, id: u16, style: TypeStyle, pixels_per_point: f32, hint: bool) -> Self {
        let pixels = (style.size * pixels_per_point * 4.0)
            .round()
            .clamp(1.0, 65535.0) as u16;
        let weight = (style.weight * 4.0).round().clamp(0.0, 65535.0) as u16;
        Self {
            face: face as u16,
            id,
            pixels,
            weight,
            hint,
            subpixel_x: 0,
            subpixel_y: 0,
        }
    }

    fn with_subpixel(mut self, x: u8, y: u8) -> Self {
        self.subpixel_x = x;
        self.subpixel_y = y;
        self
    }
}

pub struct GlyphRequest {
    pub face: usize,
    pub id: u16,
    pub style: TypeStyle,
    pub pixels_per_point: f32,
    pub hint: bool,
    pub position: (f32, f32),
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphSlot {
    pub uv: [f32; 4],
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

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

pub struct Atlas {
    device: Device,
    size: u32,
    texture: Texture,
    view: TextureView,
    sampler: Sampler,
    shelves: Vec<Shelf>,
    slots: HashMap<GlyphKey, GlyphSlot>,
    uploads: Vec<Upload>,
    resets: u32,
}

impl Atlas {
    pub fn new(device: &Device) -> Self {
        let (texture, view, sampler) = create(device, INITIAL_SIZE);
        Self {
            device: device.clone(),
            size: INITIAL_SIZE,
            texture,
            view,
            sampler,
            shelves: Vec::new(),
            slots: HashMap::new(),
            uploads: Vec::new(),
            resets: 0,
        }
    }

    pub fn view(&self) -> &TextureView {
        &self.view
    }

    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn resets(&self) -> u32 {
        self.resets
    }

    pub fn slot(&mut self, fonts: &mut Fonts, request: GlyphRequest) -> Option<GlyphSlot> {
        let fraction_x = request.position.0.rem_euclid(1.0);
        let fraction_y = request.position.1.rem_euclid(1.0);
        let quantized_x = (fraction_x * 4.0).round() as u8 & 3;
        let quantized_y = (fraction_y * 4.0).round() as u8 & 3;
        let key = GlyphKey::new(
            request.face,
            request.id,
            request.style,
            request.pixels_per_point,
            request.hint,
        )
        .with_subpixel(quantized_x, quantized_y);
        if let Some(slot) = self.slots.get(&key) {
            return Some(*slot);
        }
        let raster = fonts.rasterize(
            request.face,
            request.id,
            request.style,
            request.pixels_per_point,
            request.hint,
            swash::zeno::Vector::new(quantized_x as f32 / 4.0, quantized_y as f32 / 4.0),
        )?;
        let slot = self.insert(key, &raster)?;
        self.slots.insert(key, slot);
        Some(slot)
    }

    fn insert(&mut self, _key: GlyphKey, raster: &GlyphRaster) -> Option<GlyphSlot> {
        let width = raster.width + PADDING * 2;
        let height = raster.height + PADDING * 2;
        if width > self.size || height > self.size {
            return None;
        }
        let allocated = self.allocate(width, height).or_else(|| {
            self.grow();
            self.allocate(width, height)
        });
        let (x, y) = allocated?;
        let mut data = vec![0u8; (width * height) as usize];
        for row in 0..raster.height {
            let source = (row * raster.width) as usize;
            let destination = ((row + PADDING) * width + PADDING) as usize;
            data[destination..destination + raster.width as usize]
                .copy_from_slice(&raster.data[source..source + raster.width as usize]);
        }
        self.uploads.push(Upload {
            x,
            y,
            width,
            height,
            data,
        });
        Some(GlyphSlot {
            uv: [
                (x + PADDING) as f32 / self.size as f32,
                (y + PADDING) as f32 / self.size as f32,
                (x + PADDING + raster.width) as f32 / self.size as f32,
                (y + PADDING + raster.height) as f32 / self.size as f32,
            ],
            left: raster.left,
            top: raster.top,
            width: raster.width,
            height: raster.height,
        })
    }

    fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        for shelf in self.shelves.iter_mut() {
            if shelf.height >= height && shelf.cursor + width + PADDING <= self.size {
                let x = shelf.cursor;
                shelf.cursor += width + PADDING;
                return Some((x, shelf.top));
            }
        }
        let top = self
            .shelves
            .last()
            .map_or(0, |shelf| shelf.top + shelf.height);
        if top + height > self.size {
            return None;
        }
        self.shelves.push(Shelf {
            top,
            height,
            cursor: width + PADDING,
        });
        Some((0, top))
    }

    fn grow(&mut self) {
        let device = self.device.clone();
        self.reset(&device, (self.size * 2).min(MAXIMUM_SIZE));
    }

    pub fn reset(&mut self, device: &Device, size: u32) {
        self.resets += 1;
        self.size = size;
        self.shelves.clear();
        self.slots.clear();
        self.uploads.clear();
        let (texture, view, sampler) = create(device, size);
        self.texture = texture;
        self.view = view;
        self.sampler = sampler;
    }

    pub fn flush(&mut self, queue: &Queue) {
        for upload in self.uploads.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
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
                    bytes_per_row: Some(upload.width),
                    rows_per_image: Some(upload.height),
                },
                wgpu::Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

fn create(device: &Device, size: u32) -> (Texture, TextureView, Sampler) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.ui.atlas"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("grana.ui.atlas.sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..Default::default()
    });
    (texture, view, sampler)
}
