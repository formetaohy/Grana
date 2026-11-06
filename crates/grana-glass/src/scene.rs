use glam::Vec2;
use slotmap::{SlotMap, new_key_type};

use crate::material::{Appearance, Material};
use crate::motion::{Animated, Spring};

new_key_type! {
    pub struct GlassId;

    pub struct ContainerId;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn centered(center: Vec2, size: Vec2) -> Self {
        Self::new(
            center.x - size.x * 0.5,
            center.y - size.y * 0.5,
            size.x,
            size.y,
        )
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x + self.width
            && point.y < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Capsule,
    Rounded(f32),
    Circular(f32),
    Smooth { radius: f32, smoothing: f32 },
}

pub const IOS_SMOOTHING: f32 = 0.6;

pub fn smooth_corner(radius: f32, smoothing: f32, budget: f32) -> (f32, f32) {
    let r = radius.min(budget).max(0.0);
    if r <= 0.0 {
        return (0.0, 2.0);
    }
    let s = smoothing.clamp(0.0, 1.0).min((budget / r - 1.0).max(0.0));
    (
        r * (1.0 + 0.42 * s + 0.18 * s * s),
        2.0 + 0.95 * s + 0.6 * s * s,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Container {
    pub spacing: f32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub frame: Rect,
    pub shape: Shape,
    pub material: Material,
    pub interactive: bool,
    pub lens: bool,
    pub container: Option<ContainerId>,
    pub z: i32,
    pub clip: Option<Rect>,
}

impl Glass {
    pub fn new(frame: Rect) -> Self {
        Self {
            frame,
            shape: Shape::Capsule,
            material: Material::regular(),
            interactive: false,
            lens: false,
            container: None,
            z: 0,
            clip: None,
        }
    }

    pub fn shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }

    pub fn material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }

    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }

    pub fn lens(mut self) -> Self {
        self.lens = true;
        self
    }

    pub fn container(mut self, container: ContainerId) -> Self {
        self.container = Some(container);
        self
    }

    pub fn z(mut self, z: i32) -> Self {
        self.z = z;
        self
    }

    pub fn clip(mut self, clip: Rect) -> Self {
        self.clip = Some(clip);
        self
    }
}

pub(crate) struct Node {
    pub glass: Glass,
    pub x: Animated,
    pub y: Animated,
    pub w: Animated,
    pub h: Animated,
    pub presence: Animated,
    pub press: Animated,
    pub lift: Animated,
    pub stretch: Animated,
    pub stretch_dir: Vec2,
    pub prev_center: Vec2,
    pub velocity: Vec2,
    pub from_shape: Shape,
    pub morph: Animated,
    pub from_material: Material,
    pub material_mix: Animated,
    pub removing: bool,
    pub order: u64,
}

impl Node {
    fn is_gone(&self) -> bool {
        let faded = self.presence.target <= 0.0 && self.presence.value < 0.01;
        let near = |a: &Animated, tolerance: f32| {
            (a.value - a.target).abs() < tolerance && a.velocity.abs() < tolerance * 10.0
        };
        let merged = [&self.x, &self.y, &self.w, &self.h]
            .iter()
            .all(|a| near(a, 0.1))
            && near(&self.morph, 0.005)
            && near(&self.presence, 0.01);
        faded || merged
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    pub center: Vec2,
    pub half: Vec2,
    pub radius: f32,
    pub exponent: f32,
    pub axis: Vec2,
    pub stretch: f32,
}

impl Geometry {
    pub fn reach(&self) -> Vec2 {
        self.half * (1.0 + self.stretch)
    }

