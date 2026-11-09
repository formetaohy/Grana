use rayon::prelude::*;

const INTEGRATION_STEPS: usize = 200;
const FLOW_TO_STEP: f32 = 5.0;
const ENDPOINT_PADDING: usize = 20;
const MAX_SIZE_FRACTION: f64 = 0.4;
const MAX_FILTER_RADIUS: usize = 2;
const SEED_MIN_COUNT: u32 = 10;
const SEED_WINDOW_RADIUS: usize = 5;
const SEED_WINDOW_SIDE: usize = SEED_WINDOW_RADIUS * 2 + 1;
const SEED_GROWTH_STEPS: usize = 5;
const SEED_WINDOW_MIN_COUNT: u32 = 2;
const NEIGHBOUR_COUNT: f64 = 9.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    width: u32,
    height: u32,
    dy: Vec<f32>,
    dx: Vec<f32>,
    cellprob: Vec<f32>,
}

impl Flow {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn dy(&self) -> &[f32] {
        &self.dy
    }

    pub fn dx(&self) -> &[f32] {
        &self.dx
    }

    pub fn cellprob(&self) -> &[f32] {
        &self.cellprob
    }

    pub fn from_parts(
        width: u32,
        height: u32,
        dy: Vec<f32>,
        dx: Vec<f32>,
        cellprob: Vec<f32>,
    ) -> Self {
        assert!(width > 0 && height > 0, "a flow field is not empty");
        let area = width as usize * height as usize;
        assert_eq!(dy.len(), area, "flow dy must contain width * height values");
        assert_eq!(dx.len(), area, "flow dx must contain width * height values");
        assert_eq!(
            cellprob.len(),
            area,
            "flow cellprob must contain width * height values"
        );
        Self {
            width,
            height,
            dy,
            dx,
            cellprob,
        }
    }

