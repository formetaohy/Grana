use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::Result;
use crate::cache;
use crate::dynamics;
use crate::error::Error;
use crate::model::Model;
use crate::tile;

const TILE_BATCH: usize = 4;
const FLAT_EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    cellprob_threshold: f32,
    flow_threshold: f32,
    minimum_size: u32,
    tile_overlap: u32,
    slice_overlap: f32,
}

impl Settings {
    pub fn new(
        cellprob_threshold: f32,
        flow_threshold: f32,
        minimum_size: u32,
        tile_overlap: u32,
        slice_overlap: f32,
    ) -> Self {
        Self {
            cellprob_threshold,
            flow_threshold,
            minimum_size,
            tile_overlap,
            slice_overlap,
        }
    }

    pub fn cellprob_threshold(self) -> f32 {
        self.cellprob_threshold
    }

    pub fn flow_threshold(self) -> f32 {
        self.flow_threshold
    }

    pub fn minimum_size(self) -> u32 {
        self.minimum_size
    }

    pub fn tile_overlap(self) -> u32 {
        self.tile_overlap
    }

    pub fn slice_overlap(self) -> f32 {
        self.slice_overlap
    }
}

pub struct Segmenter {
    model: Arc<Model>,
    settings: Settings,
}

impl Segmenter {
    pub fn open(path: &Path, settings: Settings) -> Result<Self> {
        let model = cache::acquire(path)?;
        validate_settings(settings, model.info().side())?;
        Ok(Self { model, settings })
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn segment(&self, plane: &[f32], width: u32, height: u32) -> Result<Vec<u32>> {
        let length = image_length(width, height)?;
        if plane.len() != length {
            return Err(Error::input(format!(
                "the plane must contain {length} values, found {}",
                plane.len()
            )));
        }
        if plane
            .iter()
            .any(|&value| !value.is_finite() || value < 0.0 || value > 1.0)
        {
            return Err(Error::input(
                "the plane must contain finite values in the range 0..=1",
            ));
        }
        let side = self.model.info().side();
        let settings = self.settings;
        if plane.iter().all(|&value| value == plane[0]) {
            return Ok(vec![0; length]);
        }

        let (lead_x, trail_x) = tile::padding(width, side);
        let (lead_y, trail_y) = tile::padding(height, side);
        let padded_width = width
            .checked_add(lead_x + trail_x)
            .ok_or_else(|| Error::input("the padded plane width overflows the address space"))?;
        let padded_height = height
            .checked_add(lead_y + trail_y)
            .ok_or_else(|| Error::input("the padded plane height overflows the address space"))?;
        let padded_length = (padded_width as usize)
            .checked_mul(padded_height as usize)
            .ok_or_else(|| Error::input("the padded plane overflows the address space"))?;
        let mut padded = vec![0f32; padded_length];
        let width_length = width as usize;
        let height_length = height as usize;
        for row in 0..height_length {
            let source = row * width_length;
            let target = (row + lead_y as usize) * padded_width as usize + lead_x as usize;
            padded[target..target + width_length]
                .copy_from_slice(&plane[source..source + width_length]);
        }

        let starts_x = tile::starts(padded_width, side, settings.tile_overlap());
        let starts_y = tile::starts(padded_height, side, settings.tile_overlap());
        let side_length = side as usize;
        let tile_area = side_length * side_length;
        let mut taper = vec![0f32; tile_area];
        tile::taper(side_length, &mut taper);
        let mut accumulator = Accumulator {
            width: padded_width as usize,
            dy: vec![0f32; padded_length],
            dx: vec![0f32; padded_length],
            cellprob: vec![0f32; padded_length],
            weight: vec![0f32; padded_length],
            taper,
            side: side_length,
        };
        let mut batch: Vec<Vec<f32>> = Vec::with_capacity(TILE_BATCH);
        let mut origins: Vec<(u32, u32)> = Vec::with_capacity(TILE_BATCH);
        for &start_y in &starts_y {
            for &start_x in &starts_x {
                let mut values = vec![0f32; tile_area];
                let mut low = f32::INFINITY;
                let mut high = f32::NEG_INFINITY;
                for row in 0..side_length {
                    let source =
                        (start_y as usize + row) * padded_width as usize + start_x as usize;
                    let target = row * side_length;
                    for column in 0..side_length {
                        let value = padded[source + column];
                        low = low.min(value);
                        high = high.max(value);
                        values[target + column] = value;
                    }
                }
                if high - low <= FLAT_EPSILON {
                    continue;
                }
                batch.push(values);
                origins.push((start_x, start_y));
                if batch.len() == TILE_BATCH {
                    accumulator.infer(&self.model, &batch, &origins)?;
                    batch.clear();
                    origins.clear();
                }
            }
        }
        if !batch.is_empty() {
            accumulator.infer(&self.model, &batch, &origins)?;
        }

        let mut dy = vec![0f32; length];
        let mut dx = vec![0f32; length];
        let mut cellprob = vec![0f32; length];
        for row in 0..height_length {
            let source = (row + lead_y as usize) * padded_width as usize + lead_x as usize;
            let target = row * width_length;
            for column in 0..width_length {
                let index = source + column;
                let weight = accumulator.weight[index];
                if weight > 0.0 {
                    dy[target + column] = accumulator.dy[index] / weight;
                    dx[target + column] = accumulator.dx[index] / weight;
                    cellprob[target + column] = accumulator.cellprob[index] / weight;
                } else {
                    cellprob[target + column] = f32::NEG_INFINITY;
                }
            }
        }

        let flow = crate::dynamics::Flow::from_parts(width, height, dy, dx, cellprob);
        let mut labels = flow.masks(settings.cellprob_threshold());
        dynamics::remove_bad_flow_masks(&flow, &mut labels, settings.flow_threshold());
        dynamics::remove_small_masks(&mut labels, settings.minimum_size());
        dynamics::fill_holes(&mut labels, width_length, height_length);
        dynamics::renumber(&mut labels);
        Ok(labels)
    }
}

struct Accumulator {
    width: usize,
    side: usize,
    dy: Vec<f32>,
    dx: Vec<f32>,
    cellprob: Vec<f32>,
    weight: Vec<f32>,
    taper: Vec<f32>,
}

impl Accumulator {
    fn infer(&mut self, model: &Model, batch: &[Vec<f32>], origins: &[(u32, u32)]) -> Result<()> {
        let flows = model.infer_batch(batch)?;
        for (flow, &(start_x, start_y)) in flows.iter().zip(origins) {
            for row in 0..self.side {
                let target = (start_y as usize + row) * self.width + start_x as usize;
                let source = row * self.side;
                for column in 0..self.side {
                    let weight = self.taper[source + column];
                    let index = target + column;
                    self.dy[index] += flow.dy()[source + column] * weight;
                    self.dx[index] += flow.dx()[source + column] * weight;
                    self.cellprob[index] += flow.cellprob()[source + column] * weight;
                    self.weight[index] += weight;
                }
            }
        }
        Ok(())
    }
}

pub struct Volume {
    width: u32,
    height: u32,
    depth: u32,
    labels: Vec<u32>,
    count: u32,
}

impl Volume {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn labels(&self) -> &[u32] {
        &self.labels
    }

