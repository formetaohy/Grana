use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_geometry::{Bvh, PackedMesh, Primitive};
use grana_object::{RelationFrame, RelationPair, Shape};

pub struct Reference {
    bvh: Bvh,
    centers: Vec<Vec3>,
}

#[derive(Default)]
struct Touch {
    contact: f32,
    distance: f32,
    inside: usize,
}

impl Touch {
    fn new(reach: f32) -> Self {
        Self {
            contact: 0.0,
            distance: reach,
            inside: 0,
        }
    }
}

impl Reference {
    pub fn new(shape: &Shape) -> Self {
        let centers = match shape {
            Shape::Spots(spots) => spots.iter().map(|spot| Vec3::from(spot.center())).collect(),
            Shape::Mesh(mesh) => (0..mesh.object_count())
                .map(|object| Self::centroid(mesh, object))
                .collect(),
        };
        let bvh = match shape {
            Shape::Mesh(mesh) => Self::mesh(mesh),
            Shape::Spots(spots) => Bvh::new(spots.iter().enumerate().map(|(index, spot)| {
                (
                    index as u32,
                    Primitive::sphere(Vec3::from(spot.center()), spot.radius()),
                )
            })),
        };
        Self { bvh, centers }
    }

    fn centroid(mesh: &PackedMesh, object: usize) -> Vec3 {
        let vertices = mesh.groups()[object].vertex_count() as usize;
        if vertices == 0 {
            return Vec3::ZERO;
        }
        let sum = (0..vertices).fold(Vec3::ZERO, |sum, vertex| {
            sum + Vec3::from(mesh.position(object, vertex))
        });
        sum / vertices as f32
    }

    fn mesh(mesh: &PackedMesh) -> Bvh {
        let mut primitives = Vec::with_capacity(mesh.triangle_count());
        for object in 0..mesh.object_count() {
            let triangles = mesh.groups()[object].index_count() as usize / 3;
            for triangle in 0..triangles {
                let points = mesh.triangle(object, triangle).map(Vec3::from);
                primitives.push((object as u32, Primitive::triangle(points)));
            }
        }
        Bvh::new(primitives)
    }

    pub fn is_empty(&self) -> bool {
        self.bvh.is_empty()
    }

    pub fn relate(&self, source: &Shape, gate: f32, same: bool) -> RelationFrame {
        assert!(
            gate.is_finite() && gate >= 0.0,
            "the contact gate of a relation is a length"
        );
        if self.is_empty() {
            return RelationFrame::empty(sources(source));
        }
        match source {
            Shape::Spots(spots) => self.relate_spots(spots, gate, same),
            Shape::Mesh(mesh) => self.relate_mesh(mesh, gate, same),
        }
    }

    fn relate_spots(&self, spots: &[grana_object::Spot], gate: f32, same: bool) -> RelationFrame {
        let mut distance = Vec::with_capacity(spots.len());
        let mut center = Vec::with_capacity(spots.len());
        let mut inside = Vec::with_capacity(spots.len());
        let mut contact = Vec::with_capacity(spots.len());
        let mut overlapping = Vec::with_capacity(spots.len());
        let mut partner = Vec::with_capacity(spots.len());
        let mut pairs = Vec::with_capacity(spots.len());
        for (index, spot) in spots.iter().enumerate() {
            let point = Vec3::from(spot.center());
            let skipped = same.then_some(index as u32);
            let (reach, object, contained) = self.measure(point, skipped);
            let measured = reach.is_finite();
            let touched = if contained {
                0.0
            } else {
                cap(spot.radius(), reach, gate)
            };
            distance.push(reach);
            center.push(if measured {
                point.distance(self.centers[object as usize])
            } else {
                f32::INFINITY
            });
            inside.push(if contained { 1.0 } else { 0.0 });
            contact.push(touched);
            overlapping.push(u32::from(contained));
            partner.push(if measured { object + 1 } else { 0 });
            if measured {
                pairs.push(RelationPair::new(
                    index as u32,
                    object,
                    touched,
                    reach,
                    if contained { 1.0 } else { 0.0 },
                    point.distance(self.centers[object as usize]),
                ));
            }
        }
        RelationFrame::new(
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
            pairs,
        )
    }

