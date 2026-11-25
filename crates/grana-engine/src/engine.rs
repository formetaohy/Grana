use std::collections::HashSet;
use std::sync::Arc;

use grana_display::CameraState;
use grana_gpu::{Ledger, Pipelines};
use grana_object::ObjectId;
use grana_render::{Delivery, FrameInfo, PickId, Renderer, Requests};
use grana_scene::{Item, Selection};
use grana_volume::{BrickRequest, Clip, Damage, PackedBrick, Pyramid, Region};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};
use crate::image::ViewportImage;
use crate::splat::SplatMembers;
use crate::view::{self, Cache, PickRows, RenderedScene};

const PLAN_BAND: f32 = 1.4;

#[derive(Clone, Copy, PartialEq)]
struct Content {
    scene: u64,
    overlays: u64,
    camera: CameraState,
    region: Region,
    clip: Clip,
    exposure: u32,
    size: [u32; 2],
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
    picks: Vec<PickRows>,
    members: Vec<SplatMembers>,
}

struct Prepared {
    topology: u64,
    classification: u64,
    appearance: u64,
    display: u64,
    targets: Arc<view::Targets>,
    rendered: RenderedScene,
}

struct Plan {
    topology: u64,
    classification: u64,
    camera: CameraState,
    scale: f32,
    targets: Arc<view::Targets>,
}

impl Plan {
    fn accepts(&self, keys: view::Key, camera: CameraState, scale: f32, moving: bool) -> bool {
        self.topology == keys.topology()
            && self.classification == keys.classification()
            && (self.camera == camera
                || (moving && (scale / self.scale).ln().abs() <= PLAN_BAND.ln()))
    }
}

struct Failure {
    content: Content,
    detail: String,
}

