#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pearson {
    count: u64,
    sum_a: f64,
    sum_b: f64,
    sum_aa: f64,
    sum_bb: f64,
    sum_ab: f64,
}

impl Pearson {
    pub fn from_sums(count: u64, sums: [f64; 5]) -> Self {
        let [sum_a, sum_b, sum_aa, sum_bb, sum_ab] = sums;
        assert!(
            sums.iter().all(|value| value.is_finite()),
            "a pair of channels accumulates finite sums"
        );
        Self {
            count,
            sum_a,
            sum_b,
            sum_aa,
            sum_bb,
            sum_ab,
        }
    }

    pub fn count(self) -> u64 {
        self.count
    }

    pub fn sums(self) -> [f64; 5] {
        [
            self.sum_a,
            self.sum_b,
            self.sum_aa,
            self.sum_bb,
            self.sum_ab,
        ]
    }

    pub fn push(&mut self, a: f64, b: f64) {
        self.count += 1;
        self.sum_a += a;
        self.sum_b += b;
        self.sum_aa += a * a;
        self.sum_bb += b * b;
        self.sum_ab += a * b;
    }

    pub fn merge(&mut self, other: &Self) {
        self.count += other.count;
        self.sum_a += other.sum_a;
        self.sum_b += other.sum_b;
        self.sum_aa += other.sum_aa;
        self.sum_bb += other.sum_bb;
        self.sum_ab += other.sum_ab;
    }

    pub fn value(self) -> Option<f64> {
        let count = self.count as f64;
        let left = count * self.sum_aa - self.sum_a * self.sum_a;
        let right = count * self.sum_bb - self.sum_b * self.sum_b;
        if count <= 0.0 || left <= 0.0 || right <= 0.0 {
            return None;
        }
        let numerator = count * self.sum_ab - self.sum_a * self.sum_b;
        Some((numerator / (left * right).sqrt()).clamp(-1.0, 1.0))
    }
}
