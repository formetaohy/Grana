use grana_display::Camera;
use grana_render::{FrameInfo, PickId, Renderer, Requests};
use grana_scene::{Scene, Selection};
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};
use crate::view::{self, RenderedScene};

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

pub struct ViewportFrame<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub viewport: [u32; 4],
    pub revision: u64,
    pub prefetch: bool,
    pub level_cap: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    Hit(Selection),
    Seed,
    Miss,
}

#[derive(Clone, Default)]
struct Identities {
    styles: Vec<Selection>,
    markers: Vec<Option<Selection>>,
}

struct Prepared {
    revision: u64,
    detail: Vec<usize>,
    scene: RenderedScene,
}

pub struct Engine {
    renderer: Renderer,
    pending_pick: Option<Identities>,
    prepared: Option<Prepared>,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            pending_pick: None,
            prepared: None,
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, pyramid);
        self.pending_pick = None;
        self.prepared = None;
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        self.renderer.poll_requests(device)
    }

    pub fn upload(&mut self, queue: &Queue, bricks: &[(BrickRequest, BrickData)]) -> usize {
        self.renderer.upload(queue, bricks)
    }

    pub fn draw_viewport(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: ViewportFrame<'_>,
    ) -> Result<FrameInfo, String> {
        self.renderer.set_exporting(false);
        let lens = view::Lens {
            camera: frame.camera,
            height: frame.viewport[3].max(1),
        };
        let detail = view::detail(frame.scene, Some(lens));
        if self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.revision != frame.revision || prepared.detail != detail)
        {
            let scene = view::frame(frame.scene, frame.revision, &detail)?;
            self.prepared = Some(Prepared {
                revision: frame.revision,
                detail,
                scene,
            });
        }
        let Self {
            renderer, prepared, ..
        } = self;
        let prepared = prepared.as_ref().expect("the rendered scene is ready");
        Ok(frame::draw(
            renderer,
            device,
            queue,
            View {
                target: frame.target,
                scene: frame.scene,
                camera: frame.camera,
                viewport: frame.viewport,
                size: frame.size,
                samples: SAMPLE_LIMIT,
                prefetch: frame.prefetch,
                level_cap: frame.level_cap,
            },
            &prepared.scene,
        ))
    }

    pub fn request_pick(&mut self, device: &Device, queue: &Queue, point: [u32; 2]) -> bool {
        if self.pending_pick.is_some() {
            return false;
        }
        let Self {
            renderer,
            pending_pick,
            prepared,
        } = self;
        let Some(prepared) = prepared.as_ref() else {
            return false;
        };
        if !renderer.request_pick(device, queue, point) {
            return false;
        }
        *pending_pick = Some(Identities {
            styles: prepared.scene.style_ids.clone(),
            markers: prepared.scene.marker_ids.clone(),
        });
        true
    }

    pub fn poll_pick(&mut self, device: &Device) -> Option<Picked> {
        self.pending_pick.as_ref()?;
        let pick = self.renderer.poll_pick(device)?;
        let identities = self.pending_pick.take().expect("the pick was pending");
        let picked = match pick {
            PickId::None => Picked::Miss,
            PickId::Style(index) => match identities.styles.get(index as usize).copied() {
                Some(selection) => Picked::Hit(selection),
                None => Picked::Miss,
            },
            PickId::Marker(index) => match identities.markers.get(index as usize) {
                Some(Some(selection)) => Picked::Hit(*selection),
                Some(None) => Picked::Seed,
                None => Picked::Miss,
            },
        };
        Some(picked)
    }

    pub fn pick_pending(&self) -> bool {
        self.pending_pick.is_some()
    }
}
