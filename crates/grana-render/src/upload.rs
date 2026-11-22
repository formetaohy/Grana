use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, Device, Extent3d, Queue,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
};

const ALIGNMENT: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

const CAPACITY_FLOOR: u64 = 1 << 20;

enum Copy {
    Texture {
        texture: Texture,
        origin: [u32; 3],
        extent: [u32; 3],
        rows: u32,
        row_bytes: u32,
        offset: u64,
    },
    Buffer {
        target: Buffer,
        target_offset: u64,
        offset: u64,
        size: u64,
    },
}

pub(crate) struct Upload {
    buffer: Option<Buffer>,
    capacity: u64,
    bytes: Vec<u8>,
    filled: usize,
    copies: Vec<Copy>,
    commanded: u64,
    staged: u64,
    payload: u64,
}

impl Upload {
    pub fn new() -> Self {
        Self {
            buffer: None,
            capacity: 0,
            bytes: Vec::new(),
            filled: 0,
            copies: Vec::new(),
            commanded: 0,
            staged: 0,
            payload: 0,
        }
    }

    pub fn counters(&self) -> (u64, u64, u64) {
        (self.commanded, self.staged, self.payload)
    }

    pub fn texture(
        &mut self,
        texture: &Texture,
        origin: [u32; 3],
        extent: [u32; 3],
        row_bytes: u32,
        rows: u32,
        bytes: &[u8],
    ) {
        assert!(
            row_bytes.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            "an uploaded image row keeps the copy row alignment"
        );
        assert_eq!(
            bytes.len() as u64,
            u64::from(row_bytes) * u64::from(rows) * u64::from(extent[2].max(1)),
            "an uploaded image covers its rows"
        );
        let offset = self.reserve(bytes, ALIGNMENT) as u64;
        self.payload += bytes.len() as u64;
        self.commanded += 1;
        self.copies.push(Copy::Texture {
            texture: texture.clone(),
            origin,
            extent,
            rows,
            row_bytes,
            offset,
        });
    }

    pub fn bricks(
        &mut self,
        texture: &Texture,
        origin: [u32; 3],
        side: u32,
        texel: u32,
        columns: &[&[u8]],
    ) {
        assert!(
            !columns.is_empty(),
            "an uploaded brick batch holds at least one brick"
        );
        let stride = (side * texel) as usize;
        let extent = usize::try_from(side).expect("a brick side fits this platform's addresses");
        for column in columns {
            assert_eq!(
                column.len(),
                stride * extent * extent,
                "an uploaded brick covers every texel of its rows"
            );
        }
        let width = stride * columns.len();
        let row_bytes = width.next_multiple_of(ALIGNMENT);
        let length = row_bytes * extent * extent;
        let offset = self.reserve_blank(length, ALIGNMENT);
        self.payload += (width * extent * extent) as u64;
        self.commanded += 1;
        for line in 0..extent * extent {
            let row = &mut self.bytes[offset + line * row_bytes..offset + line * row_bytes + width];
            for (column, source) in columns.iter().enumerate() {
                row[column * stride..(column + 1) * stride]
                    .copy_from_slice(&source[line * stride..(line + 1) * stride]);
            }
        }
        self.copies.push(Copy::Texture {
            texture: texture.clone(),
            origin,
            extent: [
                side * u32::try_from(columns.len()).expect("a brick batch fits u32 columns"),
                side,
                side,
            ],
            rows: side,
            row_bytes: u32::try_from(row_bytes).expect("a brick row fits u32"),
            offset: offset as u64,
        });
    }

    pub fn buffer(&mut self, target: &Buffer, target_offset: u64, bytes: &[u8]) {
        assert!(
            target_offset.is_multiple_of(4) && bytes.len().is_multiple_of(4),
            "an uploaded buffer run keeps the copy alignment"
        );
        if bytes.is_empty() {
            return;
        }
        let offset = self.reserve(bytes, 4) as u64;
        self.payload += bytes.len() as u64;
        self.commanded += 1;
        self.copies.push(Copy::Buffer {
            target: target.clone(),
            target_offset,
            offset,
            size: bytes.len() as u64,
        });
    }

    pub fn encode(&mut self, device: &Device, queue: &Queue, encoder: &mut CommandEncoder) {
        if self.copies.is_empty() {
            self.filled = 0;
            return;
        }
        let size = self.filled.next_multiple_of(4);
        self.staged += size as u64;
        let capacity = (size as u64).max(CAPACITY_FLOOR).next_power_of_two();
        if self.capacity < capacity {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("grana.upload.staging"),
                size: capacity,
                usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = capacity;
        }
        if self.bytes.len() < size {
            self.bytes.resize(size, 0);
        }
        let buffer = self.buffer.as_ref().expect("the upload staging is sized");
        queue.write_buffer(buffer, 0, &self.bytes[..size]);
        for copy in self.copies.drain(..) {
            match copy {
                Copy::Texture {
                    texture,
                    origin,
                    extent,
                    rows,
                    row_bytes,
                    offset,
                } => encoder.copy_buffer_to_texture(
                    TexelCopyBufferInfo {
                        buffer,
                        layout: TexelCopyBufferLayout {
                            offset,
                            bytes_per_row: Some(row_bytes),
                            rows_per_image: Some(rows),
                        },
                    },
                    TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: origin[0],
                            y: origin[1],
                            z: origin[2],
                        },
                        aspect: TextureAspect::All,
                    },
                    Extent3d {
                        width: extent[0],
                        height: extent[1],
                        depth_or_array_layers: extent[2],
                    },
                ),
                Copy::Buffer {
                    target,
                    target_offset,
                    offset,
                    size,
                } => encoder.copy_buffer_to_buffer(buffer, offset, &target, target_offset, size),
            }
        }
        self.filled = 0;
    }

    pub fn discard(&mut self) {
        self.filled = 0;
        self.copies.clear();
    }

    fn reserve(&mut self, bytes: &[u8], alignment: usize) -> usize {
        let offset = self.reserve_blank(bytes.len(), alignment);
        self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        offset
    }

    fn reserve_blank(&mut self, len: usize, alignment: usize) -> usize {
        let offset = self.filled.next_multiple_of(alignment);
        let end = offset + len;
        if self.bytes.len() < end {
            self.bytes.resize(end, 0);
        }
        self.filled = end;
        offset
    }
}
