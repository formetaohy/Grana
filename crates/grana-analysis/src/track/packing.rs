use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Entry {
    pub(super) nodes: [u32; 3],
    pub(super) count: u8,
    pub(super) weight: f64,
}

const SEARCH_LIMIT: usize = 1 << 15;

pub(super) struct Incidence {
    incidents: Vec<Vec<u32>>,
}

impl Incidence {
    pub(super) fn new(entries: &[Entry], node_count: usize) -> Self {
        let mut incidents = vec![Vec::new(); node_count];
        for (index, entry) in entries.iter().enumerate() {
            if entry.weight <= 0.0 {
                continue;
            }
            for node in &entry.nodes[..entry.count as usize] {
                incidents[*node as usize].push(index as u32);
            }
        }
        Self { incidents }
    }

    pub(super) fn pack(
        &self,
        entries: &[Entry],
        members: &[usize],
        node_count: usize,
    ) -> Vec<usize> {
        if members.is_empty() || node_count == 0 {
            return Vec::new();
        }
        solve(entries, &self.incidents, members, node_count)
    }
}

pub(super) fn components(entries: &[Entry], node_count: usize) -> Vec<Vec<usize>> {
    if entries.is_empty() || node_count == 0 {
        return Vec::new();
    }
    let mut roots = (0..node_count).collect::<Vec<usize>>();
    for entry in entries {
        let root = find(&mut roots, entry.nodes[0] as usize);
        for node in &entry.nodes[1..entry.count as usize] {
            let other = find(&mut roots, *node as usize);
            if other != root {
                roots[other] = root;
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        let root = find(&mut roots, entry.nodes[0] as usize);
        groups.entry(root).or_default().push(index);
    }
    groups.into_values().collect()
}

fn find(roots: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while roots[root] != root {
        root = roots[root];
    }
    let mut walk = index;
    while roots[walk] != walk {
        let next = roots[walk];
        roots[walk] = root;
        walk = next;
    }
    root
}

fn solve(
    entries: &[Entry],
    incidents: &[Vec<u32>],
    members: &[usize],
    node_count: usize,
) -> Vec<usize> {
    let mut nodes = members
        .iter()
        .filter(|index| entries[**index].weight > 0.0)
        .flat_map(|index| entries[*index].nodes[..entries[*index].count as usize].iter())
        .map(|node| *node as usize)
        .collect::<Vec<_>>();
    nodes.sort_unstable();
    nodes.dedup();
    let mut search = Search {
        entries,
        incidents,
        nodes: &nodes,
        used: vec![false; node_count],
        blocked: vec![false; entries.len()],
        taken: Vec::new(),
        best: Vec::new(),
        best_score: 0.0,
        steps: 0,
        exhausted: false,
    };
    search.branch(0, 0.0);
    if search.exhausted {
        let greedy = greedy(entries, members, node_count);
        if greedy.1 > search.best_score {
            return greedy.0;
        }
    }
    search.best
}

fn greedy(entries: &[Entry], members: &[usize], node_count: usize) -> (Vec<usize>, f64) {
    let mut order = members.to_vec();
    order.sort_unstable_by(|left, right| {
        entries[*right]
            .weight
            .total_cmp(&entries[*left].weight)
            .then(left.cmp(right))
    });
    let mut used = vec![false; node_count];
    let mut taken = Vec::new();
    let mut score = 0.0;
    for index in order {
        let entry = &entries[index];
        if entry.weight <= 0.0 {
            continue;
        }
        if entry.nodes[..entry.count as usize]
            .iter()
            .any(|node| used[*node as usize])
        {
            continue;
        }
        for slot in 0..entry.count as usize {
            used[entry.nodes[slot] as usize] = true;
        }
        score += entry.weight;
        taken.push(index);
    }
    (taken, score)
}

struct Search<'a> {
    entries: &'a [Entry],
    incidents: &'a [Vec<u32>],
    nodes: &'a [usize],
    used: Vec<bool>,
    blocked: Vec<bool>,
    taken: Vec<usize>,
    best: Vec<usize>,
    best_score: f64,
    steps: usize,
    exhausted: bool,
}

impl Search<'_> {
    fn branch(&mut self, cursor: usize, score: f64) {
        if self.exhausted {
            return;
        }
        self.steps += 1;
        if self.steps > SEARCH_LIMIT {
            self.exhausted = true;
            return;
        }
        if score > self.best_score {
            self.best_score = score;
            self.best = self.taken.clone();
        }
        let mut position = cursor;
        while position < self.nodes.len() && self.used[self.nodes[position]] {
            position += 1;
        }
        if position == self.nodes.len() {
            return;
        }
        let node = self.nodes[position];
        let incident = self.incidents[node].clone();
        let mut marked = Vec::new();
        for index in &incident {
            let index = *index as usize;
            if !self.blocked[index] {
                self.blocked[index] = true;
                marked.push(index);
            }
        }
        self.branch(position + 1, score);
        for index in marked {
            self.blocked[index] = false;
        }
        for index in incident {
            self.take(index as usize, position, score);
            if self.exhausted {
                return;
            }
        }
    }

    fn take(&mut self, index: usize, position: usize, score: f64) {
        if self.blocked[index] {
            return;
        }
        let entry = self.entries[index];
        let count = entry.count as usize;
        if entry.nodes[..count]
            .iter()
            .any(|node| self.used[*node as usize])
        {
            return;
        }
        for slot in 0..count {
            self.used[entry.nodes[slot] as usize] = true;
        }
        self.taken.push(index);
        self.branch(position + 1, score + entry.weight);
        self.taken.pop();
        for slot in 0..count {
            self.used[entry.nodes[slot] as usize] = false;
        }
    }
}