    fn relate_mesh(&self, mesh: &PackedMesh, gate: f32, same: bool) -> RelationFrame {
        let mut distance = Vec::with_capacity(mesh.object_count());
        let mut center = Vec::with_capacity(mesh.object_count());
        let mut inside = Vec::with_capacity(mesh.object_count());
        let mut contact = Vec::with_capacity(mesh.object_count());
        let mut overlapping = Vec::with_capacity(mesh.object_count());
        let mut partner = Vec::with_capacity(mesh.object_count());
        let mut pairs = Vec::new();
        let mut votes: HashMap<u32, u32> = HashMap::new();
        let mut touches: HashMap<u32, Touch> = HashMap::new();
        let mut reaches: Vec<f32> = Vec::new();
        let mut targets: Vec<u32> = Vec::new();
        let mut contained: Vec<bool> = Vec::new();
        for object in 0..mesh.object_count() {
            let group = mesh.groups()[object];
            let vertices = group.vertex_count() as usize;
            let triangles = group.index_count() as usize / 3;
            reaches.clear();
            targets.clear();
            contained.clear();
            let skipped = same.then_some(object as u32);
            let mut sum = Vec3::ZERO;
            let mut reach = f32::INFINITY;
            let mut nearest: Option<u32> = None;
            let mut within = 0usize;
            votes.clear();
            touches.clear();
            for vertex in 0..vertices {
                let point = Vec3::from(mesh.position(object, vertex));
                sum += point;
                let (value, target, holds) = self.measure(point, skipped);
                reaches.push(value);
                targets.push(target);
                contained.push(holds);
                if holds {
                    within += 1;
                    *votes.entry(target).or_insert(0) += 1;
                }
                if value.is_finite() {
                    let touch = touches.entry(target).or_insert_with(|| Touch::new(value));
                    touch.distance = touch.distance.min(value);
                    touch.inside += usize::from(holds);
                }
                if value < reach {
                    reach = value;
                    nearest = Some(target);
                }
            }
            let mut area = 0.0f64;
            for triangle in 0..triangles {
                let corners = mesh.triangle_vertices(object, triangle);
                if corners
                    .iter()
                    .any(|corner| contained[*corner as usize] || reaches[*corner as usize] > gate)
                {
                    continue;
                }
                let owner = corners.iter().copied().min_by(|left, right| {
                    reaches[*left as usize].total_cmp(&reaches[*right as usize])
                });
                let Some(owner) = owner else {
                    continue;
                };
                let touched = area_of(mesh.triangle(object, triangle));
                area += touched;
                if let Some(touch) = touches.get_mut(&targets[owner as usize]) {
                    touch.contact += touched as f32;
                }
            }
            let containing = votes
                .iter()
                .max_by(|left, right| left.1.cmp(right.1).then(right.0.cmp(left.0)))
                .map(|(object, _)| *object);
            let chosen = containing.or(nearest);
            let centroid = if vertices == 0 {
                Vec3::ZERO
            } else {
                sum / vertices as f32
            };
            distance.push(reach);
            center.push(chosen.map_or(f32::INFINITY, |target| {
                centroid.distance(self.centers[target as usize])
            }));
            inside.push(within as f32 / vertices.max(1) as f32);
            contact.push(area as f32);
            overlapping.push(votes.len() as u32);
            partner.push(chosen.map_or(0, |target| target + 1));
            let start = pairs.len();
            for (target, touch) in touches.drain() {
                pairs.push(RelationPair::new(
                    object as u32,
                    target,
                    touch.contact,
                    touch.distance,
                    touch.inside as f32 / vertices.max(1) as f32,
                    centroid.distance(self.centers[target as usize]),
                ));
            }
            pairs[start..].sort_by_key(|pair| pair.target());
        }
        RelationFrame::new(
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
            pairs,
        )
    }

    fn measure(&self, point: Vec3, skipped: Option<u32>) -> (f32, u32, bool) {
        let contained = match skipped {
            Some(object) => self.bvh.contains_other(point, object),
            None => self.bvh.contains(point),
        };
        if let Some(object) = contained {
            return (0.0, object, true);
        }
        let nearest = match skipped {
            Some(object) => self.bvh.nearest_other(point, object),
            None => self.bvh.nearest(point),
        };
        match nearest {
            Some((distance, object)) => (distance, object, false),
            None => (f32::INFINITY, 0, false),
        }
    }
}

fn area_of(points: [[f32; 3]; 3]) -> f64 {
    let first = Vec3::from(points[0]);
    let second = Vec3::from(points[1]);
    let third = Vec3::from(points[2]);
    0.5 * f64::from((second - first).cross(third - first).length())
}

fn cap(radius: f32, reach: f32, gate: f32) -> f32 {
    if !(radius.is_finite() && radius > 0.0) || reach < radius {
        return 0.0;
    }
    let height = (radius + gate - reach).clamp(0.0, 2.0 * radius);
    std::f32::consts::TAU * radius * height
}

fn sources(shape: &Shape) -> usize {
    shape.objects()
}

pub struct RelationRun<'a> {
    pub shift: i32,
    pub gate: f32,
    pub same: bool,
    pub workers: usize,
    pub cancel: &'a AtomicBool,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

pub fn relate_frames(
    counts: &[Option<usize>],
    source: impl Fn(usize) -> Result<Option<Shape>, String> + Sync,
    target: impl Fn(usize) -> Result<Option<Shape>, String> + Sync,
    run: RelationRun<'_>,
) -> Result<Option<Vec<Option<RelationFrame>>>, String> {
    let RelationRun {
        shift,
        gate,
        same,
        workers,
        cancel,
        progress,
    } = run;
    let total = counts.len();
    let mut frames = (0..total).map(|_| None).collect::<Vec<_>>();
    let completed = crate::schedule::ordered(
        crate::schedule::Run {
            total,
            workers: crate::schedule::workers(workers),
            cancel,
            progress,
        },
        || (),
        |(), time| {
            let Some(index) = shifted(time, shift, total) else {
                return Ok(None);
            };
            let Some(place) = target(index)? else {
                return Ok(counts[time].map(RelationFrame::empty));
            };
            let shape = source(time)?;
            let reference = Reference::new(&place);
            drop(place);
            let same = same && shift == 0;
            Ok(shape.map(|shape| reference.relate(&shape, gate, same)))
        },
        |time, frame| {
            frames[time] = frame;
            Ok(())
        },
        |()| {},
    )?;
    Ok(completed.then_some(frames))
}

fn shifted(time: usize, shift: i32, total: usize) -> Option<usize> {
    let index = time as i64 + i64::from(shift);
    (index >= 0 && index < total as i64).then_some(index as usize)
}
