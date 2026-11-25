pub(crate) trait Source {
    fn hollow(&self, time: u32, level: usize) -> Option<&[u64]>;
}

#[derive(Clone, Copy)]
pub(crate) struct Cursor<'a> {
    source: &'a dyn Source,
    time: u32,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(source: &'a dyn Source, time: u32) -> Self {
        Self { source, time }
    }

    pub(crate) fn mask(&self, level: usize, counts: [u32; 3]) -> Option<Mask<'a>> {
        self.source
            .hollow(self.time, level)
            .map(|words| Mask::new(words, counts))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Mask<'a> {
    words: &'a [u64],
    counts: [usize; 3],
}

impl<'a> Mask<'a> {
    fn new(words: &'a [u64], counts: [u32; 3]) -> Self {
        Self {
            words,
            counts: counts.map(|count| count as usize),
        }
    }

    pub(crate) fn holds(&self, index: usize) -> bool {
        self.words
            .get(index / u64::BITS as usize)
            .is_some_and(|word| word & (1u64 << (index % u64::BITS as usize)) != 0)
    }

    pub(crate) fn count(&self, low: [u32; 3], high: [u32; 3]) -> usize {
        let width = self.counts[0] as u32;
        let mut count = 0;
        for z in low[2]..high[2] {
            for y in low[1]..high[1] {
                let row = width * (y + self.counts[1] as u32 * z);
                count += self.row(row + low[0], row + high[0]);
            }
        }
        count
    }

    fn row(&self, start: u32, end: u32) -> usize {
        let bits = self.words.len() * u64::BITS as usize;
        let start = (start as usize).min(bits);
        let end = (end as usize).min(bits);
        if start >= end {
            return 0;
        }
        let first = start / u64::BITS as usize;
        let last = (end - 1) / u64::BITS as usize;
        if first == last {
            return word_bits(self.words[first], start % 64, end - first * 64);
        }
        let mut count = word_bits(self.words[first], start % 64, 64);
        for word in &self.words[first + 1..last] {
            count += word.count_ones() as usize;
        }
        count + word_bits(self.words[last], 0, end - last * 64)
    }
}

fn word_bits(word: u64, offset: usize, end: usize) -> usize {
    let width = end - offset;
    let mask = if width >= u64::BITS as usize {
        u64::MAX
    } else {
        ((1u64 << width) - 1) << offset
    };
    (word & mask).count_ones() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS: [u32; 3] = [5, 4, 3];

    fn index(cell: [u32; 3]) -> usize {
        (cell[0] + COUNTS[0] * (cell[1] + COUNTS[1] * cell[2])) as usize
    }

    fn hollow_set() -> Vec<bool> {
        let total = (COUNTS[0] * COUNTS[1] * COUNTS[2]) as usize;
        let mut state = 0x1234_5678u32;
        (0..total)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                state >> 31 != 0
            })
            .collect()
    }

    fn expected(hollow: &[bool], low: [u32; 3], high: [u32; 3]) -> usize {
        let mut count = 0;
        for z in low[2]..high[2] {
            for y in low[1]..high[1] {
                for x in low[0]..high[0] {
                    if hollow[index([x, y, z])] {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    #[test]
    fn counts_the_hollow_bricks_of_every_brick_range() {
        let hollow = hollow_set();
        let total = hollow.len();
        let mut words = vec![0u64; total.div_ceil(u64::BITS as usize)];
        for (index, filled) in hollow.iter().enumerate() {
            if *filled {
                words[index / u64::BITS as usize] |= 1u64 << (index % u64::BITS as usize);
            }
        }
        let mask = Mask::new(&words, COUNTS);
        for z in 0..COUNTS[2] {
            for y in 0..COUNTS[1] {
                for x in 0..COUNTS[0] {
                    let cell = [x, y, z];
                    assert_eq!(mask.holds(index(cell)), hollow[index(cell)], "{cell:?}");
                    for far_z in z + 1..=COUNTS[2] {
                        for far_y in y + 1..=COUNTS[1] {
                            for far_x in x + 1..=COUNTS[0] {
                                let (low, high) = (cell, [far_x, far_y, far_z]);
                                assert_eq!(
                                    mask.count(low, high),
                                    expected(&hollow, low, high),
                                    "{low:?} to {high:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
