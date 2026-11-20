use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    pipelines: grana_gpu::Pipelines,
    _serial: MutexGuard<'static, ()>,
}

impl Gpu {
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn pipelines(&self) -> &grana_gpu::Pipelines {
        &self.pipelines
    }
}

impl Deref for Gpu {
    type Target = wgpu::Device;

    fn deref(&self) -> &wgpu::Device {
        &self.device
    }
}

pub fn splat(center: [f32; 3], radius: f32, color: [f32; 3], alpha: f32) -> grana_render::Splat {
    grana_render::Splat::gaussian(
        center,
        grana_object::Spot::new(center, radius, 0.0).sigma(),
        grana_display::srgb_to_linear(color),
        alpha,
    )
}

pub fn gpu() -> Option<Gpu> {
    static SERIAL: Mutex<()> = Mutex::new(());
    let serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let headless = match grana_gpu::Headless::shared() {
        Ok(headless) => headless,
        Err(detail) => {
            println!("{detail}, skipping");
            return None;
        }
    };
    let pipelines = headless.pipelines().clone();
    Some(Gpu {
        device: headless.device,
        queue: headless.queue,
        pipelines,
        _serial: serial,
    })
}

pub fn revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub struct MeasureSource {
    kind: grana_measure::Kind,
    channels: Vec<String>,
    objects: Vec<(u32, grana_object::SpotSet)>,
}

pub fn measure_source(
    kind: grana_measure::Kind,
    channels: &[&str],
    objects: impl IntoIterator<Item = grana_object::SpotSet>,
) -> std::sync::Arc<dyn grana_measure::FrameSource> {
    std::sync::Arc::new(MeasureSource {
        kind,
        channels: channels
            .iter()
            .map(|channel| (*channel).to_string())
            .collect(),
        objects: objects
            .into_iter()
            .enumerate()
            .map(|(time, objects)| (time as u32, objects))
            .collect(),
    })
}

impl grana_measure::FrameSource for MeasureSource {
    fn frames(&self) -> u32 {
        self.objects.len() as u32
    }

    fn kind(&self) -> grana_measure::Kind {
        self.kind
    }

    fn channels(&self) -> &[String] {
        &self.channels
    }

    fn relations(&self) -> Vec<grana_measure::RelationRow<'_>> {
        Vec::new()
    }

    fn children(&self) -> Vec<grana_measure::Child<'_>> {
        Vec::new()
    }

    fn visit(&self, time: u32, visit: &mut dyn FnMut(grana_measure::Frame<'_>)) {
        let (time, objects) = &self.objects[time as usize];
        visit(grana_measure::Frame::new(
            *time,
            grana_measure::Objects::Spots(objects),
            &[],
            &[],
        ));
    }
}

