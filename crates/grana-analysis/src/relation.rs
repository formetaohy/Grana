use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_geometry::{Bvh, PackedMesh, Primitive};
use grana_object::RelationFrame;

use grana_object::Shape;

pub struct Reference {
    bvh: Bvh,
}

impl Reference {
    pub fn new(shape: &Shape) -> Self {
        let bvh = match shape {
            Shape::Mesh(mesh) => Self::mesh(mesh),
            Shape::Spots(spots) => Bvh::new(spots.iter().enumerate().map(|(index, spot)| {
                (
                    index as u32,
                    Primitive::sphere(Vec3::from(spot.center()), spot.radius()),
                )
            })),
        };
        Self { bvh }
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

    pub fn relate(&self, source: &Shape, gate: f32) -> RelationFrame {
        assert!(
            gate.is_finite() && gate >= 0.0,
            "the contact gate of a relation is a length"
        );
        if self.is_empty() {
            return RelationFrame::empty(sources(source));
        }
        match source {
            Shape::Spots(spots) => self.relate_spots(spots, gate),
            Shape::Mesh(mesh) => self.relate_mesh(mesh, gate),
        }
    }

    fn relate_spots(&self, spots: &[grana_object::Spot], gate: f32) -> RelationFrame {
        let mut distance = Vec::with_capacity(spots.len());
        let mut inside = Vec::with_capacity(spots.len());
        let mut contact = Vec::with_capacity(spots.len());
        let mut partner = Vec::with_capacity(spots.len());
        for spot in spots {
            let (reach, object, contained) = self.measure(Vec3::from(spot.center()));
            distance.push(reach);
            inside.push(if contained { 1.0 } else { 0.0 });
            contact.push(if contained {
                0.0
            } else {
                cap(spot.radius(), reach, gate)
            });
            partner.push(object + 1);
        }
        RelationFrame::new(distance, inside, contact, partner)
    }

    fn relate_mesh(&self, mesh: &PackedMesh, gate: f32) -> RelationFrame {
        let mut distance = Vec::with_capacity(mesh.object_count());
        let mut inside = Vec::with_capacity(mesh.object_count());
        let mut contact = Vec::with_capacity(mesh.object_count());
        let mut partner = Vec::with_capacity(mesh.object_count());
        let mut votes: HashMap<u32, u32> = HashMap::new();
        let mut reaches: Vec<f32> = Vec::new();
        let mut contained: Vec<bool> = Vec::new();
        for object in 0..mesh.object_count() {
            let group = mesh.groups()[object];
            let vertices = group.vertex_count() as usize;
            let triangles = group.index_count() as usize / 3;
            reaches.clear();
            contained.clear();
            let mut reach = f32::INFINITY;
            let mut nearest = 0;
            let mut within = 0usize;
            votes.clear();
            for vertex in 0..vertices {
                let point = Vec3::from(mesh.position(object, vertex));
                let (value, target, holds) = self.measure(point);
                reaches.push(value);
                contained.push(holds);
                if holds {
                    within += 1;
                    *votes.entry(target).or_insert(0) += 1;
                }
                if value < reach {
                    reach = value;
                    nearest = target;
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
                area += area_of(mesh.triangle(object, triangle));
            }
            let containing = votes
                .iter()
                .max_by(|left, right| left.1.cmp(right.1).then(right.0.cmp(left.0)))
                .map(|(object, _)| *object);
            distance.push(reach);
            inside.push(within as f32 / vertices as f32);
            contact.push(area as f32);
            partner.push(containing.unwrap_or(nearest) + 1);
        }
        RelationFrame::new(distance, inside, contact, partner)
    }

    fn measure(&self, point: Vec3) -> (f32, u32, bool) {
        if let Some(object) = self.bvh.contains(point) {
            return (0.0, object, true);
        }
        let (distance, object) = self
            .bvh
            .nearest(point)
            .expect("a reference holds primitives");
        (distance, object, false)
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
            Ok(shape.map(|shape| reference.relate(&shape, gate)))
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
