use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use wgpu::{
    Buffer, BufferAsyncError, BufferDescriptor, BufferUsages, CommandEncoder, Device, MapMode,
    PollType, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
};

const COPY_ALIGNMENT: u64 = 4;

struct Slot {
    buffer: Buffer,
    mapping: Option<Receiver<Result<(), BufferAsyncError>>>,
    ready: bool,
    reserved: bool,
}

pub struct Readback {
    slots: Vec<Slot>,
    pending: VecDeque<usize>,
}

impl Readback {
    pub fn bytes_of(size: u64, slots: usize) -> u64 {
        assert!(
            size > 0 && size.is_multiple_of(COPY_ALIGNMENT),
            "a readback slot is a whole number of aligned bytes"
        );
        assert!(slots > 0, "a readback ring holds at least one slot");
        size * slots as u64
    }

    pub fn new(device: &Device, label: &'static str, size: u64, slots: usize) -> Self {
        assert!(
            size > 0 && size.is_multiple_of(COPY_ALIGNMENT),
            "a readback slot is a whole number of aligned bytes"
        );
        assert!(slots > 0, "a readback ring holds at least one slot");
        let slots = (0..slots)
            .map(|_| Slot {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                mapping: None,
                ready: false,
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

    pub fn bytes(&self) -> u64 {
        self.slots
            .iter()
            .map(|slot| slot.buffer.size())
            .sum::<u64>()
    }

    pub fn reserve(&mut self) -> Option<usize> {
        let slot = self.slots.iter().position(|slot| !slot.reserved)?;
        self.slots[slot].reserved = true;
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
        texture: &Texture,
        row: u64,
    ) {
        self.stage_level(encoder, slot, texture, 0, row);
    }

    pub fn stage_level(
        &self,
        encoder: &mut CommandEncoder,
        slot: usize,
        texture: &Texture,
        level: u32,
        row: u64,
    ) {
        assert!(
            row.is_multiple_of(COPY_ALIGNMENT) && row <= self.slots[slot].buffer.size(),
            "a pixel readback row fits its staging buffer"
        );
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &self.slots[slot].buffer,
                layout: TexelCopyBufferLayout {
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

    pub fn stage_image(
        &self,
        encoder: &mut CommandEncoder,
        slot: usize,
        texture: &Texture,
        size: [u32; 2],
        stride: u64,
    ) {
        let reach = stride * u64::from(size[1]);
        assert!(
            stride.is_multiple_of(COPY_ALIGNMENT) && reach <= self.slots[slot].buffer.size(),
            "a staged image fits its readback slot"
        );
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &self.slots[slot].buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::try_from(stride).expect("a staged row fits u32")),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn register(&mut self) {
        for (slot, held) in self.slots.iter_mut().enumerate() {
            if held.reserved && held.mapping.is_none() {
                let (sender, receiver) = mpsc::channel();
                held.buffer
                    .slice(..)
                    .map_async(MapMode::Read, move |result| {
                        let _ = sender.send(result);
                    });
                held.mapping = Some(receiver);
                self.pending.push_back(slot);
            }
        }
    }

    pub fn take_with<R>(
        &mut self,
        device: &Device,
        read: impl FnOnce(usize, &[u8]) -> R,
    ) -> Result<Option<R>, String> {
        Ok(self.ready(device, false)?.then(|| self.take_ready(read)))
    }

    pub fn ready(&mut self, device: &Device, wait: bool) -> Result<bool, String> {
        let Some(&slot) = self.pending.front() else {
            return Ok(false);
        };
        if self.slots[slot].ready {
            return Ok(true);
        }
        if let Err(error) = device.poll(if wait {
            PollType::wait_indefinitely()
        } else {
            PollType::Poll
        }) {
            return Err(format!(
                "the graphics device failed while reading back a frame: {error}"
            ));
        }
        let receiver = self.slots[slot]
            .mapping
            .as_ref()
            .expect("a pending readback slot is registered before it is taken");
        match receiver.try_recv() {
            Ok(Ok(())) => {
                if self.slots[slot]
                    .buffer
                    .slice(..)
                    .get_mapped_range()
                    .is_err()
                {
                    return Err(
                        "the graphics device dropped the mapped readback staging buffer"
                            .to_string(),
                    );
                }
                self.slots[slot].ready = true;
                Ok(true)
            }
            Ok(Err(error)) => Err(format!(
                "the graphics device dropped the readback staging buffer: {error}"
            )),
            Err(TryRecvError::Empty) => Ok(false),
            Err(TryRecvError::Disconnected) => {
                Err("the readback callback reported no result".to_string())
            }
        }
    }

    pub fn take_ready<R>(&mut self, read: impl FnOnce(usize, &[u8]) -> R) -> R {
        let slot = *self
            .pending
            .front()
            .expect("a ready readback slot is pending");
        assert!(
            self.slots[slot].ready,
            "a readback slot is taken once its mapping completed"
        );
        let slice = self.slots[slot].buffer.slice(..);
        let value = {
            let mapped = slice
                .get_mapped_range()
                .expect("the mapped readback staging buffer stays mapped");
            read(slot, &mapped)
        };
        self.slots[slot].buffer.unmap();
        self.slots[slot].mapping = None;
        self.slots[slot].ready = false;
        self.slots[slot].reserved = false;
        self.pending.pop_front();
        value
    }
}
