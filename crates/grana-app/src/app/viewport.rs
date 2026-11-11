use glam::{Vec2, Vec3};
use grana_display::Camera;
use grana_editor::{AnnotationTool, GrabDrag, MaskBrush, PaintDrag, Stage, ToolDrag};
use grana_engine::Picked;
use grana_scene::ObjectFrame;
use grana_scene::ObjectSetId;
use grana_session::{Command, MaskMode, MaskTool, ObjectAnchor, Session};
use grana_ui::Button as UiButton;
use winit::window::CursorIcon;

use super::input::interface_cursor;
use super::{App, Surface, TOOL_CLICK_SLOP};
impl App {
    pub(super) fn interact_viewport(&mut self) {
        if self.session.scene().is_none() {
            if let Some(surface) = self.surface.as_mut() {
                surface
                    .window
                    .set_cursor(interface_cursor(surface.ui.cursor()));
            }
            return;
        }
        let (interaction, hovered) = {
            let App {
                surface,
                session,
                camera,
                stage,
                workspace,
                ..
            } = self;
            let Some(surface) = surface.as_mut() else {
                return;
            };
            let tool = workspace
                .annotation_tool
                .filter(|_| workspace.seed.is_none() && workspace.mask.is_none());
            let paint = workspace.mask.is_some();
            let slice = camera.slice().is_some();
            let blocked = surface.ui.pointer_over_chrome()
                || surface.ui.pointer_over_widget()
                || surface.ui.pointer_captured();
            let hovered = (!blocked && !slice && tool.is_none() && workspace.seed.is_none())
                .then(|| annotation_hit(surface, session, camera, stage))
                .flatten();
            let pressed = surface.ui.input().button(UiButton::Primary).pressed;
            let grabbed = hovered.is_some() && pressed;
            let interaction = stage.input(
                &mut surface.ui,
                camera,
                grana_editor::Intent {
                    tool,
                    paint,
                    grabbed,
                },
            );
            if interaction.camera {
                if let Some(scene) = session.scene() {
                    camera.clamp_focus(0.5 * scene.dataset.world_size());
                }
                surface.window.request_redraw();
            }
            if let Some(steps) = interaction.slice
                && let Some(scene) = session.scene()
            {
                grana_editor::slice_step(scene, camera, steps);
                surface.window.request_redraw();
            }
            let cursor = if paint || workspace.seed.is_some() || tool.is_some() {
                CursorIcon::Crosshair
            } else if interaction.grab.is_some() {
                CursorIcon::Grabbing
            } else if hovered.is_some() {
                CursorIcon::Move
            } else {
                interface_cursor(surface.ui.cursor())
            };
            surface.window.set_cursor(cursor);
            (interaction, hovered)
        };
        if let Some((index, part)) = hovered
            && interaction.grab.is_some()
            && self.workspace.annotation_drag.is_none()
        {
            self.workspace.annotation_drag = Some((index, part));
            self.apply(vec![Command::SelectAnnotation(Some(index))]);
        }
        if let Some(tool) = interaction.tool
            && self.camera.slice().is_none()
        {
            self.use_tool(tool);
        }
        if let Some(grab) = interaction.grab {
            self.drag_annotation(grab);
        }
        if let Some(drag) = interaction.paint
            && self.camera.slice().is_none()
        {
            self.paint_mask(drag);
        }
        if let Some(point) = interaction.pick {
            match (self.workspace.seed, self.camera.slice().is_some()) {
                (Some(set), false) => self.place_seed(set, point, interaction.modifiers.alt),
                (Some(_), true) => {}
                (None, _) => {
                    self.pick_toggle = interaction.modifiers.shift || interaction.modifiers.control;
                    self.pick(point);
                }
            }
            self.request_redraw();
        }
    }

