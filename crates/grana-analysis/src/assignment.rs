const REJECTED: f64 = 1.0e9;

pub(crate) fn rejected() -> f64 {
    REJECTED
}

pub(crate) fn assign(cost: &[Vec<f64>], columns: usize) -> Vec<usize> {
    let rows = cost.len();
    assert!(rows <= columns, "every assigned row needs its own column");
    assert!(
        cost.iter()
            .all(|row| row.len() == columns && row.iter().all(|value| value.is_finite())),
        "the cost matrix is finite and rectangular"
    );
    if rows == 0 {
        return Vec::new();
    }
    let mut u = vec![0.0; rows + 1];
    let mut v = vec![0.0; columns + 1];
    let mut owner = vec![0usize; columns + 1];
    let mut trace = vec![0usize; columns + 1];
    for row in 1..=rows {
        owner[0] = row;
        let mut column = 0;
        let mut potential = vec![f64::INFINITY; columns + 1];
        let mut used = vec![false; columns + 1];
        loop {
            used[column] = true;
            let active = owner[column];
            let mut delta = f64::INFINITY;
            let mut next = 0;
            for candidate in 1..=columns {
                if used[candidate] {
                    continue;
                }
                let current = cost[active - 1][candidate - 1] - u[active] - v[candidate];
                if current < potential[candidate] {
                    potential[candidate] = current;
                    trace[candidate] = column;
                }
                if potential[candidate] < delta {
                    delta = potential[candidate];
                    next = candidate;
                }
            }
            assert!(next != 0, "every row finds a free column");
            for candidate in 0..=columns {
                if used[candidate] {
                    u[owner[candidate]] += delta;
                    v[candidate] -= delta;
                } else {
                    potential[candidate] -= delta;
                }
            }
            column = next;
            if owner[column] == 0 {
                break;
            }
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
    for (column, row) in owner.iter().enumerate().skip(1) {
        if *row != 0 {
            assignment[row - 1] = column - 1;
        }
    }
    assignment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_the_cheapest_columns() {
        let cost = vec![
            vec![4.0, 1.0, 3.0],
            vec![2.0, 0.0, 5.0],
            vec![3.0, 2.0, 2.0],
        ];
        assert_eq!(assign(&cost, 3), vec![1, 0, 2]);
        let rectangular = vec![vec![2.0, 1.0, 4.0], vec![3.0, 4.0, 0.0]];
        assert_eq!(assign(&rectangular, 3), vec![1, 2]);
    }
}
