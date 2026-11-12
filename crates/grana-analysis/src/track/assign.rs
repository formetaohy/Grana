use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, Ordering as Memory};

const CHECK: usize = 256;
const EPSILON: f64 = 1.0e-12;

#[derive(Clone, Copy)]
pub(super) struct Edge {
    pub column: usize,
    pub cost: f64,
}

#[derive(PartialEq)]
struct Candidate {
    value: f64,
    column: usize,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value
            .total_cmp(&other.value)
            .then_with(|| self.column.cmp(&other.column))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) fn solve(
    edges: &[Vec<Edge>],
    columns: usize,
    cancel: &AtomicBool,
) -> Option<Vec<usize>> {
    let rows = edges.len();
    assert!(rows <= columns, "every row finds a column of its own");
    let mut row_potential = vec![0.0f64; rows + 1];
    let mut column_potential = vec![0.0f64; columns + 1];
    let mut owner = vec![0usize; columns + 1];
    let mut trace = vec![0usize; columns + 1];
    let mut distance = vec![0.0f64; columns + 1];
    let mut stamp = vec![0.0f64; columns + 1];
    let mut version = vec![0u32; columns + 1];
    let mut used = vec![0u32; columns + 1];
    let mut used_owner = vec![0usize; columns + 1];
    let mut used_total = vec![0.0f64; columns + 1];
    let mut visited = Vec::new();
    let mut heap = BinaryHeap::new();
    let mut round = 0u32;
    for row in 1..=rows {
        if row.is_multiple_of(CHECK) && cancel.load(Memory::Relaxed) {
            return None;
        }
        round += 1;
        visited.clear();
        heap.clear();
        owner[0] = row;
        let mut column = 0usize;
        let mut total = 0.0f64;
        loop {
            used[column] = round;
            used_owner[column] = owner[column];
            used_total[column] = total;
            visited.push(column);
            let active = owner[column];
            for edge in &edges[active - 1] {
                let target = edge.column + 1;
                if used[target] == round {
                    continue;
                }
                let reduced = edge.cost - row_potential[active] - column_potential[target];
                let current = if version[target] == round {
                    distance[target] - (total - stamp[target])
                } else {
                    f64::INFINITY
                };
                if reduced < current - EPSILON {
                    version[target] = round;
                    distance[target] = reduced;
                    stamp[target] = total;
                    trace[target] = column;
                    heap.push(Reverse(Candidate {
                        value: reduced + total,
                        column: target,
                    }));
                }
            }
            let next = loop {
                let Some(Reverse(candidate)) = heap.pop() else {
                    panic!("a row always reaches its own free column");
                };
                if used[candidate.column] == round || version[candidate.column] != round {
                    continue;
                }
                if candidate.value > distance[candidate.column] + stamp[candidate.column] + EPSILON
                {
                    continue;
                }
                break candidate.column;
            };
            total += (distance[next] + stamp[next] - total).max(0.0);
            column = next;
            if owner[column] == 0 {
                break;
            }
        }
        for &column in &visited {
            let correction = total - used_total[column];
            row_potential[used_owner[column]] += correction;
            column_potential[column] -= correction;
        }
        loop {
            let previous = trace[column];
            owner[column] = owner[previous];
            column = previous;
            if column == 0 {
                break;
            }
        }
    }
    let mut assignment = vec![usize::MAX; rows];
    for (column, &row) in owner.iter().enumerate().skip(1) {
        if row != 0 {
            assignment[row - 1] = column - 1;
        }
    }
    assert!(
        assignment.iter().all(|column| *column != usize::MAX),
        "every row leaves the solver assigned"
    );
    Some(assignment)
}