    pub(super) fn use_tool(&mut self, drag: ToolDrag) {
        let Some(tool) = self.workspace.annotation_tool else {
            return;
        };
        match tool {
            AnnotationTool::Roi => self.draw_region(drag),
            AnnotationTool::Point | AnnotationTool::Distance => {
                if !drag.finished || (drag.position - drag.start).length() > TOOL_CLICK_SLOP {
                    return;
                }
                self.place_annotation(tool, drag.position);
            }
        }
    }

    pub(super) fn draw_region(&mut self, drag: ToolDrag) {
        if drag.finished {
            self.default_region(drag.start);
        }
    }

    pub(super) fn region_axes(&self) -> [Vec3; 3] {
        [Vec3::X, Vec3::Y, Vec3::Z]
    }

    pub(super) fn region_depth(&self, scene: &grana_scene::Scene, normal: Vec3) -> f32 {
        let world = scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size().min_element();
        (0.5 * world.abs().dot(normal.abs())).max(voxel)
    }

    pub(super) fn default_region(&mut self, point: Vec2) {
        let Some(centre) = self.annotation_point(point) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let half = 0.5 * scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size();
        let axes = self.region_axes();
        let reach = (0.06 * half.min_element()).max(voxel.min_element());
        let extent = Vec3::new(reach, reach, self.region_depth(scene, axes[2]));
        let centre = Vec3::from(centre).clamp(-half, half);
        self.apply(vec![Command::SetRegion(grana_volume::Region::new(
            centre, axes, extent,
        ))]);
    }

    pub(super) fn drag_annotation(&mut self, drag: GrabDrag) {
        let Some((index, part)) = self.workspace.annotation_drag else {
            return;
        };
        if drag.finished {
            self.workspace.annotation_drag = None;
        }
        if drag.delta.length_squared() <= 0.0 {
            return;
        }
        let Some(offset) = self.annotation_offset(index, drag.delta) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let Some(annotation) = scene.annotations.get(index) else {
            return;
        };
        let kind = annotation.kind();
        let next = match part {
            grana_figure::AnnotationPart::Anchor => {
                let at = (Vec3::from(kind.anchor()) + offset).to_array();
                (at.iter().all(|value| value.is_finite()) && kind.end() != Some(at))
                    .then(|| kind.with_anchor(at))
            }
            grana_figure::AnnotationPart::End => kind.end().and_then(|end| {
                let at = (Vec3::from(end) + offset).to_array();
                (at.iter().all(|value| value.is_finite()) && kind.anchor() != at)
                    .then(|| kind.with_end(at))
            }),
            grana_figure::AnnotationPart::Body => offset
                .is_finite()
                .then(|| kind.translated(offset.to_array())),
        };
        if let Some(kind) = next {
            self.apply(vec![Command::SetAnnotationKind { index, kind }]);
        }
    }

    pub(super) fn annotation_offset(&self, index: usize, delta: Vec2) -> Option<Vec3> {
        let scene = self.session.scene()?;
        let anchor = scene.annotations.get(index)?.kind().anchor();
        let rect = self.stage.viewport();
        if rect.height() <= 0.0 {
            return None;
        }
        let camera = &self.camera;
        let depth = (Vec3::from(anchor) - camera.eye()).dot(camera.forward());
        let per_point = camera.world_per_point(rect.height(), depth);
        Some(camera.right() * delta.x * per_point - camera.up() * delta.y * per_point)
    }

