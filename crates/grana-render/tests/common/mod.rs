use grana_display::{Camera, SectionCamera, TransferFunction};
use grana_render::{Frame, Renderer, Requests, SceneFrame};
use grana_volume::BrickRequest;
use grana_volume::{BrickData, Pyramid, Region};

pub const SIDE: u32 = 256;
pub const SAMPLES: u32 = 4;

#[derive(Clone, Copy)]
pub struct Session<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub target: &'a wgpu::TextureView,
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub scene: &'a SceneFrame<'a>,
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub region: Region,
    pub time: u32,
}

pub fn settle(
    renderer: &mut Renderer,
    session: Session<'_>,
    source: impl Fn(BrickRequest) -> BrickData,
) -> Requests {
    let mut latest = Requests::default();
    for _ in 0..96 {
        renderer.draw(session.device, session.queue, frame(&session));
        if !renderer.requests_pending() {
            return latest;
        }
        if let Some(requests) = next_demand(renderer, &session) {
            for request in requests.volume.iter().chain(requests.section.iter()) {
                let brick = source(*request);
                let _ = renderer.upload(session.device, session.queue, *request, &brick);
            }
            latest = requests;
        }
    }
    panic!("the stream never settled");
}

pub fn next_demand(renderer: &mut Renderer, session: &Session<'_>) -> Option<Requests> {
    for _ in 0..4 {
        match renderer.poll_requests(session.device) {
            Some(requests) => return Some(requests),
            None => {
                session
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("failed to wait for the gpu");
            }
        }
    }
    None
}

pub fn frame<'a>(session: &Session<'a>) -> Frame<'a, 'a> {
    Frame {
        target: session.target,
        size: [SIDE, SIDE],
        volume_region: session.volume_region,
        section_region: session.section_region,
        time: session.time,
        camera: session.camera,
        section: session.section,
        region: session.region,
        exposure: 1.0,
        samples: SAMPLES,
        scene: session.scene,
        prefetch: false,
    }
}

pub fn prefetch_frame<'a>(session: &Session<'a>) -> Frame<'a, 'a> {
    Frame {
        prefetch: true,
        ..frame(session)
    }
}

pub fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    match grana_gpu::Headless::new() {
        Ok(headless) => Some((headless.device, headless.queue)),
        Err(detail) => {
            println!("{detail}, skipping");
            None
        }
    }
}

pub fn target(device: &wgpu::Device, side: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grana.test.target"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub fn channels<'a>(
    pyramid: &Pyramid,
    transfers: &'a [TransferFunction],
) -> Vec<grana_render::ChannelRow<'a>> {
    assert_eq!(transfers.len(), pyramid.channel_count());
    pyramid
        .channels()
        .iter()
        .zip(transfers)
        .map(|(channel, transfer)| grana_render::ChannelRow {
            visible: true,
            opacity: 1.0,
            window: channel.window(),
            transfer,
        })
        .collect()
}

pub fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
    side: u32,
) -> Vec<[u8; 4]> {
    let row_bytes = side * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grana.test.readback"),
        size: (row_bytes * side) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(side),
            },
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("failed to wait for the gpu");
    receiver.recv().expect("map callback").expect("map failed");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let pixels = mapped.as_chunks::<4>().0.to_vec();
    drop(mapped);
    readback.unmap();
    pixels
}

pub fn lit(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32 > 120)
        .count()
}

pub fn blue(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[2]) > u32::from(pixel[0]) + 20
                && u32::from(pixel[2]) > u32::from(pixel[1]) + 20
                && pixel[2] > 40
        })
        .count()
}

pub fn green(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[1]) > u32::from(pixel[0]) + 20
                && u32::from(pixel[1]) > u32::from(pixel[2]) + 20
                && pixel[1] > 40
        })
        .count()
}

pub fn pixel(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIDE + x) as usize]
}

pub fn red_count(pixels: &[[u8; 4]], region: [u32; 4]) -> usize {
    let mut count = 0;
    for y in region[1]..region[1] + region[3] {
        for x in region[0]..region[0] + region[2] {
            let [red, green, blue, _] = pixel(pixels, x, y).map(u32::from);
            if red > green + 40 && red > blue + 40 {
                count += 1;
            }
        }
    }
    count
}

pub fn red_pixels(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[0]) > u32::from(pixel[1]) + 40
                && u32::from(pixel[0]) > u32::from(pixel[2]) + 40
        })
        .count()
}

pub fn green_pixels(pixels: &[[u8; 4]]) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            u32::from(pixel[1]) > u32::from(pixel[0]) + 40
                && u32::from(pixel[1]) > u32::from(pixel[2]) + 40
        })
        .count()
}

pub fn green_sum(pixels: &[[u8; 4]]) -> u64 {
    pixels.iter().map(|pixel| u64::from(pixel[1])).sum()
}

pub fn bright_fraction(pixels: &[[u8; 4]], x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    fraction(pixels, x0, x1, y0, y1, |pixel| {
        u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) > 660
    })
}

pub fn dark_fraction(pixels: &[[u8; 4]], x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    fraction(pixels, x0, x1, y0, y1, |pixel| {
        u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) < 180
    })
}

fn fraction(
    pixels: &[[u8; 4]],
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    accept: impl Fn([u8; 4]) -> bool,
) -> f32 {
    let left = (x0 * SIDE as f32) as u32;
    let right = (x1 * SIDE as f32) as u32;
    let top = (y0 * SIDE as f32) as u32;
    let bottom = (y1 * SIDE as f32) as u32;
    let mut accepted = 0;
    let mut total = 0;
    for y in top..bottom {
        for x in left..right {
            total += 1;
            if accept(pixel(pixels, x, y)) {
                accepted += 1;
            }
        }
    }
    accepted as f32 / total as f32
}
