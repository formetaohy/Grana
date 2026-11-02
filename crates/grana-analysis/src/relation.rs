use glam::Vec3;
use grana_geometry::{Bvh, PackedMesh, Primitive};
use grana_object::RelationFrame;

use crate::shape::Shape;

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

    pub fn relate(&self, source: &Shape) -> RelationFrame {
        if self.is_empty() {
            return RelationFrame::empty(sources(source));
        }
        match source {
            Shape::Spots(spots) => {
                let mut distance = Vec::with_capacity(spots.len());
                let mut inside = Vec::with_capacity(spots.len());
                let mut partner = Vec::with_capacity(spots.len());
                for spot in spots.iter() {
                    let (reach, object, contained) = self.measure(Vec3::from(spot.center()));
                    distance.push(reach);
                    inside.push(if contained { 1.0 } else { 0.0 });
                    partner.push(object + 1);
                }
                RelationFrame::new(distance, inside, partner)
            }
            Shape::Mesh(mesh) => {
                let mut distance = Vec::with_capacity(mesh.object_count());
                let mut inside = Vec::with_capacity(mesh.object_count());
                let mut partner = Vec::with_capacity(mesh.object_count());
                for object in 0..mesh.object_count() {
                    let vertices = mesh.groups()[object].vertex_count() as usize;
                    let mut reach = f32::INFINITY;
                    let mut nearest = 0;
                    let mut contained = 0usize;
                    for vertex in 0..vertices {
                        let point = Vec3::from(mesh.position(object, vertex));
                        let (value, target, within) = self.measure(point);
                        if within {
                            contained += 1;
                        }
                        if value < reach {
                            reach = value;
                            nearest = target;
                        }
                    }
                    distance.push(reach);
                    inside.push(contained as f32 / vertices as f32);
                    partner.push(nearest + 1);
                }
                RelationFrame::new(distance, inside, partner)
            }
        }
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

fn sources(shape: &Shape) -> usize {
    match shape {
        Shape::Mesh(mesh) => mesh.object_count(),
        Shape::Spots(spots) => spots.len(),
    }
}