    pub(super) fn paint_mask(&mut self, drag: PaintDrag) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let Some(point) = self.annotation_point(drag.position) else {
            self.mask_path.clear();
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let voxel = scene.dataset.voxel_size().min_element();
        let moved = self
            .mask_path
            .last()
            .is_some_and(|last| (Vec3::from(point) - Vec3::from(*last)).length() > 0.5 * voxel);
        if moved || self.mask_path.is_empty() {
            self.mask_path.push(point);
        }
        if !drag.finished {
            return;
        }
        let points = std::mem::take(&mut self.mask_path);
        if points.is_empty() {
            return;
        }
        let Some(anchor) = self.mask_anchor(brush.set, points[0]) else {
            self.set_output_message("paint inside an object of the object set");
            return;
        };
        let (tool, mode) = match brush.tool {
            grana_editor::MaskBrushTool::Brush => (MaskTool::Brush, MaskMode::Add),
            grana_editor::MaskBrushTool::Erase => (MaskTool::Brush, MaskMode::Remove),
            grana_editor::MaskBrushTool::Wand => (MaskTool::Wand, MaskMode::Add),
        };
        let points = match tool {
            MaskTool::Brush => points,
            MaskTool::Wand => vec![point],
        };
        let time = scene.time();
        self.apply(vec![Command::PaintMask {
            set: brush.set,
            frame: time,
            anchor,
            tool,
            mode,
            radius: brush.radius,
            tolerance: brush.tolerance,
            points,
        }]);
        self.retarget_mask();
    }

    pub(super) fn mask_anchor(&self, set: ObjectSetId, point: [f32; 3]) -> Option<ObjectAnchor> {
        let scene = self.session.scene()?;
        let source = scene
            .objects
            .get(set)
            .and_then(|object| object.provenance().source())
            .unwrap_or(set);
        let object = scene.objects.get(source)?;
        let ObjectFrame::Surfaces(surfaces) = object.frame(scene.time())? else {
            return None;
        };
        let anchors = surfaces
            .objects()
            .iter()
            .map(|statistics| ObjectAnchor::new(statistics.centroid(), statistics.bounds()))
            .collect::<Vec<_>>();
        let voxel = scene.dataset.voxel_size().min_element();
        grana_session::match_anchor(&anchors, point, voxel).map(|index| anchors[index])
    }

