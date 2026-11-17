use grana_volume::PADDED_SIDE;
use wgpu::{
    Buffer, BufferSize, CommandEncoder, Device, Extent3d, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect, util::StagingBelt,
};

use crate::atlas::Atlas;

const CHUNK_BYTES: u64 = 8 << 20;

const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

pub(crate) struct Staging {
    belt: StagingBelt,
    pending: Vec<Upload>,
}

struct Upload {
    pack: usize,
    slot: u32,
    buffer: Buffer,
    offset: u64,
    bytes_per_row: u32,
}

impl Staging {
    pub(crate) fn new(device: &Device) -> Self {
        Self {
            belt: StagingBelt::new(device.clone(), CHUNK_BYTES),
            pending: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, pack: usize, slot: u32, bytes_per_row: u32, bytes: &[u8]) {
        assert!(
            bytes_per_row.is_multiple_of(ROW_ALIGNMENT),
            "a staged brick row keeps the copy row alignment"
        );
        let size = (u64::from(PADDED_SIDE)).pow(2) * u64::from(bytes_per_row);
        assert_eq!(
            bytes.len() as u64,
            size,
            "a staged brick covers its packed rows"
        );
        let slice = self.belt.allocate(
            BufferSize::new(size).expect("a staged brick holds bytes"),
            BufferSize::new(u64::from(ROW_ALIGNMENT)).expect("the copy row alignment is positive"),
        );
        let offset = slice.offset();
        let buffer = slice.buffer().clone();
        slice
            .get_mapped_range_mut()
            .expect("a staging slice is mapped for writing")
            .slice(..)
            .copy_from_slice(bytes);
        self.pending.push(Upload {
            pack,
            slot,
            buffer,
            offset,
            bytes_per_row,
        });
    }

    pub(crate) fn encode(&mut self, encoder: &mut CommandEncoder, atlas: &Atlas) {
        for upload in self.pending.drain(..) {
            encoder.copy_buffer_to_texture(
                TexelCopyBufferInfo {
                    buffer: &upload.buffer,
                    layout: TexelCopyBufferLayout {
                        offset: upload.offset,
                        bytes_per_row: Some(upload.bytes_per_row),
                        rows_per_image: Some(PADDED_SIDE),
                    },
                },
                TexelCopyTextureInfo {
                    texture: atlas.texture(upload.pack),
                    mip_level: 0,
                    origin: atlas.origin(upload.slot),
                    aspect: TextureAspect::All,
                },
                Extent3d {
                    width: PADDED_SIDE,
                    height: PADDED_SIDE,
                    depth_or_array_layers: PADDED_SIDE,
                },
            );
        }
    }

    pub(crate) fn finish(&mut self, encoder: &CommandEncoder) {
        self.belt.finish_and_recall_on_submit(encoder);
    }

    pub(crate) fn discard(&mut self) {
        self.pending.clear();
        self.belt.finish();
        self.belt.recall();
    }
}
