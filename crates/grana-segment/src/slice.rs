use std::collections::HashMap;
use std::path::Path;

use crate::Result;
use crate::dynamics;
use crate::error::Error;
use crate::model::Model;
use crate::tile::{self, Tile, UnionFind};

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
    model: Model,
    settings: Settings,
}

impl Segmenter {
    pub fn open(path: &Path, settings: Settings) -> Result<Self> {
        let model = Model::open(path)?;
        validate_settings(settings, model.info().side())?;
        Ok(Self { model, settings })
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn segment(&mut self, plane: &[f32], width: u32, height: u32) -> Result<Vec<u32>> {
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
        let step = side
            .checked_sub(settings.tile_overlap())
            .ok_or_else(|| Error::input("the tile overlap must be smaller than the model side"))?;
        if step == 0 {
            return Err(Error::input(
                "the tile overlap must be smaller than the model side",
            ));
        }
        let starts_x = tile::starts(width, side, step);
        let starts_y = tile::starts(height, side, step);
        let side_length = side as usize;
        let width_length = width as usize;
        let height_length = height as usize;
        let tile_area = side_length * side_length;
        let mut input = vec![0f32; tile_area * 3];
        let mut tiles = Vec::with_capacity(starts_x.len() * starts_y.len());
        for &start_y in &starts_y {
            for &start_x in &starts_x {
                for row in 0..side_length {
                    let source_y = (start_y as usize + row).min(height_length - 1);
                    let source_base = source_y * width_length;
                    let target_base = row * side_length;
                    for column in 0..side_length {
                        let source_x = (start_x as usize + column).min(width_length - 1);
                        let value = plane[source_base + source_x];
                        input[target_base + column] = value;
                        input[tile_area + target_base + column] = value;
                        input[2 * tile_area + target_base + column] = value;
                    }
                }
                let flow = self.model.infer(&input)?;
                let mut labels = flow.masks(settings.cellprob_threshold());
                dynamics::remove_bad_flow_masks(&flow, &mut labels, settings.flow_threshold());
                tiles.push(Tile::new(start_x, start_y, labels, side, width, height));
            }
        }
        let mut labels = tile::merge(&tiles, side, width, height, settings.tile_overlap());
        dynamics::remove_small_masks(&mut labels, settings.minimum_size());
        dynamics::fill_holes(&mut labels, width_length, height_length);
        dynamics::renumber(&mut labels);
        Ok(labels)
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
