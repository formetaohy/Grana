use std::sync::Arc;

use grana_display::CameraState;
use grana_gpu::Pipelines;
use grana_render::{FrameInfo, PickId, Renderer, Requests};
use grana_scene::{Item, Selection};
use grana_volume::{BrickRequest, Clip, Damage, PackedBrick, Pyramid, Region};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame;
use crate::image::ViewportImage;
use crate::presentation::{Presentation, View};
use crate::splat::{SplatCache, SplatMembers};
use crate::view::{self, RenderedScene};

const TEXTURE_BUDGET: u64 = 512 << 20;

#[derive(Clone, Copy, PartialEq)]
struct Content {
    scene: u64,
    overlays: u64,
    camera: CameraState,
    region: Region,
    clip: Clip,
    exposure: u32,
    size: [u32; 2],
    level_cap: usize,
    generation: u64,
}

impl Content {
    fn same_structure(&self, other: &Self) -> bool {
        self.scene == other.scene
            && self.overlays == other.overlays
            && self.camera == other.camera
            && self.region == other.region
            && self.clip == other.clip
            && self.size == other.size
            && self.level_cap == other.level_cap
            && self.generation == other.generation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    Hit(Selection),
    Seed,
    Miss,
    Abandoned,
}

#[derive(Clone, Default)]
struct Identities {
    styles: Vec<Selection>,
    members: Vec<SplatMembers>,
}

struct Prepared {
    structure: u64,
    display: u64,
    targets: Arc<view::Targets>,
    rendered: RenderedScene,
}

struct Plan {
    structure: u64,
    camera: CameraState,
    height: u32,
    targets: Arc<view::Targets>,
}

struct Failure {
    content: Content,
    detail: String,
}

pub struct Engine {
    renderer: Renderer,
    pending_pick: Option<Identities>,
    prepared: Option<Prepared>,
    plan: Option<Plan>,
    splats: SplatCache,
    image: Option<ViewportImage>,
    content: Option<Content>,
    failure: Option<Failure>,
    generation: u64,
    image_revision: u64,
    info: FrameInfo,
}

impl Engine {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        pipelines: &Pipelines,
    ) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET, pipelines),
            pending_pick: None,
            prepared: None,
            plan: None,
            splats: SplatCache::new(),
            image: None,
            content: None,
            failure: None,
            generation: 0,
            image_revision: 0,
            info: FrameInfo::default(),
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, pyramid);
        self.pending_pick = None;
        self.prepared = None;
        self.plan = None;
        self.splats.clear();
        self.content = None;
        self.failure = None;
        self.info = FrameInfo::default();
    }

    pub fn holds(&self, pyramid: &Pyramid) -> bool {
        self.renderer.holds(pyramid)
    }

    pub fn invalidate(&mut self, pyramid: &Pyramid) {
        self.renderer.invalidate(pyramid);
        self.generation = self.renderer.generation();
        self.pending_pick = None;
        self.content = None;
        self.failure = None;
        self.info = FrameInfo::default();
    }

    pub fn damage(&mut self, damages: &[Damage]) {
        self.renderer.damage(damages);
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

    pub fn upload(&mut self, bricks: &[(BrickRequest, PackedBrick)]) {
        self.renderer.upload(bricks);
        self.generation = self.renderer.generation();
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, view: View<'_>) -> Result<(), String> {
        assert!(
            view.size[0] > 0 && view.size[1] > 0,
            "a viewport covers at least one pixel"
        );
        self.renderer.set_exporting(false);
        let content = self.content_of(&view);
        if self.content == Some(content) && self.resolved(view.presentation) {
            return Ok(());
        }
        if let Some(previous) = self.content
            && previous.exposure != content.exposure
            && previous.same_structure(&content)
            && self.represent(device, queue, view.scene.exposure)
        {
            self.content = Some(content);
            return Ok(());
        }
        if let Some(failure) = self.failure.as_ref().filter(|held| held.content == content) {
            return Err(failure.detail.clone());
        }
        match self.render(device, queue, view, content) {
            Ok(()) => {
                self.failure = None;
                Ok(())
            }
            Err(detail) => {
                self.failure = Some(Failure {
                    content,
                    detail: detail.clone(),
                });
                Err(detail)
            }
        }
    }

    fn content_of(&self, view: &View<'_>) -> Content {
        Content {
            scene: view::key(view.scene).digest(),
            overlays: grana_render::overlay_digest(view.overlays),
            camera: view.camera.state(),
            region: view.scene.region,
            clip: view.scene.clip,
            exposure: view.scene.exposure.to_bits(),
            size: view.size,
            level_cap: view.level_cap,
            generation: self.generation,
        }
    }

    fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        view: View<'_>,
        content: Content,
    ) -> Result<(), String> {
        let keys = view::key(view.scene);
        let size = view.size;
        if self.image.as_ref().is_none_or(|image| image.size != size) {
            self.image = Some(ViewportImage::new(device, size, self.renderer.format()));
        }
        let detail = match &self.plan {
            Some(plan)
                if plan.structure == keys.structure()
                    && plan.camera == content.camera
                    && plan.height == size[1] =>
            {
                Arc::clone(&plan.targets)
            }
            _ => {
                let targets = Arc::new(view::plan_with(
                    view.scene,
                    Some(view::Lens {
                        camera: view.camera,
                        height: size[1],
                    }),
                    &mut self.splats,
                ));
                self.plan = Some(Plan {
                    structure: keys.structure(),
                    camera: content.camera,
                    height: size[1],
                    targets: Arc::clone(&targets),
                });
                targets
            }
        };
        if self.prepared.as_ref().is_none_or(|prepared| {
            prepared.structure != keys.structure() || prepared.targets.as_ref() != detail.as_ref()
        }) {
            let rendered = view::frame(view.scene, &detail, &mut self.splats)?;
            self.prepared = Some(Prepared {
                structure: keys.structure(),
                display: keys.display(),
                targets: detail,
                rendered,
            });
        } else if let Some(prepared) = self.prepared.as_mut()
            && prepared.display != keys.display()
        {
            prepared.display = keys.display();
            view::restyle(&mut prepared.rendered, view.scene);
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
                frame::View {
                    target: &image.view,
                    scene: view.scene,
                    camera: view.camera,
                    viewport: [0, 0, size[0], size[1]],
                    size,
                    sampling: view.presentation.sampling(),
                    budget: view.budget,
                    presentation: view.presentation,
                    level_cap: view.level_cap,
                    overlays: view.overlays,
                },
                &prepared.rendered,
            )?
        };
        self.info = info;
        self.content = Some(content);
        if info.samples != 0 {
            self.image_revision = self.image_revision.wrapping_add(1);
        }
        Ok(())
    }

    fn represent(&mut self, device: &Device, queue: &Queue, exposure: f32) -> bool {
        let Some(image) = self.image.as_ref() else {
            return false;
        };
        if !self
            .renderer
            .present_only(device, queue, &image.view, exposure, false)
        {
            return false;
        }
        self.image_revision = self.image_revision.wrapping_add(1);
        true
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

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_ref().map(|failure| failure.detail.as_str())
    }

    fn resolved(&self, presentation: Presentation) -> bool {
        let info = self.info;
        if self.image.is_none() {
            return false;
        }
        if presentation.awaits() {
            return info.samples != 0 && !info.fading;
        }
        info.converged && !info.fading && info.view.resident >= info.view.demand
    }

    pub fn request_pick(&mut self, device: &Device, queue: &Queue, point: [u32; 2]) -> bool {
        if self.pending_pick.is_some() {
            return false;
        }
        let Some(image) = self.image.as_ref() else {
            return false;
        };
        if point[0] >= image.size[0] || point[1] >= image.size[1] {
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
            styles: prepared.rendered.style_ids.clone(),
            members: prepared.rendered.members.clone(),
        });
        true
    }

    pub fn poll_pick(&mut self, device: &Device) -> Option<Picked> {
        self.pending_pick.as_ref()?;
        let pick = self.renderer.poll_pick(device)?;
        let identities = self.pending_pick.take().expect("the pick was pending");
        let picked = match pick {
            PickId::None => Picked::Miss,
            PickId::Expired => Picked::Abandoned,
            PickId::Style(index) => match identities.styles.get(index as usize).copied() {
                Some(selection) => Picked::Hit(selection),
                None => Picked::Miss,
            },
            PickId::Splat { key, level, index } => {
                let members = identities
                    .members
                    .iter()
                    .find(|members| members.key() == key);
                match members.and_then(|members| members.resolve(level, index)) {
                    Some((_, crate::spot::STANDALONE)) => Picked::Seed,
                    Some((set, member)) => Picked::Hit(Selection {
                        set,
                        item: Item::Object(member as usize),
                    }),
                    None => Picked::Miss,
                }
            }
        };
        Some(picked)
    }

    pub fn pick_pending(&self) -> bool {
        self.pending_pick.is_some()
    }
}
