use grana_display::{PANE_COUNT, PaneKind, Panes};
use grana_render::{FrameInfo, PaneFrame, PickId, Renderer, Requests};
use grana_scene::{Scene, Selection};
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{Device, Queue, TextureFormat, TextureView};

use crate::frame::{self, View};

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

pub struct Engine {
    renderer: Renderer,
    identities: Identities,
    pending_pick: Option<Identities>,
}

impl Engine {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, TEXTURE_BUDGET),
            identities: Identities::default(),
            pending_pick: None,
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(device, pyramid);
        self.identities = Identities::default();
        self.pending_pick = None;
    }

    pub fn set_frame_target(&mut self, target: f32) {
        self.renderer.set_frame_target(target);
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
        let channels = std::array::from_fn(|index| frame.panes.pane(index).channels());
        let quality = std::array::from_fn(|index| frame.panes.pane(index).quality());
        let info = frame::draw(
            &mut self.renderer,
            device,
            queue,
            View {
                target: frame.target,
                scene: frame.scene,
                panes,
                channels,
                quality,
                size: frame.size,
                samples: SAMPLE_LIMIT,
                prefetch: frame.prefetch,
                level_cap: frame.level_cap,
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
                lens: grana_render::Lens {
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