    fn contains(&self, p: Vec2) -> bool {
        let q = (p - self.center).abs() - self.half + Vec2::splat(self.radius);
        let c = q.max(Vec2::ZERO);
        let n = self.exponent;
        (c.x.powf(n) + c.y.powf(n)).powf(1.0 / n) + q.x.max(q.y).min(0.0) - self.radius <= 0.0
    }
}

pub(crate) struct Touch {
    pub pos: Vec2,
    pub intensity: Animated,
    pub radius: Animated,
}

struct Drag {
    id: GlassId,
    grab: Vec2,
    pointer: Vec2,
    moved: Vec2,
    velocity: Vec2,
}

const MATERIALIZE_IN: Spring = Spring::new(0.4, 0.0);
const MATERIALIZE_OUT: Spring = Spring::new(0.3, 0.0);
const PRESS_DOWN: Spring = Spring::new(0.25, 0.0);
const PRESS_RELEASE: Spring = Spring::new(0.5, 0.35);
const LIFT: Spring = Spring::new(0.3, 0.15);
const JELLY: Spring = Spring::new(0.35, 0.4);
const GLOW_IN: Spring = Spring::new(0.2, 0.0);
const GLOW_SPREAD: Spring = Spring::new(0.45, 0.0);
const GLOW_OUT: Spring = Spring::new(0.6, 0.0);

const STRETCH_SPEED: f32 = 2500.0;
const MAX_STRETCH: f32 = 0.25;
const LENS_LIFT: f32 = 0.3;

pub struct Scene {
    pub(crate) nodes: SlotMap<GlassId, Node>,
    pub(crate) containers: SlotMap<ContainerId, Container>,
    pub appearance: Appearance,
    pub(crate) touch: Touch,
    pressed: Option<GlassId>,
    drag: Option<Drag>,
    next_order: u64,
    dirty: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            containers: SlotMap::with_key(),
            appearance: Appearance::default(),
            touch: Touch {
                pos: Vec2::ZERO,
                intensity: Animated::new(0.0, GLOW_OUT),
                radius: Animated::new(0.0, GLOW_SPREAD),
            },
            pressed: None,
            drag: None,
            next_order: 0,
            dirty: true,
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn has_layers(&self, layers: impl std::ops::RangeBounds<i32>) -> bool {
        self.nodes.iter().any(|(_, node)| {
            !node.glass.material.is_identity()
                && (node.presence.value > 1e-3 || node.presence.target > 1e-3)
                && layers.contains(&self.z_of(node))
        })
    }

    pub fn add_container(&mut self, spacing: f32, z: i32) -> ContainerId {
        self.dirty = true;
        self.containers.insert(Container { spacing, z })
    }

    pub fn set_container_spacing(&mut self, id: ContainerId, spacing: f32) {
        if let Some(container) = self.containers.get_mut(id)
            && container.spacing != spacing.max(0.0)
        {
            container.spacing = spacing.max(0.0);
            self.dirty = true;
        }
    }

    pub fn add(&mut self, glass: Glass) -> GlassId {
        self.dirty = true;
        let at = |v| Animated::new(v, Spring::SNAPPY);
        let mut presence = Animated::new(0.0, MATERIALIZE_IN);
        presence.animate_to(1.0, MATERIALIZE_IN);
        let f = glass.frame;
        self.next_order += 1;
        self.nodes.insert(Node {
            glass,
            x: at(f.x),
            y: at(f.y),
            w: at(f.width),
            h: at(f.height),
            presence,
            press: at(0.0),
            lift: at(0.0),
            stretch: at(0.0),
            stretch_dir: Vec2::X,
            prev_center: f.center(),
            velocity: Vec2::ZERO,
            from_shape: glass.shape,
            morph: at(1.0),
            from_material: glass.material,
            material_mix: at(1.0),
            removing: false,
            order: self.next_order,
        })
    }

    pub fn remove(&mut self, id: GlassId) {
        self.dirty = true;
        let spring = MATERIALIZE_OUT;
        if let Some(node) = self.nodes.get_mut(id) {
            node.removing = true;
            node.presence.animate_to(0.0, spring);
        }
    }