    pub fn masks(&self, cellprob_threshold: f32) -> Vec<u32> {
        let width = self.width as usize;
        let height = self.height as usize;
        let area = width * height;
        if width < 2 || height < 2 {
            return vec![0; area];
        }
        let mut indices = Vec::new();
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        for (index, &probability) in self.cellprob.iter().enumerate() {
            if probability > cellprob_threshold {
                indices.push(index);
                rows.push((index / width) as f32);
                columns.push((index % width) as f32);
            }
        }
        if indices.is_empty() {
            return vec![0; area];
        }

        let scale_x = (width - 1) as f32;
        let scale_y = (height - 1) as f32;
        let (flow_x, flow_y) = self.field(&indices);

        let mut points_y: Vec<f32> = rows.iter().map(|&row| row / scale_y * 2.0 - 1.0).collect();
        let mut points_x: Vec<f32> = columns
            .iter()
            .map(|&column| column / scale_x * 2.0 - 1.0)
            .collect();

        integrate(
            &mut points_x,
            &mut points_y,
            &flow_x,
            &flow_y,
            width,
            height,
        );

        let padded_width = width + 2 * ENDPOINT_PADDING;
        let padded_height = height + 2 * ENDPOINT_PADDING;
        let padded_area = padded_width * padded_height;
        let (end_x, end_y) = endpoints(&points_x, &points_y, width, height);

        let mut histogram = vec![0u32; padded_area];
        for point in 0..indices.len() {
            histogram[end_y[point] * padded_width + end_x[point]] += 1;
        }
        let maximum = maximum_filter(&histogram, padded_width, padded_height, MAX_FILTER_RADIUS);
        let margin = SEED_WINDOW_RADIUS;
        let mut seeds: Vec<usize> = (0..padded_area)
            .filter(|&index| {
                let y = index / padded_width;
                let x = index % padded_width;
                y >= margin
                    && y + margin < padded_height
                    && x >= margin
                    && x + margin < padded_width
                    && histogram[index] > SEED_MIN_COUNT
                    && histogram[index] >= maximum[index]
            })
            .collect();
        if seeds.is_empty() {
            return vec![0; area];
        }
        seeds.sort_by_key(|&index| histogram[index]);

        let mut endpoints = vec![0u32; padded_area];
        for (rank, &seed) in seeds.iter().enumerate() {
            let seed_y = seed / padded_width;
            let seed_x = seed % padded_width;
            let mut window = [0u32; SEED_WINDOW_SIDE * SEED_WINDOW_SIDE];
            for row in 0..SEED_WINDOW_SIDE {
                let source_y = seed_y + row - SEED_WINDOW_RADIUS;
                for column in 0..SEED_WINDOW_SIDE {
                    let source_x = seed_x + column - SEED_WINDOW_RADIUS;
                    window[row * SEED_WINDOW_SIDE + column] =
                        histogram[source_y * padded_width + source_x];
                }
            }
            let mut selected = [false; SEED_WINDOW_SIDE * SEED_WINDOW_SIDE];
            selected[SEED_WINDOW_RADIUS * SEED_WINDOW_SIDE + SEED_WINDOW_RADIUS] = true;
            for _ in 0..SEED_GROWTH_STEPS {
                let mut grown = [false; SEED_WINDOW_SIDE * SEED_WINDOW_SIDE];
                for row in 0..SEED_WINDOW_SIDE {
                    for column in 0..SEED_WINDOW_SIDE {
                        let mut active = false;
                        for offset_y in -1isize..=1 {
                            for offset_x in -1isize..=1 {
                                let near_y = row as isize + offset_y;
                                let near_x = column as isize + offset_x;
                                if near_y >= 0
                                    && near_y < SEED_WINDOW_SIDE as isize
                                    && near_x >= 0
                                    && near_x < SEED_WINDOW_SIDE as isize
                                    && selected
                                        [near_y as usize * SEED_WINDOW_SIDE + near_x as usize]
                                {
                                    active = true;
                                }
                            }
                        }
                        grown[row * SEED_WINDOW_SIDE + column] = active
                            && window[row * SEED_WINDOW_SIDE + column] > SEED_WINDOW_MIN_COUNT;
                    }
                }
                selected = grown;
            }
            let label = rank as u32 + 1;
            for row in 0..SEED_WINDOW_SIDE {
                for column in 0..SEED_WINDOW_SIDE {
                    if !selected[row * SEED_WINDOW_SIDE + column] {
                        continue;
                    }
                    let target_y = seed_y + row - SEED_WINDOW_RADIUS;
                    let target_x = seed_x + column - SEED_WINDOW_RADIUS;
                    let target = target_y * padded_width + target_x;
                    if endpoints[target] < label {
                        endpoints[target] = label;
                    }
                }
            }
        }

        let mut masks = vec![0u32; area];
        for (point, &index) in indices.iter().enumerate() {
            masks[index] = endpoints[end_y[point] * padded_width + end_x[point]];
        }

        let mut counts = vec![0u32; seeds.len() + 1];
        for &label in &masks {
            if label > 0 {
                counts[label as usize] += 1;
            }
        }
        let limit = area as f64 * MAX_SIZE_FRACTION;
        let mut remove = vec![false; counts.len()];
        let mut removed = false;
        for label in 1..counts.len() {
            if counts[label] as f64 > limit {
                remove[label] = true;
                removed = true;
            }
        }
        if removed {
            for label in &mut masks {
                if *label > 0 && remove[*label as usize] {
                    *label = 0;
                }
            }
        }
        renumber(&mut masks);
        masks
    }