    pub(super) fn retarget_mask(&mut self) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        if scene
            .objects
            .get(brush.set)
            .is_some_and(|object| object.provenance().masking().is_some())
        {
            return;
        }
        let Some(masked) = scene.objects.iter().find_map(|object| {
            object
                .provenance()
                .masking()
                .and_then(|(source, _)| (source == brush.set).then_some(object.id()))
        }) else {
            return;
        };
        self.workspace.mask = Some(MaskBrush {
            set: masked,
            ..brush
        });
    }

    pub(super) fn place_seed(&mut self, set: ObjectSetId, point: Vec2, remove: bool) {
        if self
            .session
            .scene()
            .is_none_or(|scene| scene.seed_source(set).is_some())
        {
            return;
        }
        let Some((origin, direction)) = self.volume_ray(point) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let channel = scene.objects.get(set).map_or(0, |object| object.channel());
        let position = match scene.surface_ray(set, origin, direction) {
            Ok(Some(hit)) => hit.to_array(),
            Ok(None) => match self.session.seed_peak(channel, origin, direction) {
                Some(peak) => peak,
                None => match self.annotation_point(point) {
                    Some(position) => position,
                    None => return,
                },
            },
            Err(detail) => {
                self.output = Some(detail);
                return;
            }
        };
        let voxel = scene.dataset.voxel_size().min_element();
        let radius = 3.0 * voxel;
        if remove {
            let mut nearest: Option<(f32, usize)> = None;
            for (index, seed) in scene.seeds(set).iter().enumerate() {
                let distance = (Vec3::from(*seed) - Vec3::from(position)).length();
                if distance <= radius && nearest.is_none_or(|(best, _)| distance < best) {
                    nearest = Some((distance, index));
                }
            }
            if let Some((_, index)) = nearest {
                self.apply(vec![Command::RemoveSeed { set, index }]);
            }
            return;
        }
        self.apply(vec![Command::AddSeed { set, position }]);
    }

    pub(super) fn volume_ray(&self, point: Vec2) -> Option<(Vec3, Vec3)> {
        let surface = self.surface.as_ref()?;
        let region = self.stage.region();
        if region[2] == 0 || region[3] == 0 {
            return None;
        }
        let scale = surface.window.scale_factor() as f32;
        let x = point.x * scale;
        let y = point.y * scale;
        let ndc = Vec2::new(
            2.0 * (x - region[0] as f32) / region[2] as f32 - 1.0,
            1.0 - 2.0 * (y - region[1] as f32) / region[3] as f32,
        );
        let aspect = region[2] as f32 / region[3] as f32;
        let ray = self.camera.ray(ndc, aspect);
        Some((ray.origin, ray.direction))
    }

    pub(super) fn annotation_point(&self, point: Vec2) -> Option<[f32; 3]> {
        let scene = self.session.scene()?;
        let (origin, direction) = self.volume_ray(point)?;
        if let Ok(Some(hit)) = scene.surface_hit(origin, direction) {
            return Some(hit.to_array());
        }
        let half = 0.5 * scene.dataset.world_size();
        let clip = scene.clip;
        let (normal, offset) = if clip.enabled() {
            (clip.normal(), clip.offset())
        } else {
            let normal = self.camera.forward();
            (normal, normal.dot(self.camera.pivot()))
        };
        let component = direction.dot(normal);
        if component.abs() <= 1.0e-6 {
            return None;
        }
        let distance = (offset - normal.dot(origin)) / component;
        if distance <= 0.0 {
            return None;
        }
        let position = (origin + direction * distance).clamp(-half, half);
        Some(position.to_array())
    }

    pub(super) fn place_annotation(&mut self, tool: AnnotationTool, point: Vec2) {
        let Some(position) = self.annotation_point(point) else {
            return;
        };
        match tool {
            AnnotationTool::Point => {
                self.apply(vec![Command::AddPoint { at: position }]);
            }
            AnnotationTool::Distance => match self.workspace.annotation_pending.take() {
                Some(from) if from != position => {
                    self.apply(vec![Command::AddDistance { from, to: position }]);
                }
                Some(from) => self.workspace.annotation_pending = Some(from),
                None => self.workspace.annotation_pending = Some(position),
            },
            AnnotationTool::Roi => {}
        }
    }

    pub(super) fn pick(&mut self, point: Vec2) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let point = [
            (point.x * scale).max(0.0).round() as u32,
            (point.y * scale).max(0.0).round() as u32,
        ];
        engine.request_pick(&surface.gpu.device, &surface.gpu.queue, point);
    }

    pub(super) fn poll_pick(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if !engine.pick_pending() {
            return;
        }
        let Some(picked) = engine.poll_pick(&surface.gpu.device) else {
            return;
        };
        let selection = match picked {
            Picked::Hit(selection) => Some(selection),
            Picked::Seed => return,
            Picked::Miss => None,
        };
        match (selection, std::mem::take(&mut self.pick_toggle)) {
            (Some(selection), true) => {
                self.apply(vec![Command::ToggleSelect(selection)]);
            }
            (selection, _) => {
                self.apply(vec![Command::Select(selection)]);
            }
        }
    }
}

fn annotation_hit(
    surface: &Surface,
    session: &Session,
    camera: &Camera,
    stage: &Stage,
) -> Option<(usize, grana_figure::AnnotationPart)> {
    let pointer = surface.ui.input().position();
    if !stage.contains(pointer) {
        return None;
    }
    let scene = session.scene()?;
    let items = grana_figure::annotations(scene, scene.time());
    if items.is_empty() {
        return None;
    }
    let rect = stage.viewport();
    if rect.height() <= 0.0 {
        return None;
    }
    let world = scene.dataset.world_size();
    let aspect = rect.width() / rect.height();
    let view_projection = camera.view_projection(aspect, 0.5 * world.length());
    let placed = grana_figure::volume_annotations(&items, view_projection, rect);
    grana_figure::pick_annotation(&placed, pointer)
}