    pub fn glass(&self, id: GlassId) -> Option<&Glass> {
        self.nodes.get(id).map(|n| &n.glass)
    }

    pub fn current_frame(&self, id: GlassId) -> Option<Rect> {
        self.nodes
            .get(id)
            .map(|n| Rect::new(n.x.value, n.y.value, n.w.value, n.h.value))
    }

    pub fn current_corner(&self, id: GlassId) -> Option<(f32, f32)> {
        self.nodes.get(id).map(|node| {
            let geometry = self.geometry(node);
            (geometry.radius, geometry.exponent)
        })
    }

    pub fn expand_from(&mut self, source: GlassId, mut glass: Glass, spring: Spring) -> GlassId {
        self.dirty = true;
        let Some(start) = self.current_frame(source) else {
            return self.add(glass);
        };
        glass.container = glass.container.or(self.nodes[source].glass.container);
        let fused = self.fuses(source, glass.container);
        let from_shape = self.nodes[source].glass.shape;
        let target = glass.frame;
        let id = self.add(Glass {
            frame: start,
            ..glass
        });
        let morph = spring;
        let node = &mut self.nodes[id];
        if fused {
            node.presence.snap(1.0);
        }
        node.from_shape = from_shape;
        node.morph.snap(0.0);
        node.morph.animate_to(1.0, morph);
        self.set_frame_with(id, target, spring);
        id
    }

    pub fn collapse_into(&mut self, id: GlassId, target: GlassId, spring: Spring) {
        self.dirty = true;
        let Some((frame, shape)) = self.glass(target).map(|g| (g.frame, g.shape)) else {
            return self.remove(id);
        };
        self.set_frame_with(id, frame, spring);
        let morph = spring;
        if let Some(node) = self.nodes.get_mut(id) {
            if node.from_shape != shape {
                node.from_shape = shape;
                node.morph.snap(1.0);
            }
            node.morph.animate_to(0.0, morph);
        }
        let container = self.glass(id).and_then(|g| g.container);
        if self.fuses(target, container) {
            self.nodes[id].removing = true;
        } else {
            self.remove(id);
        }
    }

    fn fuses(&self, other: GlassId, container: Option<ContainerId>) -> bool {
        container.is_some_and(|c| {
            self.glass(other).is_some_and(|g| g.container == Some(c))
                && self.containers.get(c).is_some_and(|c| c.spacing > 0.0)
        })
    }

    pub fn set_frame(&mut self, id: GlassId, frame: Rect) {
        self.set_frame_with(id, frame, Spring::SNAPPY);
    }

    pub fn set_frame_with(&mut self, id: GlassId, frame: Rect, spring: Spring) {
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if node.glass.frame != frame {
            self.dirty = true;
        }
        node.glass.frame = frame;
        node.x.animate_to(frame.x, spring);
        node.y.animate_to(frame.y, spring);
        node.w.animate_to(frame.width, spring);
        node.h.animate_to(frame.height, spring);
    }

    pub fn place(&mut self, id: GlassId, frame: Rect) {
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if node.glass.frame != frame
            || node.x.value != frame.x
            || node.y.value != frame.y
            || node.w.value != frame.width
            || node.h.value != frame.height
        {
            self.dirty = true;
        }
        node.glass.frame = frame;
        node.x.snap(frame.x);
        node.y.snap(frame.y);
        node.w.snap(frame.width);
        node.h.snap(frame.height);
        if !node.glass.lens {
            node.prev_center = frame.center();
            node.velocity = Vec2::ZERO;
        }
    }