    pub fn seeded(&self, prompt: Prompt<'_>) -> Vec<u32> {
        let width = self.width as usize;
        let height = self.height as usize;
        let area = width * height;
        if width < 2 || height < 2 {
            return vec![0; area];
        }
        for seed in prompt.seeds() {
            assert!(
                seed[0] >= 0.0
                    && seed[0] <= (width - 1) as f32
                    && seed[1] >= 0.0
                    && seed[1] <= (height - 1) as f32,
                "a prompt seed lies inside its plane"
            );
        }
        let mut indices = Vec::new();
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        for (index, &probability) in self.cellprob.iter().enumerate() {
            if probability > prompt.threshold() {
                indices.push(index);
                rows.push((index / width) as f32);
                columns.push((index % width) as f32);
            }
        }
        if indices.is_empty() {
            return vec![0; area];
        }

        let scale_x = (width - 1) as f32;
        let scale_y = (height - 1) as f32;
        let (flow_x, flow_y) = self.field(&indices);
        let mut points_y: Vec<f32> = rows.iter().map(|&row| row / scale_y * 2.0 - 1.0).collect();
        let mut points_x: Vec<f32> = columns
            .iter()
            .map(|&column| column / scale_x * 2.0 - 1.0)
            .collect();
        integrate(
            &mut points_x,
            &mut points_y,
            &flow_x,
            &flow_y,
            width,
            height,
        );
        let (end_x, end_y) = endpoints(&points_x, &points_y, width, height);

        let mut prompt_x = Vec::with_capacity(prompt.seeds().len());
        let mut prompt_y = Vec::with_capacity(prompt.seeds().len());
        for (x, y) in prompt
            .seeds()
            .iter()
            .map(|seed| (2.0 * seed[0] / scale_x - 1.0, 2.0 * seed[1] / scale_y - 1.0))
        {
            prompt_x.push(x);
            prompt_y.push(y);
        }
        integrate(
            &mut prompt_x,
            &mut prompt_y,
            &flow_x,
            &flow_y,
            width,
            height,
        );
        let (prompt_end_x, prompt_end_y) = endpoints(&prompt_x, &prompt_y, width, height);

        let reach = prompt.reach().max(1.0);
        let limit = reach * reach;
        let mut masks = vec![0u32; area];
        for (point, &index) in indices.iter().enumerate() {
            let mut nearest = 0u32;
            let mut distance = limit;
            for (rank, (&x, &y)) in prompt_end_x.iter().zip(&prompt_end_y).enumerate() {
                let offset_x = end_x[point] as f32 - x as f32;
                let offset_y = end_y[point] as f32 - y as f32;
                let squared = offset_x * offset_x + offset_y * offset_y;
                if squared <= distance {
                    distance = squared;
                    nearest = rank as u32 + 1;
                }
            }
            masks[index] = nearest;
        }
        renumber(&mut masks);
        masks
    }

    fn field(&self, indices: &[usize]) -> (Vec<f32>, Vec<f32>) {
        let width = self.width as usize;
        let height = self.height as usize;
        let scale_x = (width - 1) as f32;
        let scale_y = (height - 1) as f32;
        let mut flow_y = vec![0f32; width * height];
        let mut flow_x = vec![0f32; width * height];
        for &index in indices {
            flow_y[index] = self.dy[index] / FLOW_TO_STEP * 2.0 / scale_y;
            flow_x[index] = self.dx[index] / FLOW_TO_STEP * 2.0 / scale_x;
        }
        (flow_x, flow_y)
    }
}

pub struct Prompt<'a> {
    seeds: &'a [[f32; 2]],
    threshold: f32,
    reach: f32,
}

impl<'a> Prompt<'a> {
    pub fn new(seeds: &'a [[f32; 2]], threshold: f32, reach: f32) -> Self {
        assert!(!seeds.is_empty(), "a prompt names at least one seed point");
        assert!(
            seeds
                .iter()
                .all(|seed| seed.iter().all(|value| value.is_finite())),
            "a prompt seed point is finite"
        );
        assert!(
            threshold.is_finite(),
            "a prompt cell probability threshold is finite"
        );
        assert!(
            reach.is_finite() && reach >= 0.0,
            "a prompt reach is a non negative distance"
        );
        Self {
            seeds,
            threshold,
            reach,
        }
    }