pub struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    downloads: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn serve(root: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the fixture server binds");
        listener
            .set_nonblocking(true)
            .expect("the fixture server polls");
        let address = listener
            .local_addr()
            .expect("the fixture server names itself");
        let root = root.to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let downloads = Arc::new(AtomicUsize::new(0));
        let thread = {
            let stop = Arc::clone(&stop);
            let downloads = Arc::clone(&downloads);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => respond(stream, &root, &downloads),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            url: format!("http://{address}/"),
            stop,
            downloads,
            thread: Some(thread),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn downloads(&self) -> usize {
        self.downloads.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.downloads.store(0, Ordering::Relaxed);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn respond(mut stream: TcpStream, root: &Path, downloads: &AtomicUsize) {
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).unwrap_or(0);
        if read == 0 {
            return;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 64 << 10 {
            return;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let mut parts = text.lines().next().unwrap_or_default().split(' ');
    let method = parts.next().unwrap_or_default();
    let target = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    let relative = target.trim_start_matches('/');
    if relative.split('/').any(|part| part == "..") {
        return status(&mut stream, 400);
    }
    let body = match std::fs::read(root.join(relative)) {
        Ok(body) => body,
        Err(_) => return status(&mut stream, 404),
    };
    if method == "GET" && !relative.ends_with("zarr.json") {
        downloads.fetch_add(1, Ordering::Relaxed);
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"{:016x}\"\r\nConnection: close\r\n\r\n",
        body.len(),
        digest(&body)
    );
    let _ = stream.write_all(head.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(&body);
    }
    let _ = stream.flush();
}

fn status(stream: &mut TcpStream, code: u16) {
    let head = format!("HTTP/1.1 {code} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

fn digest(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn value(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }

    pub fn gaussian(&mut self) -> f32 {
        let first = self.value().max(f32::MIN_POSITIVE);
        (-2.0 * first.ln()).sqrt() * (2.0 * std::f64::consts::PI as f32 * self.value()).cos()
    }
}

pub struct Localization {
    pub frame: u32,
    pub position: [f32; 3],
    pub photons: f32,
    pub precision: [f32; 3],
    pub group: i64,
}

pub fn picasso(path: &Path, pixelsize: f32, rows: &[Localization]) {
    const RECORD: u32 = 40;
    let datatype = hdf5_pure::CompoundTypeBuilder::with_size(RECORD)
        .u32_field("frame", 0)
        .f32_field("x", 4)
        .f32_field("y", 8)
        .f32_field("z", 12)
        .f32_field("photons", 16)
        .f32_field("lpx", 20)
        .f32_field("lpy", 24)
        .f32_field("lpz", 28)
        .i64_field("group", 32)
        .build()
        .expect("the Picasso table type builds");
    let mut raw = Vec::with_capacity(rows.len() * RECORD as usize);
    for row in rows {
        raw.extend_from_slice(&row.frame.to_le_bytes());
        for value in row.position {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&row.photons.to_le_bytes());
        for value in row.precision {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&row.group.to_le_bytes());
    }
    let metadata = format!(
        r#"[{{"Generated by": "Picasso"}}, {{"Pixelsize": {pixelsize}, "Width": 64, "Height": 32}}]"#
    );
    let mut builder = hdf5_pure::FileBuilder::new();
    builder
        .create_dataset("locs")
        .with_compound_data(datatype, raw, rows.len() as u64);
    builder
        .create_dataset("metadata")
        .with_vlen_strings(&[metadata.as_str()]);
    builder.write(path).expect("the Picasso fixture writes");
}

pub fn localization_reader(
    path: &Path,
    level: grana_volume::Level,
    frames: u32,
) -> grana_readout::localization::Reader {
    grana_readout::localization::open(path, level, frames, &mut |_, _| true)
        .expect("the localization table reads")
        .expect("the localization table finishes")
}

pub fn read_localizations(
    path: &Path,
    level: grana_volume::Level,
    frames: u32,
) -> Vec<grana_readout::localization::Localization> {
    let mut reader = localization_reader(path, level, frames);
    let mut rows = Vec::new();
    while let Some(frame) = reader
        .next(&mut |_, _| true)
        .expect("a localization frame reads")
    {
        rows.extend_from_slice(frame.rows());
    }
    rows
}

pub fn versions() -> grana_render::Versions {
    let stamp = revision();
    grana_render::Versions::new(stamp, stamp, stamp)
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
pub const TIFF_ENTRIES: u32 = 12;

pub fn ome_tiff(path: &Path, xml: &str, pages: &[(u32, u32, &[u8])]) {
    let record = 2 + TIFF_ENTRIES * 12 + 4;
    let header = 8u32;
    let xml_offset = header + pages.len() as u32 * record;
    let xml_bytes = xml.as_bytes();
    let mut strip = xml_offset + xml_bytes.len() as u32;
    let mut offsets = Vec::with_capacity(pages.len());
    for page in pages {
        offsets.push(strip);
        strip += page.2.len() as u32;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42u16.to_le_bytes());
    bytes.extend_from_slice(&header.to_le_bytes());
    for (index, (width, height, _)) in pages.iter().copied().enumerate() {
        let next = if index + 1 == pages.len() {
            0
        } else {
            header + (index as u32 + 1) * record
        };
        bytes.extend_from_slice(&(TIFF_ENTRIES as u16).to_le_bytes());
        let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        };
        entry(256, 4, 1, width);
        entry(257, 4, 1, height);
        entry(258, 3, 1, 8);
        entry(259, 3, 1, 1);
        entry(262, 3, 1, 1);
        entry(270, 2, xml_bytes.len() as u32, xml_offset);
        entry(273, 4, 1, offsets[index]);
        entry(277, 3, 1, 1);
        entry(278, 4, 1, height);
        entry(279, 4, 1, pages[index].2.len() as u32);
        entry(284, 3, 1, 1);
        entry(339, 3, 1, 1);
        bytes.extend_from_slice(&next.to_le_bytes());
    }
    bytes.extend_from_slice(xml_bytes);
    for page in pages {
        bytes.extend_from_slice(page.2);
    }
    std::fs::write(path, &bytes).expect("the OME-TIFF writes");
}

pub fn plate(root: &Path) {
    std::fs::create_dir_all(root).expect("the plate directory is created");
    group(
        root,
        "/",
        serde_json::json!({
            "ome": {
                "plate": {
                    "name": "assay",
                    "rows": [{"name": "A"}],
                    "columns": [{"name": "1"}, {"name": "2"}],
                    "wells": [
                        {"path": "A/1", "rowIndex": 0, "columnIndex": 0},
                        {"path": "A/2", "rowIndex": 0, "columnIndex": 1}
                    ]
                }
            }
        }),
    );
    group(root, "/A", serde_json::json!({}));
    group(
        root,
        "/A/1",
        serde_json::json!({"ome": {"well": {"images": [{"path": "0"}]}}}),
    );
    group(
        root,
        "/A/2",
        serde_json::json!({"ome": {"well": {"images": [{"path": "1"}]}}}),
    );
    field(&root.join("A/1/0"), Extent::new(8, 6, 4));
    field(&root.join("A/2/1"), Extent::new(12, 10, 6));
}

fn group(root: &Path, path: &str, attributes: serde_json::Value) {
    let store =
        Arc::new(zarrs::filesystem::FilesystemStore::new(root).expect("the plate store opens"));
    let mut builder = zarrs::group::GroupBuilder::new();
    builder.attributes(
        attributes
            .as_object()
            .expect("group attributes are an object")
            .clone(),
    );
    let group = builder.build(store, path).expect("the plate group builds");
    group
        .store_metadata()
        .expect("the plate group stores its metadata");
}

fn field(path: &Path, extent: Extent) {
    let channels = [grana_archive::Channel {
        name: "DAPI",
        tint: [0.0, 0.0, 1.0],
        intensity: grana_volume::Intensity::new(0.0, 255.0),
        window: grana_volume::Intensity::new(0.0, 255.0),
    }];
    let levels = [grana_volume::Level::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
    )];
    let fill = |block: grana_archive::Block| -> Result<grana_volume::VoxelData, String> {
        Ok(grana_volume::VoxelData::F32(vec![64.0; block.voxels()]))
    };
    let mut progress = |_, _| {};
    grana_zarr::write_image(
        path,
        &mut grana_archive::ImageSpec {
            name: "field",
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 1,
            interval: Some(1.0),
            sample: grana_volume::VoxelFormat::F32,
            fill: grana_archive::Fill::EveryLevel(&fill),
            cancel: &std::sync::atomic::AtomicBool::new(false),
            progress: &mut progress,
        },
    )
    .expect("the plate field writes");
}

pub struct Localizations {
    frames: Vec<Arc<[grana_object::Spot]>>,
}

impl Localizations {
    pub fn of(frames: impl IntoIterator<Item = Vec<grana_object::Spot>>) -> Self {
        Self {
            frames: frames.into_iter().map(Arc::from).collect(),
        }
    }
}

impl grana_analysis::localization::LocalizationSource for Localizations {
    fn len(&self) -> usize {
        self.frames.len()
    }

    fn frame(
        &self,
        index: usize,
        filter: grana_analysis::localization::LocalizationFilter,
    ) -> grana_analysis::localization::LocalizationFrame {
        grana_analysis::localization::LocalizationFrame::new(
            Arc::clone(&self.frames[index]),
            None,
            filter,
        )
    }
}
