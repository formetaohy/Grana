use std::collections::HashMap;

use crate::PackedMesh;

const UNIT: f32 = 65535.0;

#[derive(Clone, Copy)]
struct Edge {
    low: f32,
    high: f32,
    across: f32,
    slope: f32,
}

impl Edge {
    fn at(self, up: f32) -> f32 {
        self.across + (up - self.low) * self.slope
    }
}

#[derive(Clone, Copy)]
struct Crossing {
    middle: f32,
    low: f32,
    high: f32,
}

pub fn cross_section(
    mesh: &PackedMesh,
    object: usize,
    axis: usize,
    plane: f32,
    vertices: &mut Vec<[f32; 2]>,
) -> usize {
    let group = &mesh.groups()[object];
    let base = group.min()[axis];
    let ceiling = base + UNIT * group.scale()[axis];
    if plane < base || plane > ceiling {
        return 0;
    }
    let across = (axis + 1) % 3;
    let up = (axis + 2) % 3;
    let mut segments = Vec::new();
    for triangle in 0..group.index_count() as usize / 3 {
        let corners = mesh.triangle(object, triangle);
        let mut points = [[0.0f32; 2]; 3];
        let mut found = 0usize;
        for corner in 0..3 {
            let next = (corner + 1) % 3;
            let from = corners[corner][axis] - plane;
            let to = corners[next][axis] - plane;
            if (from < 0.0) == (to < 0.0) {
                continue;
            }
            let travel = from / (from - to);
            points[found] = [
                corners[corner][across]
                    + travel * (corners[next][across] - corners[corner][across]),
                corners[corner][up] + travel * (corners[next][up] - corners[corner][up]),
            ];
            found += 1;
        }
        if found != 2 {
            continue;
        }
        segments.push([points[0], points[1]]);
    }
    if segments.len() < 2 {
        return 0;
    }
    let mut edges = Vec::with_capacity(segments.len());
    for pair in &segments {
        let mut from = pair[0];
        let mut to = pair[1];
        if from[1] > to[1] {
            std::mem::swap(&mut from, &mut to);
        }
        if from[1] == to[1] {
            continue;
        }
        edges.push(Edge {
            low: from[1],
            high: to[1],
            across: from[0],
            slope: (to[0] - from[0]) / (to[1] - from[1]),
        });
    }
    if edges.len() < 2 {
        return 0;
    }
    let mut events = edges
        .iter()
        .flat_map(|edge| [edge.low, edge.high])
        .collect::<Vec<_>>();
    events.sort_by(f32::total_cmp);
    let reach = (ceiling - base).abs().max(1.0);
    events.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-6 * reach);
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| edges[*left].low.total_cmp(&edges[*right].low));
    let mut active = Vec::with_capacity(edges.len());
    let mut pending = 0usize;
    let mut quads = 0usize;
    let mut crossings = Vec::with_capacity(edges.len());
    for band in events.windows(2) {
        let (bottom, top) = (band[0], band[1]);
        if top <= bottom {
            continue;
        }
        while pending < order.len() && edges[order[pending]].low <= bottom {
            active.push(order[pending]);
            pending += 1;
        }
        active.retain(|index| edges[*index].high > bottom);
        if active.len() < 2 {
            continue;
        }
        crossings.clear();
        crossings.extend(active.iter().map(|index| {
            let edge = edges[*index];
            let low = edge.at(bottom);
            let high = edge.at(top);
            Crossing {
                middle: 0.5 * (low + high),
                low,
                high,
            }
        }));
        crossings.sort_by(|left, right| left.middle.total_cmp(&right.middle));
        for pair in crossings.as_chunks::<2>().0 {
            let (left, right) = (pair[0], pair[1]);
            if right.low <= left.low && right.high <= left.high {
                continue;
            }
            vertices.push([left.low, bottom]);
            vertices.push([right.low, bottom]);
            vertices.push([right.high, top]);
            vertices.push([left.high, top]);
            quads += 1;
        }
    }
    quads
}
pub fn is_closed(mesh: &PackedMesh, object: usize) -> bool {
    let group = &mesh.groups()[object];
    let first = group.first_vertex() as usize;
    let base = group.first_index() as usize;
    let wide = group.wide();
    let mut tally: HashMap<(u64, u64), u32> = HashMap::with_capacity(group.index_count() as usize);
    for triangle in 0..group.index_count() as usize / 3 {
        let corners = [0, 1, 2].map(|corner| {
            let index = if wide {
                mesh.wide()[base + triangle * 3 + corner]
            } else {
                u32::from(mesh.narrow()[base + triangle * 3 + corner])
            };
            let position = mesh.vertices()[first + index as usize].position();
            u64::from(position[0]) | u64::from(position[1]) << 16 | u64::from(position[2]) << 32
        });
        for corner in 0..3 {
            let from = corners[corner];
            let to = corners[(corner + 1) % 3];
            if from == to {
                continue;
            }
            let key = if from < to { (from, to) } else { (to, from) };
            *tally.entry(key).or_insert(0u32) += 1;
        }
    }
    tally.values().all(|count| count.is_multiple_of(2))
}
