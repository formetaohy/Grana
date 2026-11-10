use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use wgpu::{
    Buffer, BufferAsyncError, BufferDescriptor, BufferUsages, CommandEncoder, Device, MapMode,
    PollType,
};

const COPY_ALIGNMENT: u64 = 4;

struct Slot {
    buffer: Buffer,
    mapping: Option<Receiver<Result<(), BufferAsyncError>>>,
    reserved: bool,
}

pub struct Readback {
    slots: Vec<Slot>,
    pending: VecDeque<usize>,
}

impl Readback {
    pub fn new(device: &Device, label: &'static str, row: u64, slots: usize) -> Self {
        assert!(
            row > 0 && row.is_multiple_of(COPY_ALIGNMENT),
            "a readback row is a whole number of aligned bytes"
        );
        assert!(slots > 0, "a readback ring holds at least one slot");
        let slots = (0..slots)
            .map(|_| Slot {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some(label),
                    size: row,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                mapping: None,
                reserved: false,
            })
            .collect();
        Self {
            slots,
            pending: VecDeque::new(),
        }
    }

    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn reserve(&mut self) -> Option<usize> {
        let slot = self.slots.iter().position(|slot| !slot.reserved)?;
        self.slots[slot].reserved = true;
        self.pending.push_back(slot);
        Some(slot)
    }

    pub fn stage(
        &self,
        encoder: &mut CommandEncoder,
        slot: usize,
        offset: u64,
        source: &Buffer,
        size: u64,
    ) {
        encoder.copy_buffer_to_buffer(source, 0, &self.slots[slot].buffer, offset, size);
    }

    pub fn stage_pixel(
        &self,
        encoder: &mut CommandEncoder,
        slot: usize,
        texture: &wgpu::Texture,
        row: u64,
    ) {
        self.stage_level(encoder, slot, texture, 0, row);
    }

    pub fn stage_level(
        &self,
        encoder: &mut CommandEncoder,
        slot: usize,
        texture: &wgpu::Texture,
        level: u32,
        row: u64,
    ) {
        assert!(
            row.is_multiple_of(COPY_ALIGNMENT) && row <= self.slots[slot].buffer.size(),
            "a pixel readback row fits its staging buffer"
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.slots[slot].buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::try_from(row).expect("a readback row fits u32")),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn register(&mut self) {
        for slot in &mut self.slots {
            if slot.reserved && slot.mapping.is_none() {
                let (sender, receiver) = mpsc::channel();
                slot.buffer
                    .slice(..)
                    .map_async(MapMode::Read, move |result| {
                        let _ = sender.send(result);
                    });
                slot.mapping = Some(receiver);
            }
        }
    }

    pub fn take(&mut self, device: &Device) -> Option<(usize, Vec<u8>)> {
        let slot = *self.pending.front()?;
        device
            .poll(PollType::Poll)
            .expect("the graphics device failed while reading back a frame");
        let receiver = self.slots[slot]
            .mapping
            .as_ref()
            .expect("a pending readback slot is registered before it is taken");
        match receiver.try_recv() {
            Ok(result) => result.expect("the readback staging buffer is mappable"),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => panic!("the readback callback fires once"),
        }
        let slice = self.slots[slot].buffer.slice(..);
        let mapped = slice
            .get_mapped_range()
            .expect("the mapped readback staging buffer stays mapped");
        let bytes = mapped.to_vec();
        drop(mapped);
        self.slots[slot].buffer.unmap();
        self.slots[slot].mapping = None;
        self.slots[slot].reserved = false;
        self.pending.pop_front();
        Some((slot, bytes))
    }
}