    pub fn snap_presence(&mut self, id: GlassId, presence: f32) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.presence.snap(presence);
            self.dirty = true;
        }
    }

    pub fn set_engaged(&mut self, id: GlassId, engaged: bool, pointer: Option<Vec2>) {
        let lift = LIFT;
        let release = PRESS_RELEASE;
        let lift_release = Spring::SNAPPY;
        let jelly = JELLY;
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if engaged {
            if node.glass.interactive && (node.press.target - 1.0).abs() > 1e-4 {
                node.press.animate_to(1.0, PRESS_DOWN);
                self.dirty = true;
            }
            if node.glass.lens && (node.lift.target - 1.0).abs() > 1e-4 {
                node.lift.animate_to(1.0, lift);
                self.dirty = true;
            }
            if let Some(pos) = pointer {
                let size = Vec2::new(node.w.target, node.h.target);
                if self.pressed != Some(id) {
                    self.pressed = Some(id);
                    self.touch.pos = pos;
                    self.touch.intensity.animate_to(1.0, GLOW_IN);
                    self.touch.radius.snap(0.0);
                    self.touch
                        .radius
                        .animate_to(size.max_element() * 1.2, GLOW_SPREAD);
                    self.dirty = true;
                } else if self.touch.pos != pos {
                    self.touch.pos = pos;
                    self.dirty = true;
                }
            }
        } else {
            if node.press.target > 0.0 {
                node.press.animate_to(0.0, release);
                self.dirty = true;
            }
            if node.lift.target > 0.0 {
                node.lift.animate_to(0.0, lift_release);
                self.dirty = true;
            }
            if node.stretch.target > 0.0 && node.velocity == Vec2::ZERO {
                node.stretch.animate_to(0.0, jelly);
                self.dirty = true;
            }
            if self.pressed == Some(id) {
                self.pressed = None;
                self.touch.intensity.animate_to(0.0, GLOW_OUT);
                self.dirty = true;
            }
        }
    }

    pub fn set_material(&mut self, id: GlassId, material: Material) {
        if let Some(node) = self.nodes.get_mut(id)
            && node.glass.material != material
        {
            node.glass.material = material;
            node.from_material = material;
            node.material_mix.snap(1.0);
            self.dirty = true;
        }
    }

    pub fn set_material_with(&mut self, id: GlassId, material: Material, spring: Spring) {
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if node.glass.material == material {
            return;
        }
        node.from_material = node
            .from_material
            .lerp(&node.glass.material, node.material_mix.value);
        node.glass.material = material;
        node.material_mix.snap(0.0);
        node.material_mix.animate_to(1.0, spring);
        self.dirty = true;
    }

    pub(crate) fn material_of(&self, node: &Node) -> Material {
        if node.material_mix.value >= 1.0 {
            node.glass.material
        } else {
            node.from_material
                .lerp(&node.glass.material, node.material_mix.value)
        }
    }

    pub fn set_shape(&mut self, id: GlassId, shape: Shape) {
        if let Some(node) = self.nodes.get_mut(id)
            && node.glass.shape != shape
        {
            node.glass.shape = shape;
            node.from_shape = shape;
            node.morph.snap(1.0);
            self.dirty = true;
        }
    }

    pub fn set_clip(&mut self, id: GlassId, clip: Option<Rect>) {
        if let Some(node) = self.nodes.get_mut(id)
            && node.glass.clip != clip
        {
            node.glass.clip = clip;
            self.dirty = true;
        }
    }

    pub fn set_behavior(&mut self, id: GlassId, interactive: bool, lens: bool) {
        if let Some(node) = self.nodes.get_mut(id)
            && (node.glass.interactive != interactive || node.glass.lens != lens)
        {
            node.glass.interactive = interactive;
            node.glass.lens = lens;
            self.dirty = true;
        }
    }

    pub fn translate(&mut self, id: GlassId, delta: Vec2) {
        if delta == Vec2::ZERO {
            return;
        }
        if let Some(node) = self.nodes.get_mut(id) {
            let frame = node.glass.frame;
            node.glass.frame = Rect::new(
                frame.x + delta.x,
                frame.y + delta.y,
                frame.width,
                frame.height,
            );
            node.x.value += delta.x;
            node.x.target += delta.x;
            node.y.value += delta.y;
            node.y.target += delta.y;
            node.prev_center += delta;
            self.dirty = true;
        }
    }

    pub fn hit_test(&self, p: Vec2) -> Option<GlassId> {
        self.nodes
            .iter()
            .filter(|(_, n)| !n.removing && !n.glass.material.is_identity())
            .filter(|(_, n)| n.glass.clip.is_none_or(|clip| clip.contains(p)))
            .filter(|(_, n)| self.geometry(n).contains(p))
            .max_by_key(|(_, n)| (self.z_of(n), n.order))
            .filter(|(_, n)| n.glass.interactive || n.glass.lens)
            .map(|(id, _)| id)
    }

    pub fn pointer_down(&mut self, p: Vec2) -> Option<GlassId> {
        self.dirty = true;
        let id = self.hit_test(p)?;
        let lift = LIFT;
        let node = &mut self.nodes[id];
        let size = Vec2::new(node.w.target, node.h.target);
        if node.glass.interactive {
            node.press.animate_to(1.0, PRESS_DOWN);
        }
        if node.glass.lens {
            node.lift.animate_to(1.0, lift);
            let center = Vec2::new(node.x.target, node.y.target) + size * 0.5;
            self.drag = Some(Drag {
                id,
                grab: center - p,
                pointer: p,
                moved: Vec2::ZERO,
                velocity: Vec2::ZERO,
            });
        }
        self.pressed = Some(id);
        self.touch.pos = p;
        self.touch.intensity.animate_to(1.0, GLOW_IN);
        self.touch.radius.snap(0.0);
        self.touch
            .radius
            .animate_to(size.max_element() * 1.2, GLOW_SPREAD);
        Some(id)
    }

    pub fn pointer_move(&mut self, p: Vec2) {
        self.dirty = true;
        if self.pressed.is_some() {
            self.touch.pos = p;
        }
        let Some(drag) = &mut self.drag else { return };
        drag.moved += p - drag.pointer;
        drag.pointer = p;
        let center = p + drag.grab;
        let id = drag.id;
        if let Some(node) = self.nodes.get_mut(id) {
            let size = Vec2::new(node.w.target, node.h.target);
            node.glass.frame = Rect::centered(center, size);
            node.x
                .animate_to(center.x - size.x * 0.5, Spring::INTERACTIVE);
            node.y
                .animate_to(center.y - size.y * 0.5, Spring::INTERACTIVE);
        }
    }

    pub fn pointer_up(&mut self) {
        self.dirty = true;
        let release = PRESS_RELEASE;
        let lift = Spring::SNAPPY;
        let jelly = JELLY;
        if let Some(node) = self.pressed.take().and_then(|id| self.nodes.get_mut(id)) {
            node.press.animate_to(0.0, release);
            node.lift.animate_to(0.0, lift);
            node.stretch.animate_to(0.0, jelly);
        }
        self.drag = None;
        self.touch.intensity.animate_to(0.0, GLOW_OUT);
    }

    pub fn update(&mut self, dt: f32) -> bool {
        let elapsed = dt.max(0.0);
        let dt = elapsed.min(1.0 / 30.0);
        let jelly = JELLY;
        let dragged = self.drag.as_ref().map(|d| d.id);

        self.update_drag_stretch(elapsed);

        let mut moving = false;
        for (id, node) in &mut self.nodes {
            if dragged != Some(id) && node.glass.lens {
                let center = Vec2::new(
                    node.x.value + node.w.value * 0.5,
                    node.y.value + node.h.value * 0.5,
                );
                if elapsed > 1e-4 {
                    let instant = (center - node.prev_center) / elapsed;
                    let spring_vel = Vec2::new(
                        node.x.velocity + node.w.velocity * 0.5,
                        node.y.velocity + node.h.velocity * 0.5,
                    );
                    let v = if instant.length_squared() > spring_vel.length_squared() {
                        instant
                    } else {
                        spring_vel
                    };
                    node.velocity = node.velocity.lerp(v, 0.5);
                    if node.velocity.length_squared() < 1.0 {
                        node.velocity = Vec2::ZERO;
                    }
                }
                node.prev_center = center;
                let speed = node.velocity.length();
                if speed > 1.0 {
                    node.stretch_dir = node.velocity / speed;
                }
                let target = (speed / STRETCH_SPEED).min(1.0) * MAX_STRETCH;
                if (node.stretch.target - target).abs() > 1e-3 {
                    node.stretch.animate_to(target, jelly);
                }
                moving |= node.velocity != Vec2::ZERO;
            }
            for a in [
                &mut node.x,
                &mut node.y,
                &mut node.w,
                &mut node.h,
                &mut node.presence,
                &mut node.press,
                &mut node.lift,
                &mut node.stretch,
                &mut node.morph,
                &mut node.material_mix,
            ] {
                moving |= a.step(dt);
            }
            if dragged == Some(id) || !node.glass.lens {
                node.prev_center = Vec2::new(
                    node.x.value + node.w.value * 0.5,
                    node.y.value + node.h.value * 0.5,
                );
            }
        }
        moving |= self.touch.intensity.step(dt);
        moving |= self.touch.radius.step(dt);
        self.nodes.retain(|_, n| !(n.removing && n.is_gone()));
        if moving {
            self.dirty = true;
        }
        moving || self.drag.as_ref().is_some_and(|d| d.velocity != Vec2::ZERO)
    }

    fn update_drag_stretch(&mut self, dt: f32) {
        let Some(drag) = &mut self.drag else { return };
        if dt > 0.0 {
            let v = drag.moved / dt;
            drag.moved = Vec2::ZERO;
            drag.velocity = drag.velocity.lerp(v, 0.5);
            if drag.velocity.length_squared() < 1.0 {
                drag.velocity = Vec2::ZERO;
            }
        }
        let Some(node) = self.nodes.get_mut(drag.id) else {
            return;
        };
        let speed = drag.velocity.length();
        if speed > 1.0 {
            node.stretch_dir = drag.velocity / speed;
        }
        let target = (speed / STRETCH_SPEED).min(1.0) * MAX_STRETCH;
        node.stretch.animate_to(target, JELLY);
    }

    pub(crate) fn z_of(&self, node: &Node) -> i32 {
        node.glass
            .container
            .and_then(|c| self.containers.get(c))
            .map_or(node.glass.z, |c| c.z)
    }

    pub(crate) fn geometry(&self, node: &Node) -> Geometry {
        let size = Vec2::new(node.w.value.max(0.0), node.h.value.max(0.0));
        let min_side = size.min_element();
        let grow = (10.0 / min_side.max(1.0)).min(0.15);
        let boost = node.press.value * grow + node.lift.value * LENS_LIFT;
        let scale = 1.0 + boost;
        let half = size * 0.5 + Vec2::splat(min_side * 0.5 * boost);
        let budget = half.min_element();
        let corner = |shape| match shape {
            Shape::Capsule => (budget, 2.0),
            Shape::Circular(r) => ((r * scale).min(budget), 2.0),
            Shape::Rounded(r) => smooth_corner(r * scale, IOS_SMOOTHING, budget),
            Shape::Smooth { radius, smoothing } => smooth_corner(radius * scale, smoothing, budget),
        };
        let (mut radius, mut exponent) = corner(node.glass.shape);
        let t = node.morph.value.clamp(0.0, 1.0);
        if t < 1.0 {
            let (r0, e0) = corner(node.from_shape);
            radius = r0 + (radius - r0) * t;
            exponent = e0 + (exponent - e0) * t;
        }
        Geometry {
            center: Vec2::new(node.x.value, node.y.value) + size * 0.5,
            half,
            radius,
            exponent,
            axis: node.stretch_dir,
            stretch: node.stretch.value.max(0.0),
        }
    }
}
