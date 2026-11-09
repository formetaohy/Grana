use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Channel, Extent, Spacing, Volume, VoxelData};

pub struct Temp {
    path: PathBuf,
}

impl Temp {
    pub fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).expect("the temporary directory is created");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    _serial: MutexGuard<'static, ()>,
}

impl Gpu {
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

impl Deref for Gpu {
    type Target = wgpu::Device;

    fn deref(&self) -> &wgpu::Device {
        &self.device
    }
}

pub fn gpu() -> Option<Gpu> {
    static SHARED: OnceLock<Option<(wgpu::Device, wgpu::Queue)>> = OnceLock::new();
    static SERIAL: Mutex<()> = Mutex::new(());
    let serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (device, queue) = SHARED
        .get_or_init(|| match grana_gpu::Headless::new() {
            Ok(headless) => Some((headless.device, headless.queue)),
            Err(detail) => {
                println!("{detail}, skipping");
                None
            }
        })
        .clone()?;
    Some(Gpu {
        device,
        queue,
        _serial: serial,
    })
}

pub fn spheres(extent: Extent, spacing: Spacing, spheres: &[([f32; 3], f32)], peak: u16) -> Volume {
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let point = [x as f32, y as f32, z as f32];
                let inside = spheres.iter().any(|(centre, radius)| {
                    (0..3)
                        .map(|axis| (point[axis] - centre[axis]).powi(2))
                        .sum::<f32>()
                        <= radius * radius
                });
                if inside {
                    values[extent.index(x, y, z) as usize] = peak;
                }
            }
        }
    }
    Volume::new(
        extent,
        spacing,
        vec![Channel::new(
            "signal",
            [0.3, 0.6, 1.0],
            VoxelData::U16(values),
        )],
    )
}

pub fn read_pixels(gpu: &Gpu, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<[u8; 4]> {
    let row_bytes = (size[0] * 4) as usize;
    let stride = row_bytes.div_ceil(256) * 256;
    let readback = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.test.readback"),
        size: (stride * size[1] as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device().create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride as u32),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    gpu.queue().submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    gpu.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("failed to wait for the gpu");
    receiver.recv().expect("map callback").expect("map failed");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let mut pixels = Vec::with_capacity(size[0] as usize * size[1] as usize);
    for row in mapped.chunks_exact(stride).take(size[1] as usize) {
        let (chunk, _) = row[..row_bytes].as_chunks::<4>();
        pixels.extend_from_slice(chunk);
    }
    drop(mapped);
    readback.unmap();
    pixels
}

const CONTRAST: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hue {
    Blue,
    Green,
    Red,
}

impl Hue {
    fn channel(self) -> usize {
        match self {
            Self::Blue => 2,
            Self::Green => 1,
            Self::Red => 0,
        }
    }
}

pub struct Snapshot {
    size: [u32; 2],
    background: Vec<[u8; 4]>,
    image: Vec<[u8; 4]>,
}

impl Snapshot {
    pub fn new(size: [u32; 2], background: Vec<[u8; 4]>, image: Vec<[u8; 4]>) -> Self {
        assert!(
            background.len() == (size[0] * size[1]) as usize && image.len() == background.len(),
            "a snapshot covers every pixel of its target"
        );
        Self {
            size,
            background,
            image,
        }
    }

    pub fn all(&self) -> [u32; 4] {
        [0, 0, self.size[0], self.size[1]]
    }

    pub fn left(&self) -> [u32; 4] {
        [0, 0, self.size[0] / 2, self.size[1]]
    }

    pub fn right(&self) -> [u32; 4] {
        [self.size[0] / 2, 0, self.size[0], self.size[1]]
    }

    pub fn middle(&self) -> [u32; 4] {
        [
            self.size[0] / 4,
            self.size[1] / 4,
            3 * self.size[0] / 4,
            3 * self.size[1] / 4,
        ]
    }

    pub fn painted(&self, rect: [u32; 4]) -> usize {
        self.count(rect, |delta| delta.iter().any(|value| *value >= CONTRAST))
    }

    pub fn hue(&self, rect: [u32; 4], hue: Hue) -> usize {
        let channel = hue.channel();
        self.count(rect, |delta| {
            delta[channel] >= CONTRAST
                && (0..3).all(|other| other == channel || delta[other] < delta[channel])
        })
    }

    pub fn coverage(&self, rect: [u32; 4]) -> f32 {
        self.painted(rect) as f32 / self.area(rect) as f32
    }

    pub fn contrast(&self, rect: [u32; 4]) -> f32 {
        let [left, top, right, bottom] = rect;
        let area = self.area(rect);
        let mut sum = 0.0;
        for y in top..bottom {
            for x in left..right {
                let index = (y * self.size[0] + x) as usize;
                let background = self.background[index];
                let pixel = self.image[index];
                let delta = (0..3)
                    .map(|channel| i32::from(pixel[channel]) - i32::from(background[channel]))
                    .max()
                    .unwrap_or(0)
                    .max(0);
                sum += delta as f32;
            }
        }
        sum / area as f32
    }

    fn area(&self, rect: [u32; 4]) -> usize {
        let [left, top, right, bottom] = rect;
        assert!(
            left < right && top < bottom && right <= self.size[0] && bottom <= self.size[1],
            "a probe region lies inside the snapshot"
        );
        ((right - left) * (bottom - top)) as usize
    }

    fn count(&self, rect: [u32; 4], visit: impl Fn([i32; 3]) -> bool) -> usize {
        self.area(rect);
        let [left, top, right, bottom] = rect;
        let mut count = 0;
        for y in top..bottom {
            for x in left..right {
                let index = (y * self.size[0] + x) as usize;
                let background = self.background[index];
                let pixel = self.image[index];
                let delta = [0, 1, 2]
                    .map(|channel| i32::from(pixel[channel]) - i32::from(background[channel]));
                if visit(delta) {
                    count += 1;
                }
            }
        }
        count
    }
}
