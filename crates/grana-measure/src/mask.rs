const WORD: usize = u64::BITS as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    words: Vec<u64>,
    rows: usize,
    count: u64,
}

impl Mask {
    pub fn all(rows: usize) -> Self {
        let words = rows.div_ceil(WORD);
        let mut bits = vec![u64::MAX; words];
        if let Some(last) = bits.last_mut()
            && !rows.is_multiple_of(WORD)
        {
            *last = (1u64 << (rows % WORD)) - 1;
        }
        Self {
            words: bits,
            rows,
            count: rows as u64,
        }
    }

    pub fn none(rows: usize) -> Self {
        Self {
            words: vec![0; rows.div_ceil(WORD)],
            rows,
            count: 0,
        }
    }

    pub fn of(rows: usize, mut admits: impl FnMut(usize) -> bool) -> Self {
        let mut words = vec![0u64; rows.div_ceil(WORD)];
        let mut count = 0u64;
        for (index, word) in words.iter_mut().enumerate() {
            let base = index * WORD;
            let mut bits = 0u64;
            for offset in 0..WORD {
                if base + offset < rows && admits(base + offset) {
                    bits |= 1 << offset;
                }
            }
            count += u64::from(bits.count_ones());
            *word = bits;
        }
        Self { words, rows, count }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn contains(&self, row: usize) -> bool {
        assert!(row < self.rows, "a mask answers for its own rows");
        self.words[row / WORD] & (1u64 << (row % WORD)) != 0
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words
            .iter()
            .enumerate()
            .flat_map(move |(index, word)| {
                let mut bits = *word;
                std::iter::from_fn(move || {
                    if bits == 0 {
                        return None;
                    }
                    let offset = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    Some(index * WORD + offset)
                })
            })
    }

    pub fn to_rows(&self) -> Vec<u32> {
        self.iter()
            .map(|row| u32::try_from(row).expect("a mask row fits a word"))
            .collect()
    }

    pub fn intersect(&mut self, other: &Self) {
        assert_eq!(self.rows, other.rows, "masks cover the same rows");
        let mut count = 0u64;
        for (word, other) in self.words.iter_mut().zip(&other.words) {
            *word &= *other;
            count += u64::from(word.count_ones());
        }
        self.count = count;
    }
}
