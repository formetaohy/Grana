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

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    side: u32,
    dy: Vec<f32>,
    dx: Vec<f32>,
    cellprob: Vec<f32>,
}

impl Flow {
    pub fn side(&self) -> u32 {
        self.side
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

    pub fn from_parts(side: u32, dy: Vec<f32>, dx: Vec<f32>, cellprob: Vec<f32>) -> Self {
        assert!(side > 0, "flow side must be positive");
        let area = side as usize * side as usize;
        assert_eq!(dy.len(), area, "flow dy must contain side * side values");
        assert_eq!(dx.len(), area, "flow dx must contain side * side values");
        assert_eq!(
            cellprob.len(),
            area,
            "flow cellprob must contain side * side values"
        );
        Self {
            side,
            dy,
            dx,
            cellprob,
        }
    }

    pub fn masks(&self, cellprob_threshold: f32) -> Vec<u32> {
        let side = self.side as usize;
        let area = side * side;
        if side < 2 {
            return vec![0; area];
        }
        let mut indices = Vec::new();
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        for (index, &probability) in self.cellprob.iter().enumerate() {
            if probability > cellprob_threshold {
                indices.push(index);
                rows.push((index / side) as f32);
                columns.push((index % side) as f32);
            }
        }
        if indices.is_empty() {
            return vec![0; area];
        }

        let scale = (side - 1) as f32;
        let normalized_scale = 2.0 / scale;
        let mut flow_y = vec![0f32; area];
        let mut flow_x = vec![0f32; area];
        for &index in &indices {
            flow_y[index] = self.dy[index] / FLOW_TO_STEP * normalized_scale;
            flow_x[index] = self.dx[index] / FLOW_TO_STEP * normalized_scale;
        }

        let mut points_y: Vec<f32> = rows.iter().map(|&row| row / scale * 2.0 - 1.0).collect();
        let mut points_x: Vec<f32> = columns
            .iter()
            .map(|&column| column / scale * 2.0 - 1.0)
            .collect();

        for _ in 0..INTEGRATION_STEPS {
            for point in 0..indices.len() {
                let step_x = sample_bilinear(&flow_x, side, points_x[point], points_y[point]);
                let step_y = sample_bilinear(&flow_y, side, points_x[point], points_y[point]);
                points_x[point] = (points_x[point] + step_x).clamp(-1.0, 1.0);
                points_y[point] = (points_y[point] + step_y).clamp(-1.0, 1.0);
            }
        }

        let padded = side + 2 * ENDPOINT_PADDING;
        let padded_area = padded * padded;
        let mut end_y = Vec::with_capacity(indices.len());
        let mut end_x = Vec::with_capacity(indices.len());
        for point in 0..indices.len() {
            let row = ((points_y[point] + 1.0) * 0.5 * scale).trunc() as isize;
            let column = ((points_x[point] + 1.0) * 0.5 * scale).trunc() as isize;
            end_y.push((row + ENDPOINT_PADDING as isize).clamp(0, padded as isize - 1) as usize);
            end_x.push((column + ENDPOINT_PADDING as isize).clamp(0, padded as isize - 1) as usize);
        }

        let mut histogram = vec![0u32; padded_area];
        for point in 0..indices.len() {
            histogram[end_y[point] * padded + end_x[point]] += 1;
        }
        let maximum = maximum_filter(&histogram, padded, MAX_FILTER_RADIUS);
        let margin = SEED_WINDOW_RADIUS;
        let mut seeds: Vec<usize> = (0..padded_area)
            .filter(|&index| {
                let y = index / padded;
                let x = index % padded;
                y >= margin
                    && y + margin < padded
                    && x >= margin
                    && x + margin < padded
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
            let seed_y = seed / padded;
            let seed_x = seed % padded;
            let mut window = [0u32; SEED_WINDOW_SIDE * SEED_WINDOW_SIDE];
            for row in 0..SEED_WINDOW_SIDE {
                let source_y = seed_y + row - SEED_WINDOW_RADIUS;
                for column in 0..SEED_WINDOW_SIDE {
                    let source_x = seed_x + column - SEED_WINDOW_RADIUS;
                    window[row * SEED_WINDOW_SIDE + column] =
                        histogram[source_y * padded + source_x];
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
                    let target = target_y * padded + target_x;
                    if endpoints[target] < label {
                        endpoints[target] = label;
                    }
                }
            }
        }

        let mut masks = vec![0u32; area];
        for (point, &index) in indices.iter().enumerate() {
            masks[index] = endpoints[end_y[point] * padded + end_x[point]];
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
}

pub(crate) fn remove_bad_flow_masks(flow: &Flow, labels: &mut [u32], threshold: f32) {
    let side = flow.side as usize;
    assert_eq!(
        labels.len(),
        side * side,
        "flow labels must match side * side"
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
    let mut counts = vec![0u32; length];
    let mut sum_y = vec![0f64; length];
    let mut sum_x = vec![0f64; length];
    for (index, &label) in labels.iter().enumerate() {
        if label == 0 {
            continue;
        }
        counts[label as usize] += 1;
        sum_y[label as usize] += (index / side) as f64;
        sum_x[label as usize] += (index % side) as f64;
    }
    let mut errors = vec![0f64; length];
    for (index, &label) in labels.iter().enumerate() {
        if label == 0 {
            continue;
        }
        let count = counts[label as usize] as f64;
        let center_y = sum_y[label as usize] / count;
        let center_x = sum_x[label as usize] / count;
        let y = (index / side) as f64;
        let x = (index % side) as f64;
        let offset_y = center_y - y;
        let offset_x = center_x - x;
        let magnitude = (offset_y * offset_y + offset_x * offset_x).sqrt();
        let (direction_y, direction_x) = if magnitude > 0.0 {
            (offset_y / magnitude, offset_x / magnitude)
        } else {
            (0.0, 0.0)
        };
        let residual_y = direction_y - flow.dy[index] as f64 / FLOW_TO_STEP as f64;
        let residual_x = direction_x - flow.dx[index] as f64 / FLOW_TO_STEP as f64;
        errors[label as usize] += residual_y * residual_y + residual_x * residual_x;
    }

    let mut remove = vec![false; length];
    let mut removed = false;
    for label in 1..length {
        if counts[label] > 0 && errors[label] / counts[label] as f64 > threshold as f64 {
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

fn sample_bilinear(field: &[f32], side: usize, x: f32, y: f32) -> f32 {
    let source_x = ((x + 1.0) * side as f32 - 1.0) * 0.5;
    let source_y = ((y + 1.0) * side as f32 - 1.0) * 0.5;
    let left = source_x.floor() as isize;
    let top = source_y.floor() as isize;
    let weight_x = source_x - left as f32;
    let weight_y = source_y - top as f32;
    let top_left = sample_or_zero(field, side, left, top);
    let top_right = sample_or_zero(field, side, left + 1, top);
    let bottom_left = sample_or_zero(field, side, left, top + 1);
    let bottom_right = sample_or_zero(field, side, left + 1, top + 1);
    (top_left * (1.0 - weight_x) + top_right * weight_x) * (1.0 - weight_y)
        + (bottom_left * (1.0 - weight_x) + bottom_right * weight_x) * weight_y
}

fn sample_or_zero(field: &[f32], side: usize, x: isize, y: isize) -> f32 {
    if x < 0 || y < 0 || x as usize >= side || y as usize >= side {
        0.0
    } else {
        field[y as usize * side + x as usize]
    }
}

fn maximum_filter(field: &[u32], side: usize, radius: usize) -> Vec<u32> {
    let mut horizontal = vec![0u32; field.len()];
    for y in 0..side {
        for x in 0..side {
            let start = x.saturating_sub(radius);
            let end = (x + radius).min(side - 1);
            let mut maximum = 0u32;
            for candidate in start..=end {
                maximum = maximum.max(field[y * side + candidate]);
            }
            horizontal[y * side + x] = maximum;
        }
    }
    let mut vertical = vec![0u32; field.len()];
    for y in 0..side {
        let start = y.saturating_sub(radius);
        let end = (y + radius).min(side - 1);
        for x in 0..side {
            let mut maximum = 0u32;
            for candidate in start..=end {
                maximum = maximum.max(horizontal[candidate * side + x]);
            }
            vertical[y * side + x] = maximum;
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
