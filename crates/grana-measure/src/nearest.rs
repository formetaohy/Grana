use std::sync::Arc;

use crate::table::Table;

const MIN_SIDE: usize = 16;
const MAX_SIDE: usize = 512;

pub struct Nearest {
    x: Arc<Vec<f32>>,
    y: Arc<Vec<f32>>,
    extent: [(f32, f32); 2],
    side: usize,
    starts: Vec<u32>,
    ids: Vec<u32>,
}

impl Nearest {
    pub fn of(table: &Table, rows: &[u32], axes: [usize; 2]) -> Option<Self> {
        if rows.is_empty()
            || axes[0] == axes[1]
            || axes[0] >= table.columns().len()
            || axes[1] >= table.columns().len()
        {
            return None;
        }
        let extent = [table.extent(axes[0])?, table.extent(axes[1])?];
        if !(extent[0].1 > extent[0].0 && extent[1].1 > extent[1].0) {
            return None;
        }
        let x = table.materialize(axes[0]);
        let y = table.materialize(axes[1]);
        let side = ((rows.len() as f64).sqrt().ceil() as usize).clamp(MIN_SIDE, MAX_SIDE);
        let cells = side * side;
        let mut counts = vec![0u32; cells];
        for row in rows {
            if let Some(cell) = cell(extent, side, x[*row as usize], y[*row as usize]) {
                counts[cell] += 1;
            }
        }
        let mut starts = Vec::with_capacity(cells + 1);
        starts.push(0u32);
        for count in &counts {
            let held = *starts.last().expect("a nearest grid starts at zero");
            starts.push(held + count);
        }
        let total = *starts.last().expect("a nearest grid starts at zero") as usize;
        let mut ids = vec![0u32; total];
        let mut cursors = starts.clone();
        for row in rows {
            if let Some(cell) = cell(extent, side, x[*row as usize], y[*row as usize]) {
                ids[cursors[cell] as usize] = *row;
                cursors[cell] += 1;
            }
        }
        Some(Self {
            x,
            y,
            extent,
            side,
            starts,
            ids,
        })
    }

    pub fn nearest(&self, target: [f32; 2], pixels: [f32; 2]) -> Option<u32> {
        assert!(
            target.iter().all(|value| value.is_finite()),
            "a probe targets finite values"
        );
        assert!(
            pixels
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "a probe measures non-negative pixels"
        );
        let column = axis_cell(self.extent[0], self.side, target[0])? as isize;
        let row = axis_cell(self.extent[1], self.side, target[1])? as isize;
        let spans = [
            self.extent[0].1 - self.extent[0].0,
            self.extent[1].1 - self.extent[1].0,
        ];
        let least = pixels[0].min(pixels[1]);
        let side = self.side as isize;
        let mut best = f32::INFINITY;
        let mut found = None;
        for ring in 0..side {
            let reach = (ring as f32 - 1.0).max(0.0) / self.side as f32 * least;
            if reach * reach > best {
                break;
            }
            for cell in ring_cells(column, row, ring, side) {
                let start = self.starts[cell] as usize;
                let end = self.starts[cell + 1] as usize;
                for id in &self.ids[start..end] {
                    let row = *id as usize;
                    let x = self.x[row];
                    let y = self.y[row];
                    if !(x.is_finite() && y.is_finite()) {
                        continue;
                    }
                    let dx = (x - target[0]) / spans[0] * pixels[0];
                    let dy = (y - target[1]) / spans[1] * pixels[1];
                    let distance = dx * dx + dy * dy;
                    if distance < best {
                        best = distance;
                        found = Some(*id);
                    }
                }
            }
            if best == 0.0 {
                break;
            }
        }
        found
    }
}

fn cell(extent: [(f32, f32); 2], side: usize, x: f32, y: f32) -> Option<usize> {
    let column = axis_cell(extent[0], side, x)?;
    let row = axis_cell(extent[1], side, y)?;
    Some(row * side + column)
}

fn axis_cell((low, high): (f32, f32), side: usize, value: f32) -> Option<usize> {
    if !value.is_finite() {
        return None;
    }
    let span = high - low;
    if span <= 0.0 {
        return None;
    }
    let position = ((value - low) / span * side as f32).floor();
    Some((position as isize).clamp(0, side as isize - 1) as usize)
}

fn ring_cells(column: isize, row: isize, ring: isize, side: isize) -> Vec<usize> {
    let mut cells = Vec::new();
    let limit = side - 1;
    let inside = |x: isize, y: isize| (0..=limit).contains(&x) && (0..=limit).contains(&y);
    let push = |cells: &mut Vec<usize>, x: isize, y: isize| {
        if inside(x, y) {
            cells.push((y * side + x) as usize);
        }
    };
    if ring == 0 {
        push(&mut cells, column, row);
        return cells;
    }
    let left = column - ring;
    let right = column + ring;
    let top = row - ring;
    let bottom = row + ring;
    for x in left..=right {
        push(&mut cells, x, top);
        push(&mut cells, x, bottom);
    }
    for y in (top + 1)..bottom {
        push(&mut cells, left, y);
        push(&mut cells, right, y);
    }
    cells
}