    pub fn seeds(&self) -> &[[f32; 2]] {
        self.seeds
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    pub fn reach(&self) -> f32 {
        self.reach
    }
}

fn integrate(
    points_x: &mut [f32],
    points_y: &mut [f32],
    flow_x: &[f32],
    flow_y: &[f32],
    width: usize,
    height: usize,
) {
    for _ in 0..INTEGRATION_STEPS {
        points_x
            .par_iter_mut()
            .zip(points_y.par_iter_mut())
            .for_each(|(point_x, point_y)| {
                let step_x = sample_bilinear(flow_x, width, height, *point_x, *point_y);
                let step_y = sample_bilinear(flow_y, width, height, *point_x, *point_y);
                *point_x = (*point_x + step_x).clamp(-1.0, 1.0);
                *point_y = (*point_y + step_y).clamp(-1.0, 1.0);
            });
    }
}

fn endpoints(
    points_x: &[f32],
    points_y: &[f32],
    width: usize,
    height: usize,
) -> (Vec<usize>, Vec<usize>) {
    let scale_x = (width - 1) as f32;
    let scale_y = (height - 1) as f32;
    let padded_width = width + 2 * ENDPOINT_PADDING;
    let padded_height = height + 2 * ENDPOINT_PADDING;
    let mut end_x = Vec::with_capacity(points_x.len());
    let mut end_y = Vec::with_capacity(points_x.len());
    for point in 0..points_x.len() {
        let row = ((points_y[point] + 1.0) * 0.5 * scale_y).trunc() as isize;
        let column = ((points_x[point] + 1.0) * 0.5 * scale_x).trunc() as isize;
        end_y.push((row + ENDPOINT_PADDING as isize).clamp(0, padded_height as isize - 1) as usize);
        end_x.push(
            (column + ENDPOINT_PADDING as isize).clamp(0, padded_width as isize - 1) as usize,
        );
    }
    (end_x, end_y)
}

pub(crate) fn remove_bad_flow_masks(flow: &Flow, labels: &mut [u32], threshold: f32) {
    let width = flow.width as usize;
    let height = flow.height as usize;
    assert_eq!(
        labels.len(),
        width * height,
        "flow labels must match width * height"
    );
    if threshold <= 0.0 {
        return;
    }
    let mut max_label = 0u32;
    for &label in labels.iter() {
        max_label = max_label.max(label);
    }
    if max_label == 0 {
        return;
    }

    let length = max_label as usize + 1;
    let mut geometry = vec![Geometry::default(); length];
    for (index, &label) in labels.iter().enumerate() {
        if label == 0 {
            continue;
        }
        let geometry = &mut geometry[label as usize];
        let y = index / width;
        let x = index % width;
        if geometry.pixels.is_empty() {
            geometry.min_y = y;
            geometry.max_y = y;
            geometry.min_x = x;
            geometry.max_x = x;
        } else {
            geometry.min_y = geometry.min_y.min(y);
            geometry.max_y = geometry.max_y.max(y);
            geometry.min_x = geometry.min_x.min(x);
            geometry.max_x = geometry.max_x.max(x);
        }
        geometry.pixels.push(index);
        geometry.sum_y += y as u64;
        geometry.sum_x += x as u64;
    }

    let iterations = 2 * geometry
        .iter()
        .filter(|geometry| !geometry.pixels.is_empty())
        .map(|geometry| geometry.max_y - geometry.min_y + geometry.max_x - geometry.min_x + 4)
        .max()
        .unwrap_or(0);
    if iterations == 0 {
        return;
    }

    let padded_width = width + 2;
    let padded_height = height + 2;
    let slot_count: usize = geometry.iter().map(|geometry| geometry.pixels.len()).sum();
    let mut slots = Vec::with_capacity(slot_count);
    let mut neighbours = Vec::with_capacity(slot_count * 9);
    let mut neighbours_same = Vec::with_capacity(slot_count * 9);
    let mut label_of_slot = Vec::with_capacity(slot_count);
    let mut error = vec![0.0f64; length];
    let mut centers = vec![0usize; length];
    for label in 1..length {
        let geometry = &mut geometry[label];
        if geometry.pixels.is_empty() {
            continue;
        }
        let count = geometry.pixels.len() as f64;
        let center_y = (geometry.sum_y as f64 / count).round() as usize;
        let center_x = (geometry.sum_x as f64 / count).round() as usize;
        let center = if labels[center_y * width + center_x] == label as u32 {
            center_y * width + center_x
        } else {
            geometry
                .pixels
                .iter()
                .copied()
                .min_by_key(|&pixel| {
                    let y = pixel / width;
                    let x = pixel % width;
                    y.abs_diff(center_y).pow(2) + x.abs_diff(center_x).pow(2)
                })
                .expect("a mask holds pixels")
        };
        for &pixel in geometry.pixels.iter() {
            let y = pixel / width;
            let x = pixel % width;
            let slot = slots.len();
            if pixel == center {
                centers[label] = slot;
            }
            slots.push((y + 1) * padded_width + (x + 1));
            label_of_slot.push(label as u32);
            for offset_y in -1isize..=1 {
                for offset_x in -1isize..=1 {
                    let near_y = y as isize + offset_y;
                    let near_x = x as isize + offset_x;
                    let inside = near_y >= 0
                        && near_x >= 0
                        && near_y < height as isize
                        && near_x < width as isize;
                    neighbours.push(
                        (near_y + 1).max(0) as usize * padded_width + (near_x + 1).max(0) as usize,
                    );
                    neighbours_same.push(
                        inside && labels[near_y as usize * width + near_x as usize] == label as u32,
                    );
                }
            }
        }
    }

    let size = padded_width * padded_height;
    let mut field = vec![0f64; size];
    let mut next = vec![0f64; slot_count];
    for _ in 0..iterations {
        for label in 1..length {
            if !geometry[label].pixels.is_empty() {
                field[slots[centers[label]]] += 1.0;
            }
        }
        next.par_iter_mut().enumerate().for_each(|(slot, value)| {
            let mut total = 0.0;
            for offset in 0..9 {
                if neighbours_same[slot * 9 + offset] {
                    total += field[neighbours[slot * 9 + offset]];
                }
            }
            *value = total / NEIGHBOUR_COUNT;
        });
        for slot in 0..slot_count {
            field[slots[slot]] = next[slot];
        }
    }

    for slot in 0..slot_count {
        let index = slots[slot];
        let down = field[index + padded_width];
        let up = field[index - padded_width];
        let right = field[index + 1];
        let left = field[index - 1];
        let mut dy = down - up;
        let mut dx = right - left;
        let magnitude = (dy * dy + dx * dx).sqrt();
        if magnitude > 0.0 {
            dy /= magnitude;
            dx /= magnitude;
        } else {
            dy = 0.0;
            dx = 0.0;
        }
        let pixel = (index / padded_width - 1) * width + index % padded_width - 1;
        let residual_y = dy - f64::from(flow.dy[pixel] / FLOW_TO_STEP);
        let residual_x = dx - f64::from(flow.dx[pixel] / FLOW_TO_STEP);
        error[label_of_slot[slot] as usize] += residual_y * residual_y + residual_x * residual_x;
    }

    let mut remove = vec![false; length];
    let mut removed = false;
    for label in 1..length {
        let count = geometry[label].pixels.len();
        if count > 0 && error[label] / count as f64 > f64::from(threshold) {
            remove[label] = true;
            removed = true;
        }
    }
    if removed {
        for label in labels.iter_mut() {
            if *label > 0 && remove[*label as usize] {
                *label = 0;
            }
        }
        renumber(labels);
    }
}

#[derive(Clone, Default)]
struct Geometry {
    pixels: Vec<usize>,
    min_y: usize,
    max_y: usize,
    min_x: usize,
    max_x: usize,
    sum_y: u64,
    sum_x: u64,
}

pub(crate) fn remove_small_masks(labels: &mut [u32], minimum_size: u32) {
    if minimum_size == 0 {
        return;
    }
    let mut max_label = 0u32;
    for &label in labels.iter() {
        max_label = max_label.max(label);
    }
    if max_label == 0 {
        return;
    }
    let mut counts = vec![0u32; max_label as usize + 1];
    for &label in labels.iter() {
        if label > 0 {
            counts[label as usize] += 1;
        }
    }
    let mut remove = vec![false; counts.len()];
    let mut removed = false;
    for label in 1..counts.len() {
        if counts[label] > 0 && counts[label] < minimum_size {
            remove[label] = true;
            removed = true;
        }
    }
    if removed {
        for label in labels.iter_mut() {
            if *label > 0 && remove[*label as usize] {
                *label = 0;
            }
        }
        renumber(labels);
    }
}

pub(crate) fn fill_holes(labels: &mut [u32], width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    let area = width * height;
    assert_eq!(
        labels.len(),
        area,
        "label image must contain width * height values"
    );
    if !labels.iter().any(|&label| label != 0) {
        return;
    }

    let mut border_reachable = vec![false; area];
    let mut queue = Vec::new();
    for x in 0..width {
        seed_background(x, labels, &mut border_reachable, &mut queue);
        seed_background(
            (height - 1) * width + x,
            labels,
            &mut border_reachable,
            &mut queue,
        );
    }
    for y in 0..height {
        seed_background(y * width, labels, &mut border_reachable, &mut queue);
        seed_background(
            y * width + width - 1,
            labels,
            &mut border_reachable,
            &mut queue,
        );
    }
    let mut head = 0;
    while head < queue.len() {
        let index = queue[head];
        head += 1;
        for_each_neighbor(index, width, height, |neighbor| {
            if labels[neighbor] == 0 && !border_reachable[neighbor] {
                border_reachable[neighbor] = true;
                queue.push(neighbor);
            }
        });
    }

    let mut has_holes = false;
    for index in 0..area {
        if labels[index] == 0 && !border_reachable[index] {
            has_holes = true;
            break;
        }
    }
    if !has_holes {
        return;
    }

    let mut assigned = vec![0u32; area];
    let mut candidates = vec![0u32; area];
    let mut frontier = Vec::new();
    for index in 0..area {
        if labels[index] != 0 || border_reachable[index] {
            continue;
        }
        let mut nearest = 0u32;
        for_each_neighbor(index, width, height, |neighbor| {
            let label = labels[neighbor];
            if label > 0 && (nearest == 0 || label < nearest) {
                nearest = label;
            }
        });
        if nearest > 0 {
            assigned[index] = nearest;
            frontier.push(index);
        }
    }

    while !frontier.is_empty() {
        let mut next = Vec::new();
        for &index in &frontier {
            let label = assigned[index];
            for_each_neighbor(index, width, height, |neighbor| {
                if labels[neighbor] == 0 && !border_reachable[neighbor] && assigned[neighbor] == 0 {
                    if candidates[neighbor] == 0 {
                        candidates[neighbor] = label;
                        next.push(neighbor);
                    } else if label < candidates[neighbor] {
                        candidates[neighbor] = label;
                    }
                }
            });
        }
        for &index in &next {
            assigned[index] = candidates[index];
        }
        frontier = next;
    }

    for (index, label) in labels.iter_mut().enumerate() {
        if *label == 0 && !border_reachable[index] {
            assert!(
                assigned[index] > 0,
                "every enclosed background pixel must be assigned to a surrounding label"
            );
            *label = assigned[index];
        }
    }
}

pub(crate) fn renumber(labels: &mut [u32]) {
    let mut max_label = 0u32;
    for &label in labels.iter() {
        max_label = max_label.max(label);
    }
    if max_label == 0 {
        return;
    }
    let mut mapping = vec![0u32; max_label as usize + 1];
    let mut next = 0u32;
    for label in labels.iter_mut() {
        if *label == 0 {
            continue;
        }
        let slot = &mut mapping[*label as usize];
        if *slot == 0 {
            next += 1;
            *slot = next;
        }
        *label = *slot;
    }
}

fn sample_bilinear(field: &[f32], width: usize, height: usize, x: f32, y: f32) -> f32 {
    let source_x = ((x + 1.0) * width as f32 - 1.0) * 0.5;
    let source_y = ((y + 1.0) * height as f32 - 1.0) * 0.5;
    let left = source_x.floor() as isize;
    let top = source_y.floor() as isize;
    let weight_x = source_x - left as f32;
    let weight_y = source_y - top as f32;
    let top_left = sample_or_zero(field, width, height, left, top);
    let top_right = sample_or_zero(field, width, height, left + 1, top);
    let bottom_left = sample_or_zero(field, width, height, left, top + 1);
    let bottom_right = sample_or_zero(field, width, height, left + 1, top + 1);
    (top_left * (1.0 - weight_x) + top_right * weight_x) * (1.0 - weight_y)
        + (bottom_left * (1.0 - weight_x) + bottom_right * weight_x) * weight_y
}

fn sample_or_zero(field: &[f32], width: usize, height: usize, x: isize, y: isize) -> f32 {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        0.0
    } else {
        field[y as usize * width + x as usize]
    }
}