    pub fn count(&self) -> u32 {
        self.count
    }
}

pub fn assemble(slices: &[Vec<u32>], width: u32, height: u32, overlap: f32) -> Result<Volume> {
    let length = image_length(width, height)?;
    if slices.is_empty() {
        return Err(Error::input("a volume must contain at least one slice"));
    }
    if !overlap.is_finite() || !(0.0..=1.0).contains(&overlap) {
        return Err(Error::input(
            "the slice overlap must be a finite value in the range 0..=1",
        ));
    }
    for (index, slice) in slices.iter().enumerate() {
        if slice.len() != length {
            return Err(Error::input(format!(
                "slice {index} must contain {length} labels, found {}",
                slice.len()
            )));
        }
    }

    let mut forest = UnionFind::new();
    let mut nodes: Vec<HashMap<u32, u32>> = Vec::with_capacity(slices.len());
    let mut areas: Vec<HashMap<u32, u32>> = Vec::with_capacity(slices.len());
    for slice in slices {
        let mut area_map = HashMap::new();
        let mut node_map = HashMap::new();
        for &label in slice {
            if label == 0 {
                continue;
            }
            *area_map.entry(label).or_insert(0u32) += 1;
            node_map.entry(label).or_insert_with(|| forest.add());
        }
        nodes.push(node_map);
        areas.push(area_map);
    }

    for index in 0..slices.len() - 1 {
        let mut intersections: HashMap<(u32, u32), u32> = HashMap::new();
        for (&left, &right) in slices[index].iter().zip(&slices[index + 1]) {
            if left > 0 && right > 0 {
                *intersections.entry((left, right)).or_insert(0) += 1;
            }
        }
        for ((left, right), intersection) in intersections {
            let left_area = areas[index][&left];
            let right_area = areas[index + 1][&right];
            let smaller = left_area.min(right_area);
            if smaller > 0 && intersection as f64 / smaller as f64 >= overlap as f64 {
                let left_node = nodes[index][&left];
                let right_node = nodes[index + 1][&right];
                forest.union(left_node, right_node);
            }
        }
    }

    let depth = slices.len();
    let label_count = length
        .checked_mul(depth)
        .ok_or_else(|| Error::input("the volume labels overflow the address space"))?;
    let mut labels = vec![0u32; label_count];
    let mut numbering: HashMap<u32, u32> = HashMap::new();
    let mut count = 0u32;
    for (index, slice) in slices.iter().enumerate() {
        let base = index * length;
        for (pixel, &label) in slice.iter().enumerate() {
            if label == 0 {
                continue;
            }
            let node = nodes[index][&label];
            let root = forest.find(node);
            let dense = match numbering.get(&root) {
                Some(&dense) => dense,
                None => {
                    count += 1;
                    numbering.insert(root, count);
                    count
                }
            };
            labels[base + pixel] = dense;
        }
    }

    Ok(Volume {
        width,
        height,
        depth: depth as u32,
        labels,
        count,
    })
}

fn validate_settings(settings: Settings, side: u32) -> Result<()> {
    if !settings.cellprob_threshold.is_finite() {
        return Err(Error::input("the cellprob threshold must be finite"));
    }
    if !settings.flow_threshold.is_finite() || settings.flow_threshold < 0.0 {
        return Err(Error::input(
            "the flow threshold must be a finite non-negative value",
        ));
    }
    if settings.tile_overlap >= side {
        return Err(Error::input(
            "the tile overlap must be smaller than the model side",
        ));
    }
    if !settings.slice_overlap.is_finite() || !(0.0..=1.0).contains(&settings.slice_overlap) {
        return Err(Error::input(
            "the slice overlap must be a finite value in the range 0..=1",
        ));
    }
    Ok(())
}

fn image_length(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(Error::input("the image width and height must be positive"));
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::input("the image dimensions overflow the address space"))
}

struct UnionFind {
    parents: Vec<u32>,
    sizes: Vec<u32>,
}

impl UnionFind {
    fn new() -> Self {
        Self {
            parents: Vec::new(),
            sizes: Vec::new(),
        }
    }

    fn add(&mut self) -> u32 {
        let node = self.parents.len() as u32;
        self.parents.push(node);
        self.sizes.push(1);
        node
    }

    fn find(&mut self, mut node: u32) -> u32 {
        while self.parents[node as usize] != node {
            let parent = self.parents[node as usize];
            let grandparent = self.parents[parent as usize];
            self.parents[node as usize] = grandparent;
            node = grandparent;
        }
        node
    }

    fn union(&mut self, left: u32, right: u32) {
        let left = self.find(left);
        let right = self.find(right);
        if left == right {
            return;
        }
        let (larger, smaller) = if self.sizes[left as usize] >= self.sizes[right as usize] {
            (left, right)
        } else {
            (right, left)
        };
        self.parents[smaller as usize] = larger;
        self.sizes[larger as usize] += self.sizes[smaller as usize];
    }
}
