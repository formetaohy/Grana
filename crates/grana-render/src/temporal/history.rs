use glam::{Mat4, Vec3};
use wgpu::{Device, Texture, TextureFormat, TextureUsages, TextureView};

use crate::target::{SCENE_FORMAT, image, view};

pub(crate) const STATISTICS_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub(crate) const COLOR_BYTES: u64 = 8;

pub(crate) const STATISTICS_BYTES: u64 = 8;

const HELD: TextureUsages = TextureUsages::RENDER_ATTACHMENT.union(TextureUsages::TEXTURE_BINDING);

pub(crate) struct History {
    color: [TextureView; 2],
    statistics: [TextureView; 2],
    resolved: Texture,
    resolved_view: TextureView,
    read: usize,
    intact: bool,
    camera: Option<Mat4>,
    eye: Option<Vec3>,
    reach: f32,
}

impl History {
    pub(crate) fn new(device: &Device, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a temporal history covers a positive surface"
        );
        let color = [0, 1].map(|_| {
            view(&image(
                device,
                "grana.temporal.history",
                size,
                SCENE_FORMAT,
                HELD,
            ))
        });
        let statistics = [0, 1].map(|_| {
            view(&image(
                device,
                "grana.temporal.statistics",
                size,
                STATISTICS_FORMAT,
                HELD,
            ))
        });
        let resolved = image(
            device,
            "grana.temporal.resolved",
            size,
            SCENE_FORMAT,
            HELD | TextureUsages::COPY_SRC,
        );
        let resolved_view = view(&resolved);
        Self {
            color,
            statistics,
            resolved,
            resolved_view,
            read: 0,
            intact: false,
            camera: None,
            eye: None,
            reach: 0.0,
        }
    }

    pub(crate) fn size(size: [u32; 2]) -> u64 {
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        pixels * (2 * COLOR_BYTES + 2 * STATISTICS_BYTES + COLOR_BYTES)
    }

    pub(crate) fn intact(&self) -> bool {
        self.intact
    }

    pub(crate) fn invalidate(&mut self) {
        self.intact = false;
        self.reach = 0.0;
    }

    pub(crate) fn camera(&self) -> Option<Mat4> {
        self.camera
    }

    pub(crate) fn eye(&self) -> Option<Vec3> {
        self.eye
    }

    pub(crate) fn reach(&self) -> f32 {
        self.reach
    }

    pub(crate) fn read_color(&self) -> &TextureView {
        &self.color[self.read]
    }

    pub(crate) fn write_color(&self) -> &TextureView {
        &self.color[1 - self.read]
    }

    pub(crate) fn read_statistics(&self) -> &TextureView {
        &self.statistics[self.read]
    }

    pub(crate) fn write_statistics(&self) -> &TextureView {
        &self.statistics[1 - self.read]
    }

    pub(crate) fn resolved(&self) -> &TextureView {
        &self.resolved_view
    }

    pub(crate) fn resolved_texture(&self) -> &Texture {
        &self.resolved
    }

    pub(crate) fn advance(&mut self, camera: Mat4, eye: Vec3, reach: f32) {
        assert!(
            reach > 0.0 && reach.is_finite(),
            "a temporal history names a positive depth reach"
        );
        self.read = 1 - self.read;
        self.intact = true;
        self.camera = Some(camera);
        self.eye = Some(eye);
        self.reach = reach;
    }
}
