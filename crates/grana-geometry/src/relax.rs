use glam::Vec3;

const SMOOTH: f32 = 0.5;
const SHRINK: f32 = 0.53;

pub fn relax(positions: &[[f32; 3]], indices: &[u32], iterations: usize) -> Vec<[f32; 3]> {
    assert!(
        indices.len().is_multiple_of(3),
        "a relaxed mesh holds whole triangles"
    );
    assert!(
        indices
            .iter()
            .all(|index| (*index as usize) < positions.len()),
        "every relaxed index addresses a vertex"
    );
    let mut points = positions.to_vec();
    if iterations == 0 || indices.is_empty() {
        return points;
    }
    let neighbours = neighbours_of(positions.len(), indices);
    let mut spread = vec![Vec3::ZERO; positions.len()];
    for _ in 0..iterations {
        for weight in [SMOOTH, -SHRINK] {
            gather(&points, &neighbours, &mut spread);
            for (point, offset) in points.iter_mut().zip(&spread) {
                *point = (Vec3::from(*point) + weight * *offset).to_array();
            }
        }
    }
    points
}

fn neighbours_of(vertices: usize, indices: &[u32]) -> Vec<Vec<u32>> {
    let mut neighbours = vec![Vec::new(); vertices];
    for triangle in indices.as_chunks::<3>().0 {
        for (from, to) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            neighbours[from as usize].push(to);
            neighbours[to as usize].push(from);
        }
    }
    for ring in &mut neighbours {
        ring.sort_unstable();
        ring.dedup();
    }
    neighbours
}

fn gather(points: &[[f32; 3]], neighbours: &[Vec<u32>], spread: &mut [Vec3]) {
    for (slot, (point, ring)) in points.iter().zip(neighbours).enumerate() {
        let average = ring.iter().fold(Vec3::ZERO, |sum, neighbour| {
            sum + Vec3::from(points[*neighbour as usize])
        }) / ring.len().max(1) as f32;
        spread[slot] = average - Vec3::from(*point);
    }
}
