use glam::{DVec3, Vec3};

const LEAF: usize = 16;

const DEPTH: usize = 64;

const SUBTENSE: f64 = 0.125;

const FLOOR: f64 = 1.0e-6;

#[derive(Clone, Copy, Debug)]
struct Cluster {
    center: DVec3,
    radius: f64,
    mass: f64,
    offset: DVec3,
    spread: f64,
    right: u32,
    start: u32,
    end: u32,
}

impl Cluster {
    const EMPTY: Self = Self {
        center: DVec3::ZERO,
        radius: 0.0,
        mass: 0.0,
        offset: DVec3::ZERO,
        spread: 0.0,
        right: 0,
        start: 0,
        end: 0,
    };

    fn leaf(order: &[u32], items: &[[f32; 4]], base: u32) -> Self {
        let (low, high) = extent(order, items);
        let center = 0.5 * (low + high);
        let mut cluster = Self {
            center,
            start: base,
            end: base + order.len() as u32,
            ..Self::EMPTY
        };
        for slot in order {
            let (point, radius) = parts(items[*slot as usize]);
            let apart = point - center;
            let weight = radius * radius;
            cluster.radius = cluster.radius.max(apart.length() + radius);
            cluster.mass += weight;
            cluster.offset += weight * apart;
            cluster.spread += weight * apart.length_squared();
        }
        cluster
    }

    fn join(first: Self, second: Self, right: u32) -> Self {
        let apart = (second.center - first.center).length();
        let (center, radius) = if apart + first.radius <= second.radius {
            (second.center, second.radius)
        } else if apart + second.radius <= first.radius {
            (first.center, first.radius)
        } else {
            let radius = 0.5 * (apart + first.radius + second.radius);
            let along = (radius - first.radius) / apart;
            (
                first.center + (second.center - first.center) * along,
                radius,
            )
        };
        let first_apart = first.center - center;
        let second_apart = second.center - center;
        Self {
            center,
            radius,
            mass: first.mass + second.mass,
            offset: first.offset
                + first.mass * first_apart
                + second.offset
                + second.mass * second_apart,
            spread: first.spread
                + second.spread
                + 2.0 * (first_apart.dot(first.offset) + second_apart.dot(second.offset))
                + first.mass * first_apart.length_squared()
                + second.mass * second_apart.length_squared(),
            right,
            start: 0,
            end: 0,
        }
    }
}

pub struct SphereTree {
    clusters: Vec<Cluster>,
    items: Vec<[f32; 4]>,
}

impl SphereTree {
    pub fn new(items: impl IntoIterator<Item = [f32; 4]>) -> Self {
        let items = items.into_iter().collect::<Vec<_>>();
        assert!(
            items.iter().flatten().all(|value| value.is_finite()),
            "a sphere is finite"
        );
        assert!(
            items.iter().all(|sphere| sphere[3] >= 0.0),
            "a sphere has a non negative radius"
        );
        let mut order = (0..items.len() as u32).collect::<Vec<_>>();
        let mut clusters = Vec::new();
        if !items.is_empty() {
            Self::build(&mut order, &items, &mut clusters, 0);
        }
        let items = order.iter().map(|&slot| items[slot as usize]).collect();
        Self { clusters, items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn mass(&self) -> f64 {
        self.clusters.first().map_or(0.0, |root| root.mass)
    }

    pub fn subtense(&self, eye: Vec3) -> f64 {
        let eye = DVec3::from(eye);
        if self.clusters.is_empty() {
            return 0.0;
        }
        let mut total = 0.0;
        let mut stack = [0u32; DEPTH];
        let mut depth = 0usize;
        push(&mut stack, &mut depth, 0);
        while depth > 0 {
            depth -= 1;
            let index = stack[depth];
            let cluster = self.clusters[index as usize];
            let apart = eye - cluster.center;
            let reach = apart.length();
            if reach > 0.0 && cluster.radius <= SUBTENSE * reach {
                let inverse = 1.0 / (reach * reach);
                let dipole = 2.0 * apart.dot(cluster.offset) - cluster.spread;
                total += cluster.mass * inverse + dipole * inverse * inverse;
                continue;
            }
            if cluster.start == cluster.end {
                push(&mut stack, &mut depth, index + 1);
                push(&mut stack, &mut depth, cluster.right);
                continue;
            }
            for sphere in &self.items[cluster.start as usize..cluster.end as usize] {
                let (point, radius) = parts(*sphere);
                let distance = (eye - point).length().max(0.5 * radius).max(FLOOR);
                total += (radius / distance).powi(2);
            }
        }
        total
    }

    fn build(order: &mut [u32], items: &[[f32; 4]], clusters: &mut Vec<Cluster>, base: u32) -> u32 {
        let index = clusters.len() as u32;
        let (low, high) = extent(order, items);
        let span = high - low;
        let axis = if span.x >= span.y && span.x >= span.z {
            0
        } else if span.y >= span.z {
            1
        } else {
            2
        };
        if order.len() <= LEAF || span[axis] <= 0.0 {
            clusters.push(Cluster::leaf(order, items, base));
            return index;
        }
        let middle = order.len() / 2;
        order.select_nth_unstable_by(middle, |left, right| {
            items[*left as usize][axis].total_cmp(&items[*right as usize][axis])
        });
        clusters.push(Cluster::EMPTY);
        let (left, right) = order.split_at_mut(middle);
        Self::build(left, items, clusters, base);
        let right_index = Self::build(right, items, clusters, base + middle as u32);
        clusters[index as usize] = Cluster::join(
            clusters[index as usize + 1],
            clusters[right_index as usize],
            right_index,
        );
        index
    }
}

fn extent(order: &[u32], items: &[[f32; 4]]) -> (DVec3, DVec3) {
    let mut low = DVec3::splat(f64::INFINITY);
    let mut high = DVec3::splat(f64::NEG_INFINITY);
    for slot in order {
        let point = parts(items[*slot as usize]).0;
        low = low.min(point);
        high = high.max(point);
    }
    (low, high)
}

fn parts(sphere: [f32; 4]) -> (DVec3, f64) {
    (
        DVec3::new(
            f64::from(sphere[0]),
            f64::from(sphere[1]),
            f64::from(sphere[2]),
        ),
        f64::from(sphere[3]),
    )
}

fn push(stack: &mut [u32; DEPTH], depth: &mut usize, index: u32) {
    assert!(*depth < DEPTH, "the tree is shallower than its stack");
    stack[*depth] = index;
    *depth += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            f64::from((self.0 >> 11) as u32) / 4_294_967_296.0
        }
    }

