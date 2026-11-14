pub(super) struct Decimator<T> {
    values: Vec<T>,
    stride: usize,
    limit: usize,
}

impl<T> Decimator<T> {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            values: Vec::with_capacity(limit + 1),
            stride: 1,
            limit,
        }
    }

    pub(super) fn push(&mut self, index: usize, value: T) {
        if !index.is_multiple_of(self.stride) {
            return;
        }
        self.values.push(value);
        if self.values.len() > self.limit {
            self.stride *= 2;
            let held = std::mem::take(&mut self.values);
            self.values = held
                .into_iter()
                .enumerate()
                .filter(|(position, _)| position.is_multiple_of(2))
                .map(|(_, value)| value)
                .collect();
        }
    }

    pub(super) fn into_values(self) -> Vec<T> {
        self.values
    }
}
