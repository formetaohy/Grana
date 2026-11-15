#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chunk {
    shape: [u32; 3],
}

impl Chunk {
    pub(crate) fn new(shape: [u32; 3]) -> Self {
        assert!(
            shape.iter().all(|axis| *axis > 0),
            "a chunk holds at least one voxel on every axis"
        );
        Self { shape }
    }

    pub(crate) fn shape(self) -> [u32; 3] {
        self.shape
    }

    pub(crate) fn voxels(self) -> usize {
        (self.shape[0] as usize) * (self.shape[1] as usize) * (self.shape[2] as usize)
    }

    pub(crate) fn stage<T: Copy + Default>(self, length: [u32; 3], region: &[T], chunk: &mut [T]) {
        assert!(
            (0..3).all(|axis| length[axis] <= self.shape[axis]),
            "a staged region lies inside its chunk"
        );
        assert_eq!(
            region.len(),
            (length[0] as usize) * (length[1] as usize) * (length[2] as usize),
            "a staged region covers every voxel of its extent"
        );
        assert_eq!(
            chunk.len(),
            self.voxels(),
            "a staged chunk holds every voxel of the chunk shape"
        );
        if (0..3).all(|axis| length[axis] == self.shape[axis]) {
            chunk.copy_from_slice(region);
            return;
        }
        chunk.fill(T::default());
        let width = length[0] as usize;
        let row = self.shape[0] as usize;
        let plane = row * self.shape[1] as usize;
        for z in 0..length[2] as usize {
            for y in 0..length[1] as usize {
                let source = width * (y + length[1] as usize * z);
                let target = row * y + plane * z;
                chunk[target..target + width].copy_from_slice(&region[source..source + width]);
            }
        }
    }
}
