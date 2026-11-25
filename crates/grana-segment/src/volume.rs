#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Component {
    center: [f32; 3],
}

impl Component {
    pub fn center(&self) -> [f32; 3] {
        self.center
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
    pub fn new(width: u32, height: u32, depth: u32, labels: Vec<u32>) -> Self {
        let count = labels.iter().copied().max().unwrap_or(0);
        Self {
            width,
            height,
            depth,
            labels,
            count,
        }
    }

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

    pub fn filter(&mut self, mut keep: impl FnMut([u32; 3]) -> bool) {
        let width = self.width as usize;
        let height = self.height as usize;
        for (index, label) in self.labels.iter_mut().enumerate() {
            if *label == 0 {
                continue;
            }
            let position = [
                (index % width) as u32,
                (index / width % height) as u32,
                (index / (width * height)) as u32,
            ];
            if !keep(position) {
                *label = 0;
            }
        }
        self.renumber();
    }

    pub fn cropped(&self, origin: [u32; 3], extent: [u32; 3]) -> Self {
        let zero = extent.iter().all(|value| *value > 0);
        let inside = (0..3)
            .all(|axis| origin[axis] + extent[axis] <= [self.width, self.height, self.depth][axis]);
        assert!(
            zero && inside,
            "a crop covers a non empty block inside the volume"
        );
        let width = extent[0] as usize;
        let height = extent[1] as usize;
        let depth = extent[2] as usize;
        let mut labels = Vec::with_capacity(width * height * depth);
        for z in 0..depth {
            for y in 0..height {
                let start = origin[0] as usize
                    + self.width as usize * (origin[1] as usize + y)
                    + self.width as usize * self.height as usize * (origin[2] as usize + z);
                labels.extend_from_slice(&self.labels[start..start + width]);
            }
        }
        Self {
            width: extent[0],
            height: extent[1],
            depth: extent[2],
            labels,
            count: self.count,
        }
    }

    fn renumber(&mut self) {
        let mut maximum = 0u32;
        for label in &self.labels {
            maximum = maximum.max(*label);
        }
        let mut mapping = vec![0u32; maximum as usize + 1];
        let mut next = 0u32;
        for label in &mut self.labels {
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
        self.count = next;
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn retain_containing(&mut self, voxels: &[[u32; 3]]) -> Vec<Component> {
        let width = self.width as usize;
        let height = self.height as usize;
        let limits = [self.width, self.height, self.depth];
        let mut selected: Vec<u32> = Vec::with_capacity(voxels.len());
        for voxel in voxels {
            if (0..3).any(|axis| voxel[axis] >= limits[axis]) {
                continue;
            }
            let index =
                voxel[0] as usize + width * (voxel[1] as usize + height * voxel[2] as usize);
            let label = self.labels[index];
            if label > 0 && !selected.contains(&label) {
                selected.push(label);
            }
        }
        selected.sort_unstable();
        let mut slots = vec![u32::MAX; self.count as usize + 1];
        for (slot, label) in selected.iter().enumerate() {
            slots[*label as usize] = slot as u32;
        }
        let mut sums = vec![[0.0f64; 3]; selected.len()];
        let mut areas = vec![0u64; selected.len()];
        for (index, label) in self.labels.iter_mut().enumerate() {
            if *label == 0 {
                continue;
            }
            let slot = slots[*label as usize];
            if slot == u32::MAX {
                *label = 0;
                continue;
            }
            *label = slot + 1;
            let x = index % width;
            let y = index / width % height;
            let z = index / (width * height);
            let sum = &mut sums[slot as usize];
            sum[0] += x as f64;
            sum[1] += y as f64;
            sum[2] += z as f64;
            areas[slot as usize] += 1;
        }
        self.count = selected.len() as u32;
        sums.iter()
            .zip(&areas)
            .map(|(sum, area)| {
                let area = *area as f64;
                Component {
                    center: [
                        (sum[0] / area) as f32,
                        (sum[1] / area) as f32,
                        (sum[2] / area) as f32,
                    ],
                }
            })
            .collect()
    }
}
