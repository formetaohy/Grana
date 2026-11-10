use grana_display::{PANE_COUNT, PaneKind, Panes};
use grana_render::{EXPORT_CEILING, FrameInfo, Lens, PaneFrame, PickId, Renderer, Requests};
use grana_scene::{Scene, Selection};
use grana_ui::Ui;
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};
use crate::output::{Export, Outcome, Plan, Pose};
use crate::view::RenderedScene;

const TEXTURE_BUDGET: u64 = 512 << 20;
pub const SAMPLE_LIMIT: u32 = 32;

pub struct ViewportFrame<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub panes: &'a Panes,
    pub size: [u32; 2],
    pub regions: [Option<[u32; 4]>; PANE_COUNT],
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
    time: u32,
    rendered: RenderedScene,
}

pub struct Engine {
    renderer: Renderer,
    dataset: Option<Pyramid>,
    export: Option<Export>,
    output_message: Option<String>,
    identities: Identities,
    pending_pick: Option<Identities>,
    prepared: Option<Prepared>,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            dataset: None,
            export: None,
            output_message: None,
            identities: Identities::default(),
            pending_pick: None,
            prepared: None,
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, pyramid);
        self.dataset = Some(pyramid.clone());
        self.identities = Identities::default();
        self.pending_pick = None;
        self.prepared = None;
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    fn restore_viewport(&mut self, device: &Device) {
        if self.renderer.budget() == TEXTURE_BUDGET {
            return;
        }
        let Some(pyramid) = self.dataset.clone() else {
            return;
        };
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, &pyramid);
        self.identities = Identities::default();
        self.prepared = None;
    }

    pub fn release_residency(&mut self, device: &Device) {
        self.restore_viewport(device);
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
        self.restore_viewport(device);
        self.renderer.set_exporting(false);
        let lens = frame
            .panes
            .panes()
            .iter()
            .enumerate()
            .filter(|(index, pane)| frame.regions[*index].is_some() && pane.kind().volume())
            .find_map(|(index, pane)| {
                let region = frame.regions[index]?;
                Some(crate::view::Lens {
                    camera: pane.camera(),
                    height: region[3],
                })
            });
        let rendered = crate::view::frame(frame.scene, lens)?;
        let panes = pane_frames(frame.panes, frame.regions);
        let info = frame::draw(
            &mut self.renderer,
            device,
            queue,
            View {
                target: frame.target,
                scene: frame.scene,
                panes,
                size: frame.size,
                samples: SAMPLE_LIMIT,
                prefetch: frame.prefetch,
                transparent: false,
                level_cap: frame.level_cap,
                assured: false,
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
        pane: usize,
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
        let Some(pyramid) = self.dataset.clone() else {
            return Err("the dataset is still opening".to_string());
        };
        crate::output::validate(&plan)?;
        let needed = crate::output::residency(&plan, &pyramid)?;
        self.renderer
            .reserve(device, &pyramid, needed.slots, EXPORT_CEILING)?;
        let first = self.renderer.first_level();
        if first > needed.level {
            return Err(format!(
                "an export at this size needs level {} but the renderer addresses levels {first} and coarser",
                needed.level
            ));
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

    pub fn render_export<A: Clone>(
        &mut self,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui<A>,
        scene: &Scene,
        revision: u64,
    ) -> Outcome {
        let Self {
            renderer,
            export,
            output_message,
            prepared,
            ..
        } = self;
        let Some(job) = export.as_mut() else {
            return Outcome::Failed {
                detail: "the export disappeared".to_string(),
            };
        };
        let time = scene.time();
        if prepared
            .as_ref()
            .is_none_or(|held| held.revision != revision || held.time != time)
        {
            let lens = job.lens().map(|(camera, height)| crate::view::Lens {
                camera,
                height: *height,
            });
            match crate::view::frame(scene, lens) {
                Ok(rendered) => {
                    *prepared = Some(Prepared {
                        revision,
                        time,
                        rendered,
                    });
                }
                Err(detail) => {
                    *output_message = Some(detail.clone());
                    return Outcome::Failed { detail };
                }
            }
        }
        let rendered = &prepared
            .as_ref()
            .expect("the prepared scene is ready")
            .rendered;
        renderer.set_exporting(true);
        match job.render(renderer, device, queue, ui, scene, rendered) {
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

fn pane_frames<'a>(
    panes: &'a Panes,
    regions: [Option<[u32; 4]>; PANE_COUNT],
) -> [Option<PaneFrame<'a>>; PANE_COUNT] {
    std::array::from_fn(|index| {
        let region = regions[index]?;
        let pane = panes.pane(index);
        match pane.kind() {
            PaneKind::Volume => Some(PaneFrame::Volume {
                camera: pane.camera(),
                lens: Lens {
                    camera: pane.camera(),
                    aspect: region[2].max(1) as f32 / region[3].max(1) as f32,
                },
                region,
            }),
            PaneKind::Section => Some(PaneFrame::Section {
                section: pane.section(),
                region,
            }),
        }
    })
}
