use glam::Vec3;

const LEAF: usize = 8;
const DEPTH: usize = 128;
const PARALLEL: f32 = 1.0e-12;
const REACH: f32 = 1.0e-6;
const PROBE: [f32; 3] = [0.7231, -0.4127, 0.5533];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    Triangle([Vec3; 3]),
    Sphere { center: Vec3, radius: f32 },
}

impl Primitive {
    pub fn triangle(points: [Vec3; 3]) -> Self {
        assert!(
            points.iter().all(|point| point.is_finite()),
            "a primitive triangle is finite"
        );
        Self::Triangle(points)
    }

    pub fn sphere(center: Vec3, radius: f32) -> Self {
        assert!(
            center.is_finite() && radius.is_finite() && radius > 0.0,
            "a primitive sphere is a positive finite sphere"
        );
        Self::Sphere { center, radius }
    }

    pub fn distance(self, point: Vec3) -> f32 {
        match self {
            Self::Triangle(points) => point.distance(closest(points, point)),
            Self::Sphere { center, radius } => point.distance(center) - radius,
        }
        .max(0.0)
    }

    fn extent(self) -> (Vec3, Vec3) {
        match self {
            Self::Triangle(points) => points.iter().copied().fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(low, high), point| (low.min(point), high.max(point)),
            ),
            Self::Sphere { center, radius } => {
                let reach = Vec3::splat(radius);
                (center - reach, center + reach)
            }
        }
    }

    fn centre(self) -> Vec3 {
        match self {
            Self::Triangle(points) => (points[0] + points[1] + points[2]) / 3.0,
            Self::Sphere { center, .. } => center,
        }
    }

    fn crossings(self, origin: Vec3, direction: Vec3) -> u32 {
        match self {
            Self::Triangle(points) => u32::from(triangle_ray(points, origin, direction).is_some()),
            Self::Sphere { center, radius } => {
                let offset = origin - center;
                let b = offset.dot(direction);
                let c = offset.length_squared() - radius * radius;
                let discriminant = b * b - c;
                if discriminant <= 0.0 {
                    return 0;
                }
                let root = discriminant.sqrt();
                u32::from(-b - root > REACH) + u32::from(-b + root > REACH)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    low: Vec3,
    high: Vec3,
    start: u32,
    count: u32,
    right: u32,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    object: u32,
    primitive: Primitive,
}

pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    entries: Vec<Entry>,
}

impl Bvh {
    pub fn new(primitives: impl IntoIterator<Item = (u32, Primitive)>) -> Self {
        let entries = primitives
            .into_iter()
            .map(|(object, primitive)| Entry { object, primitive })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Self {
                nodes: Vec::new(),
                order: Vec::new(),
                entries,
            };
        }
        let mut order = (0..entries.len() as u32).collect::<Vec<_>>();
        let centres = entries
            .iter()
            .map(|entry| entry.primitive.centre())
            .collect::<Vec<_>>();
        let mut nodes = Vec::new();
        Self::build(&mut order, 0, &entries, &centres, &mut nodes);
        Self {
            nodes,
            order,
            entries,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn primitives(&self) -> usize {
        self.entries.len()
    }

    pub fn nearest(&self, point: Vec3) -> Option<(f32, u32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best = f32::INFINITY;
        let mut object = 0;
        let mut stack = [0u32; DEPTH];
        let mut depth = 1usize;
        stack[0] = 0;
        while depth > 0 {
            depth -= 1;
            let index = stack[depth];
            let node = self.nodes[index as usize];
            if box_distance(node.low, node.high, point) >= best {
                continue;
            }
            if node.count > 0 {
                for slot in node.start..node.start + node.count {
                    let entry = self.entries[self.order[slot as usize] as usize];
                    let distance = entry.primitive.distance(point);
                    if distance < best {
                        best = distance;
                        object = entry.object;
                    }
                }
                continue;
            }
            push(&mut stack, &mut depth, index + 1);
            push(&mut stack, &mut depth, node.right);
        }
        best.is_finite().then_some((best, object))
    }

    pub fn contains(&self, point: Vec3) -> Option<u32> {
        let node = self.nodes.first()?;
        if (0..3).any(|axis| point[axis] < node.low[axis] || point[axis] > node.high[axis]) {
            return None;
        }
        let direction = Vec3::from(PROBE).normalize();
        let mut counts = [(0u32, 0u32); 8];
        let mut used = 0usize;
        let mut stack = [0u32; DEPTH];
        let mut depth = 1usize;
        stack[0] = 0;
        while depth > 0 {
            depth -= 1;
            let index = stack[depth];
            let node = self.nodes[index as usize];
            if !box_crossed(node.low, node.high, point, direction) {
                continue;
            }
            if node.count > 0 {
                for slot in node.start..node.start + node.count {
                    let entry = self.entries[self.order[slot as usize] as usize];
                    let crossings = entry.primitive.crossings(point, direction);
                    if crossings == 0 {
                        continue;
                    }
                    let Some(existing) = counts[..used]
                        .iter_mut()
                        .find(|count| count.0 == entry.object)
                    else {
                        assert!(used < counts.len(), "a probe meets few objects at once");
                        counts[used] = (entry.object, crossings);
                        used += 1;
                        continue;
                    };
                    existing.1 += crossings;
                }
                continue;
            }
            push(&mut stack, &mut depth, index + 1);
            push(&mut stack, &mut depth, node.right);
        }
        counts[..used]
            .iter()
            .find(|count| count.1 % 2 == 1)
            .map(|count| count.0)
    }

    fn build(
        order: &mut [u32],
        base: usize,
        entries: &[Entry],
        centres: &[Vec3],
        nodes: &mut Vec<Node>,
    ) -> u32 {
        let index = nodes.len() as u32;
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        let mut centre_low = Vec3::splat(f32::INFINITY);
        let mut centre_high = Vec3::splat(f32::NEG_INFINITY);
        for slot in order.iter() {
            let entry = entries[*slot as usize];
            let (entry_low, entry_high) = entry.primitive.extent();
            low = low.min(entry_low);
            high = high.max(entry_high);
            let centre = centres[*slot as usize];
            centre_low = centre_low.min(centre);
            centre_high = centre_high.max(centre);
        }
        let spread = centre_high - centre_low;
        let axis = if spread.x >= spread.y && spread.x >= spread.z {
            0
        } else if spread.y >= spread.z {
            1
        } else {
            2
        };
        if order.len() <= LEAF || spread[axis] <= 0.0 {
            nodes.push(Node {
                low,
                high,
                start: base as u32,
                count: order.len() as u32,
                right: 0,
            });
            return index;
        }
        order.sort_unstable_by(|left, right| {
            centres[*left as usize][axis].total_cmp(&centres[*right as usize][axis])
        });
        let middle = order.len() / 2;
        nodes.push(Node {
            low,
            high,
            start: 0,
            count: 0,
            right: 0,
        });
        let (left, right) = order.split_at_mut(middle);
        Self::build(left, base, entries, centres, nodes);
        let right_index = Self::build(right, base + middle, entries, centres, nodes);
        nodes[index as usize].right = right_index;
        index
    }
}

fn push(stack: &mut [u32; DEPTH], depth: &mut usize, index: u32) {
    assert!(*depth < DEPTH, "the tree is shallower than its stack");
    stack[*depth] = index;
    *depth += 1;
}

fn closest(points: [Vec3; 3], point: Vec3) -> Vec3 {
    let [first, second, third] = points;
    let across = second - first;
    let down = third - first;
    let offset = point - first;
    let d1 = across.dot(offset);
    let d2 = down.dot(offset);
    if d1 <= 0.0 && d2 <= 0.0 {
        return first;
    }
    let offset = point - second;
    let d3 = across.dot(offset);
    let d4 = down.dot(offset);
    if d3 >= 0.0 && d4 <= d3 {
        return second;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return first + across * (d1 / (d1 - d3));
    }
    let offset = point - third;
    let d5 = across.dot(offset);
    let d6 = down.dot(offset);
    if d6 >= 0.0 && d5 <= d6 {
        return third;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return first + down * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return second + (third - second) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let total = 1.0 / (va + vb + vc);
    first + across * (vb * total) + down * (vc * total)
}

fn triangle_ray(points: [Vec3; 3], origin: Vec3, direction: Vec3) -> Option<f32> {
    let [first, second, third] = points;
    let edge = second - first;
    let across = third - first;
    let normal = direction.cross(across);
    let determinant = edge.dot(normal);
    if determinant.abs() < PARALLEL {
        return None;
    }
    let inverse = 1.0 / determinant;
    let offset = origin - first;
    let down = offset.dot(normal) * inverse;
    if !(0.0..=1.0).contains(&down) {
        return None;
    }
    let crossing = offset.cross(edge);
    let up = direction.dot(crossing) * inverse;
    if up < 0.0 || down + up > 1.0 {
        return None;
    }
    let distance = across.dot(crossing) * inverse;
    (distance > REACH).then_some(distance)
}

fn box_distance(low: Vec3, high: Vec3, point: Vec3) -> f32 {
    let offset = (low - point).max(point - high).max(Vec3::ZERO);
    offset.length()
}

fn box_crossed(low: Vec3, high: Vec3, origin: Vec3, direction: Vec3) -> bool {
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in 0..3 {
        let step = direction[axis];
        if step.abs() < PARALLEL {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return false;
            }
            continue;
        }
        let inverse = 1.0 / step;
        let first = (low[axis] - origin[axis]) * inverse;
        let second = (high[axis] - origin[axis]) * inverse;
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    exit >= entry.max(0.0)
}
