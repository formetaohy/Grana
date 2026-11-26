use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, CommandEncoderDescriptor, Device,
    Extent3d, Queue, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect,
};

const ALIGNMENT: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

pub(crate) const STAGING_BYTES: u64 = 4 << 20;

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

impl Copy {
    fn offset(&self) -> u64 {
        match self {
            Self::Texture { offset, .. } | Self::Buffer { offset, .. } => *offset,
        }
    }
}

pub(crate) struct Upload {
    buffer: Buffer,
    bytes: Vec<u8>,
    filled: usize,
    copies: Vec<Copy>,
    commanded: u64,
    staged: u64,
    payload: u64,
}

impl Upload {
    pub fn new(device: &Device) -> Self {
        Self {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("grana.upload.staging"),
                size: STAGING_BYTES,
                usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bytes: Vec::new(),
            filled: 0,
            copies: Vec::new(),
            commanded: 0,
            staged: 0,
            payload: 0,
        }
    }

    pub fn bytes() -> u64 {
        STAGING_BYTES
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
        let rows = extent * extent;
        let within = |count: usize| (stride * count).next_multiple_of(ALIGNMENT) * rows;
        let mut width = columns.len();
        while width > 1 && within(width) > STAGING_BYTES as usize {
            width -= 1;
        }
        let mut first = 0;
        while first < columns.len() {
            let last = (first + width).min(columns.len());
            let held = &columns[first..last];
            let payload = stride * held.len();
            let row_bytes = payload.next_multiple_of(ALIGNMENT);
            let length = row_bytes * rows;
            let offset = self.reserve_blank(length, ALIGNMENT);
            self.payload += (payload * rows) as u64;
            self.commanded += 1;
            for line in 0..rows {
                let row =
                    &mut self.bytes[offset + line * row_bytes..offset + line * row_bytes + payload];
                for (column, source) in held.iter().enumerate() {
                    row[column * stride..(column + 1) * stride]
                        .copy_from_slice(&source[line * stride..(line + 1) * stride]);
                }
            }
            self.copies.push(Copy::Texture {
                texture: texture.clone(),
                origin: [
                    origin[0]
                        + u32::try_from(first).expect("a brick batch fits u32 columns") * side,
                    origin[1],
                    origin[2],
                ],
                extent: [
                    side * u32::try_from(held.len()).expect("a brick run fits u32 columns"),
                    side,
                    side,
                ],
                rows: side,
                row_bytes: u32::try_from(row_bytes).expect("a brick row fits u32"),
                offset: offset as u64,
            });
            first = last;
        }
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

    pub fn flush(&mut self, device: &Device, queue: &Queue) {
        let chunk = STAGING_BYTES as usize;
        let mut first = 0;
        while first < self.copies.len() {
            let start = self.copies[first].offset() as usize / chunk * chunk;
            let mut last = first + 1;
            while last < self.copies.len()
                && (self.copies[last].offset() as usize) / chunk * chunk == start
            {
                last += 1;
            }
            let reach = self.bytes.len().min(start + chunk) - start;
            queue.write_buffer(&self.buffer, 0, &self.bytes[start..start + reach]);
            self.staged += reach as u64;
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("grana.upload"),
            });
            for copy in &self.copies[first..last] {
                stage(&mut encoder, &self.buffer, copy, start as u64);
            }
            queue.submit([encoder.finish()]);
            first = last;
        }
        self.copies.clear();
        self.bytes.clear();
        self.filled = 0;
        if self.bytes.capacity() as u64 > 4 * STAGING_BYTES {
            self.bytes.shrink_to_fit();
        }
    }

    pub fn discard(&mut self) {
        self.copies.clear();
        self.bytes.clear();
        self.filled = 0;
    }

    fn reserve(&mut self, bytes: &[u8], alignment: usize) -> usize {
        let offset = self.reserve_blank(bytes.len(), alignment);
        self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        offset
    }

    fn reserve_blank(&mut self, len: usize, alignment: usize) -> usize {
        let chunk = STAGING_BYTES as usize;
        assert!(
            len <= chunk,
            "an uploaded run fits one staging chunk of {STAGING_BYTES} bytes"
        );
        let mut offset = self.filled.next_multiple_of(alignment);
        if offset % chunk + len > chunk {
            offset = offset.next_multiple_of(chunk);
        }
        let end = offset + len;
        if self.bytes.len() < end {
            self.bytes.resize(end, 0);
        }
        self.filled = end;
        offset
    }
}

fn stage(encoder: &mut CommandEncoder, buffer: &Buffer, copy: &Copy, start: u64) {
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
                    offset: offset - start,
                    bytes_per_row: Some(*row_bytes),
                    rows_per_image: Some(*rows),
                },
            },
            TexelCopyTextureInfo {
                texture,
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
        } => encoder.copy_buffer_to_buffer(buffer, offset - start, target, *target_offset, *size),
    }
}
