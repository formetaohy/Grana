use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_object::TrackDetection;

pub(super) struct Detection {
    centroid: Vec3,
    radii: [f32; 3],
    frame: u32,
    object: usize,
}

pub(super) struct Detections {
    detections: Vec<Detection>,
    spans: Vec<Range<usize>>,
    largest: f32,
}

impl Detections {
    pub fn new(frames: &[&[TrackDetection]]) -> Self {
        let mut detections = Vec::new();
        let mut spans = Vec::with_capacity(frames.len());
        let mut largest = 0.0f32;
        for (frame, frame_detections) in frames.iter().enumerate() {
            let start = detections.len();
            detections.extend(
                frame_detections
                    .iter()
                    .enumerate()
                    .map(|(object, detection)| {
                        let radii = detection.radii();
                        largest = radii.into_iter().fold(largest, f32::max);
                        Detection {
                            centroid: Vec3::from(detection.centroid()),
                            radii,
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
            largest: largest.max(f32::MIN_POSITIVE),
        }
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

    pub fn gate(&self, index: usize, reach: f32, step: f32) -> Vec3 {
        let radii = self.detection(index).radii;
        Vec3::new(
            reach * step.max(radii[0]),
            reach * step.max(radii[1]),
            reach * step.max(radii[2]),
        )
    }

    pub fn grid(&self, frame: usize, step: f32) -> Grid<'_> {
        Grid::new(self, frame, step.max(self.largest))
    }

    pub fn largest(&self) -> f32 {
        self.largest
    }

    fn detection(&self, index: usize) -> &Detection {
        &self.detections[index]
    }
}

fn key(position: Vec3, cell: f32) -> [i64; 3] {
    [
        (position.x / cell).floor() as i64,
        (position.y / cell).floor() as i64,
        (position.z / cell).floor() as i64,
    ]
}

pub(super) struct Grid<'a> {
    detections: &'a Detections,
    cell: f32,
    cells: HashMap<[i64; 3], Vec<usize>>,
}

impl<'a> Grid<'a> {
    fn new(detections: &'a Detections, frame: usize, cell: f32) -> Self {
        assert!(
            cell.is_finite() && cell > 0.0,
            "a detection index holds a positive cell"
        );
        let span = detections.span(frame);
        let mut cells: HashMap<[i64; 3], Vec<usize>> = HashMap::with_capacity(span.len());
        for index in span {
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
        let reach = [0, 1, 2].map(|axis| (gate[axis] / self.cell).ceil().max(0.0) as i64);
        let base = key(at, self.cell);
        for z in -reach[2]..=reach[2] {
            for y in -reach[1]..=reach[1] {
                for x in -reach[0]..=reach[0] {
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