fn maximum_filter(field: &[u32], width: usize, height: usize, radius: usize) -> Vec<u32> {
    let mut horizontal = vec![0u32; field.len()];
    for y in 0..height {
        for x in 0..width {
            let start = x.saturating_sub(radius);
            let end = (x + radius).min(width - 1);
            let mut maximum = 0u32;
            for candidate in start..=end {
                maximum = maximum.max(field[y * width + candidate]);
            }
            horizontal[y * width + x] = maximum;
        }
    }
    let mut vertical = vec![0u32; field.len()];
    for y in 0..height {
        let start = y.saturating_sub(radius);
        let end = (y + radius).min(height - 1);
        for x in 0..width {
            let mut maximum = 0u32;
            for candidate in start..=end {
                maximum = maximum.max(horizontal[candidate * width + x]);
            }
            vertical[y * width + x] = maximum;
        }
    }
    vertical
}

fn seed_background(index: usize, labels: &[u32], reachable: &mut [bool], queue: &mut Vec<usize>) {
    if labels[index] == 0 && !reachable[index] {
        reachable[index] = true;
        queue.push(index);
    }
}

fn for_each_neighbor(index: usize, width: usize, height: usize, mut visit: impl FnMut(usize)) {
    let x = index % width;
    let y = index / width;
    if x > 0 {
        visit(index - 1);
    }
    if x + 1 < width {
        visit(index + 1);
    }
    if y > 0 {
        visit(index - width);
    }
    if y + 1 < height {
        visit(index + width);
    }
}
