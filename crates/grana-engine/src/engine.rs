use grana_display::Viewport;
use grana_render::{FrameInfo, PickId, PickPane, Renderer, Requests};
use grana_scene::{Scene, Selection};
use grana_ui::Ui;
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};
use crate::output::{Export, Outcome, Plan, Pose};

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

pub struct ViewportFrame<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub viewport: &'a Viewport,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub prefetch: bool,
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

pub struct Engine {
    renderer: Renderer,
    export: Option<Export>,
    output_message: Option<String>,
    identities: Identities,
    pending_pick: Option<Identities>,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            export: None,
            output_message: None,
            identities: Identities::default(),
            pending_pick: None,
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_dataset(device, pyramid);
        self.identities = Identities::default();
        self.pending_pick = None;
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        self.renderer.poll_requests(device)
    }

    pub fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        request: BrickRequest,
        data: &BrickData,
    ) {
        self.renderer.upload(device, queue, request, data);
    }

    pub fn draw_viewport(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: ViewportFrame<'_>,
    ) -> Result<FrameInfo, String> {
        let rendered = crate::view::frame(frame.scene)?;
        let info = frame::draw(
            &mut self.renderer,
            device,
            queue,
            View {
                target: frame.target,
                scene: frame.scene,
                camera: frame.viewport.camera(),
                section: frame.viewport.section(),
                size: frame.size,
                volume_region: frame.volume_region,
                section_region: frame.section_region,
                samples: SAMPLE_LIMIT,
                prefetch: frame.prefetch,
                transparent: false,
            },
            &rendered,
        );
        self.identities = Identities {
            styles: rendered.style_ids,
            markers: rendered.marker_ids,
        };
        Ok(info)
    }

    pub fn request_pick(
        &mut self,
        device: &Device,
        queue: &Queue,
        pane: PickPane,
        point: [u32; 2],
    ) -> bool {
        if self.pending_pick.is_some() || !self.renderer.request_pick(device, queue, pane, point) {
            return false;
        }
        self.pending_pick = Some(self.identities.clone());
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

    pub fn start_export(&mut self, device: &Device, plan: Plan) -> Result<(), String> {
        if self.export.is_some() {
            return Err("an export is already running".to_string());
        }
        self.renderer.begin_export();
        self.export = Some(Export::start(device, self.renderer.format(), plan)?);
        self.output_message = None;
        Ok(())
    }

    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    pub fn progress(&self) -> Option<(u32, u32)> {
        self.export.as_ref().map(Export::progress)
    }

    pub fn pose(&self) -> Option<Pose> {
        self.export.as_ref().map(Export::pose)
    }

    pub fn output_message(&self) -> Option<String> {
        self.output_message.clone()
    }

    pub fn set_output_message(&mut self, message: Option<String>) {
        self.output_message = message;
    }

    pub fn render_export(
        &mut self,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui,
        scene: &Scene,
    ) -> Outcome {
        let Self {
            renderer,
            export,
            output_message,
            ..
        } = self;
        let Some(job) = export.as_mut() else {
            return Outcome::Failed {
                detail: "the export disappeared".to_string(),
            };
        };
        match job.render(renderer, device, queue, ui, scene) {
            Outcome::Running => Outcome::Running,
            Outcome::Complete { path } => {
                *output_message = Some(format!("saved {}", crate::output::name(&path)));
                *export = None;
                Outcome::Complete { path }
            }
            Outcome::Failed { detail } => {
                *output_message = Some(detail.clone());
                *export = None;
                Outcome::Failed { detail }
            }
        }
    }

    pub fn cancel_export(&mut self) -> bool {
        if self.export.take().is_none() {
            return false;
        }
        self.output_message = Some("the export was cancelled".to_string());
        true
    }
}
