use std::collections::HashMap;
use std::ops::Range;

use glam::Vec3;
use grana_object::TrackDetection;

pub(super) struct Detection {
    centroid: Vec3,
    radius: f32,
    frame: u32,
    object: usize,
}

pub(super) struct Detections {
    detections: Vec<Detection>,
    spans: Vec<Range<usize>>,
}

impl Detections {
    pub fn new(frames: &[&[TrackDetection]]) -> Self {
        let mut detections = Vec::new();
        let mut spans = Vec::with_capacity(frames.len());
        for (frame, frame_detections) in frames.iter().enumerate() {
            let start = detections.len();
            detections.extend(
                frame_detections
                    .iter()
                    .enumerate()
                    .map(|(object, detection)| Detection {
                        centroid: Vec3::from(detection.centroid()),
                        radius: detection.radius(),
                        frame: frame as u32,
                        object,
                    }),
            );
            spans.push(start..detections.len());
        }
        Self { detections, spans }
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
        self.detection(index).radius
    }

    pub fn grid(&self, frame: usize, cell: f32) -> Grid<'_> {
        Grid::new(self, frame, cell)
    }

    fn detection(&self, index: usize) -> &Detection {
        &self.detections[index]
    }
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

    pub fn near(&self, at: Vec3, radius: f32, mut visit: impl FnMut(usize, f32)) {
        let base = key(at, self.cell);
        let reach = (radius / self.cell).ceil() as i64;
        for z in -reach..=reach {
            for y in -reach..=reach {
                for x in -reach..=reach {
                    let Some(indices) = self.cells.get(&[base[0] + x, base[1] + y, base[2] + z])
                    else {
                        continue;
                    };
                    for &index in indices {
                        let distance = at.distance(self.detections.centroid(index));
                        if distance <= radius {
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