pub struct Engine {
    renderer: Renderer,
    budget: u64,
    pending_pick: Option<Identities>,
    prepared: Option<Prepared>,
    plan: Option<Plan>,
    cache: Cache,
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
        ledger: &Arc<Ledger>,
        budget: u64,
        pipelines: &Pipelines,
    ) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, ledger, budget, pipelines),
            budget,
            pending_pick: None,
            prepared: None,
            plan: None,
            cache: Cache::new(),
            image: None,
            content: None,
            failure: None,
            generation: 0,
            image_revision: 0,
            info: FrameInfo::default(),
        }
    }

    pub fn set_dataset(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        budget: u64,
        size: [u32; 2],
    ) -> Result<(), String> {
        assert!(budget > 0, "the engine keeps a positive texture budget");
        self.budget = budget;
        self.renderer.set_dataset(device, pyramid, budget, size)?;
        self.pending_pick = None;
        self.prepared = None;
        self.plan = None;
        self.cache.clear();
        self.content = None;
        self.failure = None;
        self.info = FrameInfo::default();
        Ok(())
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
        self.generation = self.renderer.generation();
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    pub fn poll_requests(
        &mut self,
        device: &Device,
        time: u32,
        pending: &HashSet<BrickRequest>,
    ) -> Result<Option<Requests>, String> {
        let Some(requests) = self.renderer.poll_requests(device, pending)? else {
            return Ok(None);
        };
        if requests.time == time {
            self.info.view = requests.view;
        }
        Ok(Some(requests))
    }

    pub fn upload(&mut self, bricks: &[(BrickRequest, PackedBrick)]) {
        self.renderer.upload(bricks);
        self.generation = self.renderer.generation();
    }

    pub fn abandon(&mut self, requests: &[BrickRequest]) {
        self.renderer.abandon(requests);
        self.generation = self.renderer.generation();
    }

    pub fn statistics(&self) -> grana_render::Statistics {
        self.renderer.statistics()
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, view: View<'_>) -> Result<(), String> {
        assert!(
            view.size[0] > 0 && view.size[1] > 0,
            "a viewport covers at least one pixel"
        );
        self.renderer.set_exporting(false);
        let keys = view::key(view.scene);
        let content = self.content_of(&view, keys);
        if self.content == Some(content) && self.resolved(view.delivery) {
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
        match self.render(device, queue, view, keys, content) {
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

    fn content_of(&self, view: &View<'_>, keys: view::Key) -> Content {
        Content {
            scene: keys.digest(),
            overlays: grana_render::overlay_digest(view.overlays),
            camera: view.camera.state(),
            region: view.scene.region,
            clip: view.scene.clip,
            exposure: view.scene.exposure.to_bits(),
            size: view.size,
            generation: self.generation,
        }
    }

    fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        view: View<'_>,
        keys: view::Key,
        content: Content,
    ) -> Result<(), String> {
        let size = view.size;
        if self.image.as_ref().is_none_or(|image| image.size != size) {
            self.image = Some(ViewportImage::new(device, size, self.renderer.format()));
        }
        let moving = self
            .content
            .is_some_and(|previous| previous.camera != content.camera);
        let scale = view
            .camera
            .world_per_point(size[1] as f32, view.camera.distance());
        let detail = match &self.plan {
            Some(plan) if plan.accepts(keys, content.camera, scale, moving) => {
                Arc::clone(&plan.targets)
            }
            _ => {
                let targets = Arc::new(view::plan_with(
                    view.scene,
                    Some(view::Lens {
                        camera: view.camera,
                        height: size[1],
                    }),
                    &mut self.cache,
                ));
                self.plan = Some(Plan {
                    topology: keys.topology(),
                    classification: keys.classification(),
                    camera: content.camera,
                    scale,
                    targets: Arc::clone(&targets),
                });
                targets
            }
        };
        if self.prepared.as_ref().is_none_or(|prepared| {
            prepared.topology != keys.topology()
                || prepared.classification != keys.classification()
                || prepared.targets.as_ref() != detail.as_ref()
        }) {
            let rendered = view::frame(view.scene, &detail, &mut self.cache)?;
            self.prepared = Some(Prepared {
                topology: keys.topology(),
                classification: keys.classification(),
                appearance: keys.appearance(),
                display: keys.display(),
                targets: detail,
                rendered,
            });
        } else if let Some(prepared) = self.prepared.as_mut()
            && (prepared.appearance != keys.appearance() || prepared.display != keys.display())
        {
            prepared.appearance = keys.appearance();
            prepared.display = keys.display();
            prepared.rendered = view::frame(view.scene, &prepared.targets, &mut self.cache)?;
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
                frame::Draw {
                    target: &image.view,
                    view: View {
                        scene: view.scene,
                        camera: view.camera,
                        size,
                        delivery: view.delivery,
                        overlays: view.overlays,
                    },
                },
                &prepared.rendered,
            )?
        };
        self.info = info;
        self.content = Some(content);
        if info.drawn {
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

    fn resolved(&self, delivery: Delivery) -> bool {
        let info = self.info;
        if self.image.is_none() {
            return false;
        }
        if delivery.awaits() {
            return info.drawn && !info.fading;
        }
        info.refined() && !info.fading && info.view.resident >= info.view.demand
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
            picks: prepared.rendered.picks().to_vec(),
            members: prepared.rendered.members.clone(),
        });
        true
    }

    pub fn poll_pick(&mut self, device: &Device) -> Result<Option<Picked>, String> {
        if self.pending_pick.is_none() {
            return Ok(None);
        }
        let Some(pick) = self.renderer.poll_pick(device)? else {
            return Ok(None);
        };
        let identities = self.pending_pick.take().expect("the pick was pending");
        let picked = match pick {
            PickId::None => Picked::Miss,
            PickId::Expired => Picked::Abandoned,
            PickId::Style(index) => identities
                .picks
                .iter()
                .find_map(|rows| rows.resolve(index))
                .map_or(Picked::Miss, Picked::Hit),
            PickId::Splat { key, level, index } => {
                let members = identities
                    .members
                    .iter()
                    .find(|members| members.key() == key);
                match members.and_then(|members| members.resolve(level, index)) {
                    Some((_, crate::spot::STANDALONE)) => Picked::Seed,
                    Some((set, member)) => match ObjectId::from_raw(member) {
                        Some(id) => Picked::Hit(Selection {
                            set,
                            item: Item::Object(id),
                        }),
                        None => Picked::Miss,
                    },
                    None => Picked::Miss,
                }
            }
        };
        Ok(Some(picked))
    }

    pub fn pick_pending(&self) -> bool {
        self.pending_pick.is_some()
    }
}
