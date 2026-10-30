use bytemuck::cast_slice;
use grana_volume::PADDED_SIDE;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Device,
    Queue, Sampler, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension,
};

const BYTES_PER_VOXEL: u64 = size_of::<u16>() as u64;

#[derive(Clone, Copy)]
struct Slot {
    brick: Option<(usize, usize)>,
    used: u64,
}

impl Slot {
    const EMPTY: Self = Self {
        brick: None,
        used: 0,
    };
}

pub struct Atlas {
    textures: Vec<Texture>,
    group: BindGroup,
    slot_grid: [u32; 3],
    free: Vec<u32>,
    slots: Vec<Slot>,
    bytes: u64,
}

impl Atlas {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        dummy: &TextureView,
        sampler: &Sampler,
        channel_count: usize,
        requested: u64,
        max_dimension: u32,
    ) -> Self {
        let slot_side = PADDED_SIDE;
        assert!(
            (1..=crate::MAX_CHANNEL).contains(&channel_count),
            "an atlas binds one to {} channels",
            crate::MAX_CHANNEL
        );
        assert!(
            max_dimension >= slot_side,
            "the adapter cannot hold a padded brick"
        );
        let slots_per_axis = ((requested as f64).cbrt().ceil() as u32)
            .max(1)
            .min(max_dimension / slot_side);
        let slot_grid = [slots_per_axis; 3];
        let capacity = (slots_per_axis as usize).pow(3);
        let dims = slot_grid.map(|count| count * slot_side);
        let textures = (0..channel_count)
            .map(|_| {
                device.create_texture(&TextureDescriptor {
                    label: Some("grana.atlas"),
                    size: wgpu::Extent3d {
                        width: dims[0],
                        height: dims[1],
                        depth_or_array_layers: dims[2],
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D3,
                    format: TextureFormat::R16Unorm,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .collect::<Vec<_>>();
        let views = textures
            .iter()
            .map(|texture| {
                texture.create_view(&TextureViewDescriptor {
                    label: Some("grana.atlas.view"),
                    dimension: Some(TextureViewDimension::D3),
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        let mut bindings = views.iter().collect::<Vec<&TextureView>>();
        bindings.resize(crate::MAX_CHANNEL, dummy);
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.atlas.group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureViewArray(&bindings),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        });
        let bytes =
            capacity as u64 * (slot_side as u64).pow(3) * BYTES_PER_VOXEL * channel_count as u64;
        Self {
            textures,
            group,
            slot_grid,
            free: (0..capacity as u32).rev().collect(),
            slots: vec![Slot::EMPTY; capacity],
            bytes,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }

    pub fn slot_grid(&self) -> [u32; 3] {
        self.slot_grid
    }

    pub fn slots(&self) -> usize {
        self.slots.len()
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn mark(&mut self, slot: u32, frame: u64) {
        self.slots[slot as usize].used = frame;
    }

    pub fn allocate(&mut self, frame: u64) -> Option<(u32, Option<(usize, usize)>)> {
        if let Some(slot) = self.free.pop() {
            return Some((slot, None));
        }
        let victim = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.brick.is_some() && slot.used < frame)
            .min_by_key(|(_, slot)| slot.used)
            .map(|(index, _)| index)?;
        let brick = self.slots[victim]
            .brick
            .expect("the eviction candidate holds a brick");
        self.slots[victim].brick = None;
        Some((victim as u32, Some(brick)))
    }

    pub fn assign(&mut self, slot: u32, level: usize, index: usize, frame: u64) {
        self.slots[slot as usize] = Slot {
            brick: Some((level, index)),
            used: frame,
        };
    }

    pub fn write(&self, queue: &Queue, channel: usize, slot: u32, units: &[u16]) {
        assert_eq!(
            units.len() as u64,
            (PADDED_SIDE as u64).pow(3),
            "a padded brick fills one slot"
        );
        let origin = slot_origin(slot, self.slot_grid);
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.textures[channel],
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                aspect: TextureAspect::All,
            },
            cast_slice(units),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PADDED_SIDE * size_of::<u16>() as u32),
                rows_per_image: Some(PADDED_SIDE),
            },
            wgpu::Extent3d {
                width: PADDED_SIDE,
                height: PADDED_SIDE,
                depth_or_array_layers: PADDED_SIDE,
            },
        );
    }
}

fn slot_origin(slot: u32, slot_grid: [u32; 3]) -> [u32; 3] {
    let layer = slot / (slot_grid[0] * slot_grid[1]);
    let remainder = slot % (slot_grid[0] * slot_grid[1]);
    let row = remainder / slot_grid[0];
    let column = remainder % slot_grid[0];
    [column * PADDED_SIDE, row * PADDED_SIDE, layer * PADDED_SIDE]
}
