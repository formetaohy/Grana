use grana_display::CameraState;
use grana_render::{Digest, FrameInfo, PickId, Renderer, Requests};
use grana_scene::{Item, Selection};
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::display::{Display, View};
use crate::frame;
use crate::image::ViewportImage;
use crate::splat::{SplatCache, SplatMembers};
use crate::view::{self, RenderedScene};

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

#[derive(Clone, Copy, PartialEq)]
struct Request {
    scene: u64,
    overlays: u64,
    camera: CameraState,
    size: [u32; 2],
    level_cap: usize,
    generation: u64,
    display: Display,
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
    members: Vec<SplatMembers>,
}

struct Prepared {
    scene_key: u64,
    targets: view::Targets,
    scene: RenderedScene,
}

pub struct Engine {
    renderer: Renderer,
    pending_pick: Option<Identities>,
    prepared: Option<Prepared>,
    splats: SplatCache,
    image: Option<ViewportImage>,
    request: Option<Request>,
    generation: u64,
    image_revision: u64,
    info: FrameInfo,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            pending_pick: None,
            prepared: None,
            splats: SplatCache::new(),
            image: None,
            request: None,
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
        self.splats.clear();
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
        self.renderer.upload(queue, bricks);
        self.generation = self.renderer.generation();
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, view: View<'_>) -> Result<(), String> {
        assert!(
            view.size[0] > 0 && view.size[1] > 0,
            "a viewport covers at least one pixel"
        );
        self.renderer.set_exporting(false);
        let scene_key = view::key(view.scene);
        let request = Request {
            scene: scene_key,
            overlays: overlay_key(view.overlays),
            camera: view.camera.state(),
            size: view.size,
            level_cap: view.level_cap,
            generation: self.generation,
            display: view.display,
        };
        if self.request == Some(request) && self.resolved(view.display) {
            return Ok(());
        }
        let size = view.size;
        if self.image.as_ref().is_none_or(|image| image.size != size) {
            self.image = Some(ViewportImage::new(device, size, self.renderer.format()));
        }
        let detail = view::plan_with(
            view.scene,
            Some(view::Lens {
                camera: view.camera,
                height: size[1],
            }),
            &mut self.splats,
        );
        if self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.scene_key != scene_key || prepared.targets != detail)
        {
            let scene = view::frame(view.scene, &detail, &mut self.splats)?;
            self.prepared = Some(Prepared {
                scene_key,
                scene,
                targets: detail,
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
                frame::View {
                    target: &image.view,
                    scene: view.scene,
                    camera: view.camera,
                    viewport: [0, 0, size[0], size[1]],
                    size,
                    samples: SAMPLE_LIMIT,
                    display: view.display.render(),
                    level_cap: view.level_cap,
                    overlays: view.overlays,
                },
                &prepared.scene,
            )
        };
        self.info = info;
        self.request = Some(request);
        if info.samples != 0 {
            self.image_revision = self.image_revision.wrapping_add(1);
        }
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

    fn resolved(&self, display: Display) -> bool {
        let info = self.info;
        if display == Display::Interactive {
            return info.samples != 0;
        }
        if display.awaits() {
            return info.samples != 0 && !info.fading;
        }
        info.samples >= SAMPLE_LIMIT && !info.fading && info.view.resident >= info.view.demand
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
            styles: prepared.scene.style_ids.clone(),
            members: prepared.scene.members.clone(),
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

fn overlay_key(overlays: &[grana_render::Overlay<'_>]) -> u64 {
    let mut key = Digest::new();
    key.mix(overlays.len() as u64);
    for overlay in overlays {
        key.mix(overlay.digest());
    }
    key.finish()
}
