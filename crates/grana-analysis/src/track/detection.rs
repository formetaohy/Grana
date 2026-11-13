use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_object::TrackDetection;

pub(super) struct Detection {
    centroid: Vec3,
    radii: [f32; 3],
    stretch: [f32; 3],
    frame: u32,
    object: usize,
}

pub(super) struct Detections {
    detections: Vec<Detection>,
    spans: Vec<Range<usize>>,
    stretch: f32,
}

impl Detections {
    pub fn new(frames: &[&[TrackDetection]]) -> Self {
        let mut detections = Vec::new();
        let mut spans = Vec::with_capacity(frames.len());
        let mut widest = 1.0f32;
        for (frame, frame_detections) in frames.iter().enumerate() {
            let start = detections.len();
            detections.extend(
                frame_detections
                    .iter()
                    .enumerate()
                    .map(|(object, detection)| {
                        let radii = detection.radii();
                        let shape = stretch(radii);
                        widest = widest.max(shape.into_iter().fold(1.0, f32::max));
                        Detection {
                            centroid: Vec3::from(detection.centroid()),
                            radii,
                            stretch: shape,
                            frame: frame as u32,
                            object,
                        }
                    }),
            );
            spans.push(start..detections.len());
        }
        Self {
            detections,
            spans,
            stretch: widest,
        }
    }

    pub fn len(&self) -> usize {
        self.detections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.detections.is_empty()
    }

    pub fn frame_count(&self) -> usize {
        self.spans.len()
    }

    pub fn span(&self, frame: usize) -> Range<usize> {
        self.spans[frame].clone()
    }

    pub fn frame(&self, index: usize) -> u32 {
        self.detection(index).frame
    }

    pub fn object(&self, index: usize) -> usize {
        self.detection(index).object
    }

    pub fn centroid(&self, index: usize) -> Vec3 {
        self.detection(index).centroid
    }

    pub fn radius(&self, index: usize) -> f32 {
        let radii = self.detection(index).radii;
        (radii[0] * radii[1] * radii[2]).cbrt()
    }

    pub fn gate(&self, index: usize, reach: f32) -> Vec3 {
        Vec3::from(self.detection(index).stretch) * reach
    }

    pub fn grid(&self, frame: usize, reach: f32) -> Grid<'_> {
        Grid::new(self, frame, reach * self.stretch)
    }

    fn detection(&self, index: usize) -> &Detection {
        &self.detections[index]
    }
}

fn stretch(radii: [f32; 3]) -> [f32; 3] {
    let smallest = radii.into_iter().fold(f32::INFINITY, f32::min);
    radii.map(|radius| radius / smallest)
}

pub(super) struct Grid<'a> {
    detections: &'a Detections,
    cell: f32,
    cells: HashMap<[i64; 3], Vec<usize>>,
}

impl<'a> Grid<'a> {
    fn new(detections: &'a Detections, frame: usize, cell: f32) -> Self {
        let mut cells: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for index in detections.span(frame) {
            cells
                .entry(key(detections.centroid(index), cell))
                .or_default()
                .push(index);
        }
        Self {
            detections,
            cell,
            cells,
        }
    }

    pub fn near(&self, at: Vec3, gate: Vec3, mut visit: impl FnMut(usize, f32)) {
        let gate = gate.to_array();
        let base = key(at, self.cell);
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    let Some(indices) = self.cells.get(&[base[0] + x, base[1] + y, base[2] + z])
                    else {
                        continue;
                    };
                    for &index in indices {
                        let offset = at - self.detections.centroid(index);
                        let distance = [offset.x, offset.y, offset.z]
                            .into_iter()
                            .zip(gate)
                            .map(|(offset, gate)| (offset / gate).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        if distance <= 1.0 {
                            visit(index, distance);
                        }
                    }
                }
            }
        }
    }
}

fn key(position: Vec3, cell: f32) -> [i64; 3] {
    [
        (position.x / cell).floor() as i64,
        (position.y / cell).floor() as i64,
        (position.z / cell).floor() as i64,
    ]
}
