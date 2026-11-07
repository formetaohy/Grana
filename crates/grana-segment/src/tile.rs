use std::collections::HashMap;

use crate::dynamics::renumber;

pub(crate) struct Tile {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) labels: Vec<u32>,
    pub(crate) area: Vec<u32>,
}

impl Tile {
    pub(crate) fn new(
        x: u32,
        y: u32,
        labels: Vec<u32>,
        side: u32,
        width: u32,
        height: u32,
    ) -> Self {
        assert!(x < width, "a tile must start inside the image width");
        assert!(y < height, "a tile must start inside the image height");
        let side = side as usize;
        assert_eq!(
            labels.len(),
            side * side,
            "a tile must contain side * side labels"
        );
        let valid_width = ((width - x) as usize).min(side);
        let valid_height = ((height - y) as usize).min(side);
        let mut max_label = 0u32;
        for row in 0..valid_height {
            for column in 0..valid_width {
                max_label = max_label.max(labels[row * side + column]);
            }
        }
        let mut area = vec![0u32; max_label as usize + 1];
        for row in 0..valid_height {
            for column in 0..valid_width {
                let label = labels[row * side + column];
                if label > 0 {
                    area[label as usize] += 1;
                }
            }
        }
        Self { x, y, labels, area }
    }
}

pub(crate) struct UnionFind {
    parents: Vec<u32>,
    sizes: Vec<u32>,
}

