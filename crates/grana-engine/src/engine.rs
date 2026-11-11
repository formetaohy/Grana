use grana_display::{Camera, CameraState};
use grana_render::{FrameInfo, PickId, Renderer, Requests};
use grana_scene::{Scene, Selection};
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};
use crate::image::ViewportImage;
use crate::view::{self, RenderedScene};

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

pub struct ViewportFrame<'a> {
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub revision: u64,
    pub prefetch: bool,
    pub level_cap: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct Request {
    revision: u64,
    camera: CameraState,
    size: [u32; 2],
    level_cap: usize,
    prefetch: bool,
    uploads: u64,
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
    image: Option<ViewportImage>,
    request: Option<Request>,
    uploads: u64,
    image_revision: u64,
    info: FrameInfo,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            pending_pick: None,
            prepared: None,
            image: None,
            request: None,
            uploads: 0,
            image_revision: 0,
            info: FrameInfo::default(),
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, pyramid);
        self.pending_pick = None;
        self.prepared = None;
        self.request = None;
        self.info = FrameInfo::default();
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    pub fn poll_requests(&mut self, device: &Device, time: u32) -> Option<Requests> {
        let requests = self.renderer.poll_requests(device)?;
        if requests.time == time {
            self.info.view = requests.view;
        }
        Some(requests)
    }

    pub fn upload(&mut self, queue: &Queue, bricks: &[(BrickRequest, BrickData)]) {
        let count = self.renderer.upload(queue, bricks);
        self.uploads = self.uploads.wrapping_add(count as u64);
    }

    pub fn draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: ViewportFrame<'_>,
    ) -> Result<(), String> {
        assert!(
            frame.size[0] > 0 && frame.size[1] > 0,
            "a viewport covers at least one pixel"
        );
        self.renderer.set_exporting(false);
        if self
            .image
            .as_ref()
            .is_none_or(|image| image.size != frame.size)
        {
            self.image = Some(ViewportImage::new(
                device,
                frame.size,
                self.renderer.format(),
            ));
            self.request = None;
        }
        let request = Request {
            revision: frame.revision,
            camera: frame.camera.state(),
            size: frame.size,
            level_cap: frame.level_cap,
            prefetch: frame.prefetch,
            uploads: self.uploads,
        };
        if self.request == Some(request) && self.finished() {
            return Ok(());
        }
        let detail = view::detail(
            frame.scene,
            Some(view::Lens {
                camera: frame.camera,
                height: frame.size[1],
            }),
        );
        if self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.revision != frame.revision || prepared.detail != detail)
        {
            self.prepared = Some(Prepared {
                revision: frame.revision,
                scene: view::frame(frame.scene, frame.revision, &detail)?,
                detail,
            });
        }
        let info = {
            let Self {
                renderer,
                prepared,
                image,
                ..
            } = self;
            let prepared = prepared.as_ref().expect("the rendered scene is ready");
            let image = image.as_ref().expect("the viewport image is ready");
            frame::draw(
                renderer,
                device,
                queue,
                View {
                    target: &image.view,
                    scene: frame.scene,
                    camera: frame.camera,
                    viewport: [0, 0, frame.size[0], frame.size[1]],
                    size: frame.size,
                    samples: SAMPLE_LIMIT,
                    prefetch: frame.prefetch,
                    level_cap: frame.level_cap,
                },
                &prepared.scene,
            )
        };
        self.info = info;
        self.request = Some(request);
        self.image_revision = self.image_revision.wrapping_add(1);
        Ok(())
    }

    pub fn view(&self) -> Option<&TextureView> {
        self.image.as_ref().map(|image| &image.view)
    }

    pub fn image_revision(&self) -> u64 {
        self.image_revision
    }

    pub fn info(&self) -> FrameInfo {
        self.info
    }

    fn finished(&self) -> bool {
        let info = self.info;
        info.samples >= SAMPLE_LIMIT && !info.fading && info.view.resident >= info.view.demand
    }

    pub fn request_pick(&mut self, device: &Device, queue: &Queue, point: [u32; 2]) -> bool {
        if self.pending_pick.is_some() {
            return false;
        }
        let Self {
            renderer,
            pending_pick,
            prepared,
            ..
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