    fn cloud(count: usize) -> Vec<[f32; 4]> {
        let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
        (0..count)
            .map(|_| {
                let center = [0, 1, 2].map(|_| (noise.next() * 40.0 - 20.0) as f32);
                [
                    center[0],
                    center[1],
                    center[2],
                    0.2 + (noise.next() * 0.5) as f32,
                ]
            })
            .collect()
    }

    fn audit(tree: &SphereTree, index: usize) -> (u32, u32) {
        let cluster = tree.clusters[index];
        let items = if cluster.start == cluster.end {
            let left = audit(tree, index + 1);
            let right = audit(tree, cluster.right as usize);
            assert_eq!(
                left.1, right.0,
                "the children of a cluster cover adjacent spheres"
            );
            (left.0, right.1)
        } else {
            (cluster.start, cluster.end)
        };
        let mut mass = 0.0;
        let mut offset = DVec3::ZERO;
        let mut spread = 0.0;
        for sphere in &tree.items[items.0 as usize..items.1 as usize] {
            let (point, radius) = parts(*sphere);
            let apart = point - cluster.center;
            assert!(
                apart.length() + radius <= cluster.radius * (1.0 + 1.0e-12),
                "a cluster bounds every sphere below it"
            );
            mass += radius * radius;
            offset += radius * radius * apart;
            spread += radius * radius * apart.length_squared();
        }
        let scale = mass.max(1.0);
        assert!((cluster.mass - mass).abs() <= 1.0e-12 * scale);
        assert!((cluster.offset - offset).length() <= 1.0e-12 * scale * cluster.radius.max(1.0));
        assert!(
            (cluster.spread - spread).abs() <= 1.0e-12 * scale * cluster.radius.powi(2).max(1.0)
        );
        items
    }

    #[test]
    fn every_cluster_bounds_and_weighs_the_spheres_below_it() {
        let spheres = cloud(3000);
        let tree = SphereTree::new(spheres.iter().copied());
        assert_eq!(audit(&tree, 0), (0, 3000));
        let mass = tree
            .items
            .iter()
            .map(|sphere| f64::from(sphere[3]).powi(2))
            .sum::<f64>();
        assert!(
            (tree.mass() - mass).abs() <= 1.0e-12 * mass,
            "the root carries the mass of every sphere once: {} against {mass}",
            tree.mass()
        );
        let mut held = spheres;
        held.sort_by_key(|sphere| sphere.map(f32::to_bits));
        let mut given = tree.items.clone();
        given.sort_by_key(|sphere| sphere.map(f32::to_bits));
        assert_eq!(held, given, "the leaves hold every sphere exactly once");
    }
}
