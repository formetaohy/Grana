use std::time::{Duration, Instant};

use grana_display::CameraState;
use grana_render::{FrameInfo, PickId, Renderer, Requests};
use grana_scene::Selection;
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::display::View;
use crate::frame;
use crate::image::ViewportImage;
use crate::view::{self, RenderedScene};

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

const DRAFT_BUDGET: Duration = Duration::from_millis(10);
const DRAFT_HOLD: Duration = Duration::from_millis(150);
const DRAFT_DEFAULT: f32 = 0.5;
const DRAFT_MINIMUM: f32 = 0.25;
const SAMPLE_FLOOR: Duration = Duration::from_millis(20);
const SAMPLE_BLEND: f64 = 0.5;

#[derive(Clone, Copy, PartialEq)]
struct Request {
    scene: u64,
    camera: CameraState,
    size: [u32; 2],
    level_cap: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Draft,
    Refine,
    Ready,
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
    splats: Vec<Option<Selection>>,
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
    image: Option<ViewportImage>,
    request: Option<Request>,
    display: [u32; 2],
    phase: Phase,
    draft: f32,
    sample: Option<Duration>,
    drawn_at: Option<Instant>,
    changed_at: Option<Instant>,
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
            image: None,
            request: None,
            display: [0, 0],
            phase: Phase::Ready,
            draft: DRAFT_DEFAULT,
            sample: None,
            drawn_at: None,
            changed_at: None,
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
        self.request = None;
        self.phase = Phase::Ready;
        self.drawn_at = None;
        self.changed_at = None;
        self.generation = 0;
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
        let now = Instant::now();
        let scene_key = view::key(view.scene);
        let request = Request {
            scene: scene_key,
            camera: view.camera.state(),
            size: view.size,
            level_cap: view.level_cap,
            generation: self.generation,
        };
        let animating = view.display == crate::display::Display::Interactive;
        if self.request != Some(request) {
            self.request = None;
            self.phase = if animating {
                Phase::Draft
            } else {
                Phase::Refine
            };
            self.changed_at = Some(now);
            self.drawn_at = None;
        } else {
            match self.phase {
                Phase::Ready => return Ok(()),
                Phase::Draft => {
                    let held = self
                        .changed_at
                        .is_some_and(|at| now.saturating_duration_since(at) >= DRAFT_HOLD);
                    if animating || !held {
                        self.drawn_at = None;
                        return Ok(());
                    }
                    self.phase = Phase::Refine;
                    self.drawn_at = None;
                }
                Phase::Refine => {
                    if animating {
                        self.drawn_at = None;
                        return Ok(());
                    }
                }
            }
        }
        let size = match self.phase {
            Phase::Draft => draft_size(view.size, self.draft),
            _ => view.size,
        };
        if self.image.as_ref().is_none_or(|image| image.size != size) {
            self.image = Some(ViewportImage::new(device, size, self.renderer.format()));
        }
        let detail = view::plan(
            view.scene,
            Some(view::Lens {
                camera: view.camera,
                height: view.size[1],
            }),
        );
        if self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.scene_key != scene_key || prepared.targets != detail)
        {
            self.prepared = Some(Prepared {
                scene_key,
                scene: view::frame(view.scene, &detail)?,
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
                },
                &prepared.scene,
            )
        };
        self.info = info;
        self.request = Some(request);
        self.display = view.size;
        if info.samples != 0 {
            self.image_revision = self.image_revision.wrapping_add(1);
        }
        if self.phase == Phase::Refine {
            if let Some(last) = self.drawn_at {
                self.observe(now.saturating_duration_since(last));
            }
            self.drawn_at = Some(now);
            if self.resolved(view.display) {
                self.phase = Phase::Ready;
                self.drawn_at = None;
            }
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

    fn observe(&mut self, interval: Duration) {
        if interval <= SAMPLE_FLOOR {
            self.draft = 1.0;
            return;
        }
        let blended = self.sample.map_or(interval, |sample| {
            sample.mul_f64(1.0 - SAMPLE_BLEND) + interval.mul_f64(SAMPLE_BLEND)
        });
        self.sample = Some(blended);
        self.draft = draft_scale(blended);
    }

    fn resolved(&self, display: crate::display::Display) -> bool {
        let info = self.info;
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
        let display = self.display;
        let point = [
            (u64::from(point[0]) * u64::from(image.size[0]) / u64::from(display[0].max(1))) as u32,
            (u64::from(point[1]) * u64::from(image.size[1]) / u64::from(display[1].max(1))) as u32,
        ];
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
            splats: prepared.scene.splat_ids.clone(),
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
            PickId::Splat(index) => match identities.splats.get(index as usize) {
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

fn draft_size(size: [u32; 2], scale: f32) -> [u32; 2] {
    let scale = scale.clamp(DRAFT_MINIMUM, 1.0);
    [
        ((size[0] as f32 * scale).round() as u32).max(1),
        ((size[1] as f32 * scale).round() as u32).max(1),
    ]
}

fn draft_scale(interval: Duration) -> f32 {
    (DRAFT_BUDGET.as_secs_f64() / interval.as_secs_f64())
        .sqrt()
        .clamp(f64::from(DRAFT_MINIMUM), 1.0) as f32
}