impl UnionFind {
    pub(crate) fn new() -> Self {
        Self {
            parents: Vec::new(),
            sizes: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self) -> u32 {
        let node = self.parents.len() as u32;
        self.parents.push(node);
        self.sizes.push(1);
        node
    }

    pub(crate) fn find(&mut self, mut node: u32) -> u32 {
        while self.parents[node as usize] != node {
            let parent = self.parents[node as usize];
            let grandparent = self.parents[parent as usize];
            self.parents[node as usize] = grandparent;
            node = grandparent;
        }
        node
    }

    pub(crate) fn union(&mut self, left: u32, right: u32) {
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

pub(crate) fn starts(length: u32, side: u32, step: u32) -> Vec<u32> {
    assert!(step > 0, "the tile step must be positive");
    assert!(side > 0, "the tile side must be positive");
    if length <= side {
        return vec![0];
    }
    let span = u64::from(length - side);
    let count = span.div_ceil(u64::from(step)) + 1;
    (0..count)
        .map(|index| (u128::from(span) * u128::from(index) / u128::from(count - 1)) as u32)
        .collect()
}

pub(crate) fn merge(
    tiles: &[Tile],
    side: u32,
    width: u32,
    height: u32,
    tile_overlap: u32,
) -> Vec<u32> {
    assert!(!tiles.is_empty(), "tile merging requires at least one tile");
    let step = side
        .checked_sub(tile_overlap)
        .expect("the tile overlap must be smaller than the side");
    assert!(step > 0, "the tile step must be positive");
    let columns = starts(width, side, step);
    let rows = starts(height, side, step);
    assert_eq!(
        tiles.len(),
        columns.len() * rows.len(),
        "the tile grid must match the image extents"
    );
    for (row, &tile_y) in rows.iter().enumerate() {
        for (column, &tile_x) in columns.iter().enumerate() {
            let tile = &tiles[row * columns.len() + column];
            assert!(
                tile.x == tile_x && tile.y == tile_y,
                "the tiles must be ordered row-major"
            );
        }
    }

    let mut forest = UnionFind::new();
    let mut offsets = Vec::with_capacity(tiles.len());
    let mut total_nodes = 0usize;
    for tile in tiles {
        offsets.push(total_nodes as u32);
        let node_count = tile.area.len() - 1;
        for _ in 0..node_count {
            forest.add();
        }
        total_nodes += node_count;
    }

    let side_extent = u64::from(side);
    let width_extent = u64::from(width);
    let height_extent = u64::from(height);
    for left_index in 0..tiles.len() {
        let left = &tiles[left_index];
        for right_index in (left_index + 1)..tiles.len() {
            let right = &tiles[right_index];
            let left_x = u64::from(left.x).max(u64::from(right.x));
            let right_x = (u64::from(left.x) + side_extent)
                .min(u64::from(right.x) + side_extent)
                .min(width_extent);
            let top = u64::from(left.y).max(u64::from(right.y));
            let bottom = (u64::from(left.y) + side_extent)
                .min(u64::from(right.y) + side_extent)
                .min(height_extent);
            if left_x >= right_x || top >= bottom {
                continue;
            }
            let mut intersections: HashMap<(u32, u32), u32> = HashMap::new();
            for y in top..bottom {
                let left_row = (y - u64::from(left.y)) as usize * side as usize;
                let right_row = (y - u64::from(right.y)) as usize * side as usize;
                for x in left_x..right_x {
                    let left_label = left.labels[left_row + (x - u64::from(left.x)) as usize];
                    let right_label = right.labels[right_row + (x - u64::from(right.x)) as usize];
                    if left_label > 0 && right_label > 0 {
                        *intersections.entry((left_label, right_label)).or_insert(0) += 1;
                    }
                }
            }
            for ((left_label, right_label), intersection) in intersections {
                let left_area = left.area[left_label as usize];
                let right_area = right.area[right_label as usize];
                let smaller = left_area.min(right_area);
                if smaller > 0 && intersection as f64 / smaller as f64 >= 0.5 {
                    forest.union(
                        offsets[left_index] + left_label - 1,
                        offsets[right_index] + right_label - 1,
                    );
                }
            }
        }
    }

    let mut mappings = Vec::with_capacity(tiles.len());
    for (index, tile) in tiles.iter().enumerate() {
        let mut mapping = vec![0u32; tile.area.len()];
        for (label, slot) in mapping.iter_mut().enumerate().skip(1) {
            *slot = forest.find(offsets[index] + label as u32 - 1) + 1;
        }
        mappings.push(mapping);
    }

    let center_offset = (side as f64 - 1.0) * 0.5;
    let column_candidates: Vec<Vec<(u32, f64)>> = (0..width)
        .map(|x| covering(&columns, x, side, center_offset))
        .collect();
    let row_candidates: Vec<Vec<(u32, f64)>> = (0..height)
        .map(|y| covering(&rows, y, side, center_offset))
        .collect();
    let column_count = columns.len();
    let mut labels = vec![0u32; width as usize * height as usize];
    for y in 0..height {
        let row_base = y as usize * width as usize;
        for x in 0..width {
            let mut best_label = 0u32;
            let mut best_distance = f64::INFINITY;
            for &(row, row_distance) in &row_candidates[y as usize] {
                let row = row as usize;
                let tile_row = rows[row];
                let local_y = y - tile_row;
                for &(column, column_distance) in &column_candidates[x as usize] {
                    let column = column as usize;
                    let tile = &tiles[row * column_count + column];
                    let local_x = x - columns[column];
                    let label = tile.labels[local_y as usize * side as usize + local_x as usize];
                    if label == 0 {
                        continue;
                    }
                    let distance = row_distance + column_distance;
                    if distance < best_distance {
                        best_distance = distance;
                        best_label = mappings[row * column_count + column][label as usize];
                    }
                }
            }
            labels[row_base + x as usize] = best_label;
        }
    }
    renumber(&mut labels);
    labels
}

fn covering(starts: &[u32], coordinate: u32, side: u32, center_offset: f64) -> Vec<(u32, f64)> {
    let upper = starts.partition_point(|&start| start <= coordinate);
    let lower = starts
        .partition_point(|&start| u64::from(start) + u64::from(side) <= u64::from(coordinate));
    assert!(
        lower < upper,
        "every pixel must be covered by at least one tile"
    );
    (lower..upper)
        .map(|index| {
            let center = starts[index] as f64 + center_offset;
            let offset = coordinate as f64 - center;
            (index as u32, offset * offset)
        })
        .collect()
}
