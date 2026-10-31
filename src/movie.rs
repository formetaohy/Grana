use std::path::{Path, PathBuf};

use grana_render::Image;
use grana_video::Recorder;
use wgpu::{Device, Queue, Texture, TextureFormat, TextureUsages, TextureView};

pub struct Plan {
    pub path: PathBuf,
    pub size: [u32; 2],
    pub volume: [u32; 4],
    pub fps: u32,
    pub frames: u32,
    pub resume: u32,
    pub position: f32,
}

pub struct Movie {
    path: PathBuf,
    texture: Texture,
    view: TextureView,
    pub size: [u32; 2],
    pub volume: [u32; 4],
    pub frame: u32,
    pub frames: u32,
    pub attempts: u32,
    resume: u32,
    position: f32,
    recorder: Recorder,
}

impl Movie {
    pub fn start(device: &Device, format: TextureFormat, plan: Plan) -> Result<Self, String> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grana.movie"),
            size: wgpu::Extent3d {
                width: plan.size[0],
                height: plan.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let recorder =
            Recorder::start(&plan.path, plan.size, plan.fps).map_err(|error| error.to_string())?;
        Ok(Self {
            path: plan.path,
            texture,
            view,
            size: plan.size,
            volume: plan.volume,
            frame: 0,
            frames: plan.frames,
            attempts: 0,
            resume: plan.resume,
            position: plan.position,
            recorder,
        })
    }

    pub fn target(&self) -> &TextureView {
        &self.view
    }

    pub fn capture(&self, device: &Device, queue: &Queue, format: TextureFormat) -> Image {
        grana_render::read_image(device, queue, &self.texture, self.size, format)
    }

    pub fn push(&mut self, image: &Image) -> Result<(), String> {
        self.recorder
            .push(image.pixels())
            .map_err(|error| error.to_string())?;
        self.frame += 1;
        self.attempts = 0;
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.frame >= self.frames
    }

    pub fn resume(&self) -> (u32, f32) {
        (self.resume, self.position)
    }

    pub fn finish(self) -> Result<PathBuf, String> {
        let path = self.path.clone();
        self.recorder.finish().map_err(|error| error.to_string())?;
        Ok(path)
    }
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("the movie")
        .to_string()
}
