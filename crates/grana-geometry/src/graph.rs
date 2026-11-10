use crate::{MeshVertex, push_sphere, push_tube};

pub fn push_graph(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    positions: &[[f32; 3]],
    radii: &[f32],
    edges: &[[u32; 2]],
    object: u32,
) -> usize {
    assert!(!positions.is_empty(), "a graph carries nodes");
    assert_eq!(
        positions.len(),
        radii.len(),
        "a graph carries one radius per node"
    );
    assert!(
        positions.iter().flatten().all(|value| value.is_finite())
            && radii
                .iter()
                .all(|radius| radius.is_finite() && *radius > 0.0),
        "a graph is finite and positively thick"
    );
    assert!(
        edges
            .iter()
            .flatten()
            .all(|node| (*node as usize) < positions.len()),
        "a graph edge names one of its nodes"
    );
    let mut incident = vec![Vec::new(); positions.len()];
    for (index, edge) in edges.iter().enumerate() {
        assert!(
            edge[0] != edge[1] && positions[edge[0] as usize] != positions[edge[1] as usize],
            "a graph edge joins two distinct nodes"
        );
        incident[edge[0] as usize].push((edge[1], index as u32));
        incident[edge[1] as usize].push((edge[0], index as u32));
    }
    let mut used = vec![false; edges.len()];
    let mut branches = 0;
    for node in 0..positions.len() {
        if incident[node].len() == 2 {
            continue;
        }
        for (neighbour, edge) in incident[node].clone() {
            if used[edge as usize] {
                continue;
            }
            let path = walk(
                &incident,
                positions,
                radii,
                &mut used,
                node as u32,
                neighbour,
                edge,
            );
            push_tube(vertices, indices, &path.points, &path.radii, object);
            branches += 1;
        }
    }
    for edge in 0..edges.len() {
        if used[edge] {
            continue;
        }
        let path = walk(
            &incident,
            positions,
            radii,
            &mut used,
            edges[edge][0],
            edges[edge][1],
            edge as u32,
        );
        push_tube(vertices, indices, &path.points, &path.radii, object);
        branches += 1;
    }
    for node in 0..positions.len() {
        if incident[node].len() >= 3 {
            push_sphere(vertices, indices, positions[node], radii[node], object);
        }
    }
    branches
}

struct Path {
    points: Vec<[f32; 3]>,
    radii: Vec<f32>,
}

fn walk(
    incident: &[Vec<(u32, u32)>],
    positions: &[[f32; 3]],
    radii: &[f32],
    used: &mut [bool],
    start: u32,
    first: u32,
    edge: u32,
) -> Path {
    let mut path = Path {
        points: vec![positions[start as usize]],
        radii: vec![radii[start as usize]],
    };
    let mut current = start;
    let mut next = first;
    let mut held = edge;
    loop {
        used[held as usize] = true;
        path.points.push(positions[next as usize]);
        path.radii.push(radii[next as usize]);
        if incident[next as usize].len() != 2 {
            return path;
        }
        let Some((following, forward)) = incident[next as usize]
            .iter()
            .copied()
            .find(|(node, index)| *node != current && !used[*index as usize])
        else {
            return path;
        };
        current = next;
        next = following;
        held = forward;
    }
}

pub struct Ends {
    pub terminals: u32,
    pub junctions: u32,
}

pub fn ends(edges: &[[u32; 2]], nodes: usize) -> Ends {
    let mut degree = vec![0u32; nodes];
    for edge in edges {
        degree[edge[0] as usize] += 1;
        degree[edge[1] as usize] += 1;
    }
    Ends {
        terminals: degree.iter().filter(|degree| **degree == 1).count() as u32,
        junctions: degree.iter().filter(|degree| **degree >= 3).count() as u32,
    }
}
