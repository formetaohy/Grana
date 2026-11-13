use super::depth::LocalizationDepth;
use super::report::LocalizationImage;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationStack {
    depth: LocalizationDepth,
    layers: Vec<LocalizationImage>,
}

impl LocalizationStack {
    pub(super) fn new(depth: LocalizationDepth, layers: Vec<LocalizationImage>) -> Self {
        assert_eq!(
            layers.len(),
            depth.layers() as usize,
            "a localization stack paints every slice it names"
        );
        assert!(
            !layers.is_empty(),
            "a localization stack holds at least one slice"
        );
        let size = layers[0].size();
        assert!(
            layers.iter().all(|layer| layer.size() == size),
            "every slice of a localization stack covers the same raster"
        );
        Self { depth, layers }
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn size(&self) -> [u32; 2] {
        self.layers[0].size()
    }

    pub fn layers(&self) -> &[LocalizationImage] {
        &self.layers
    }

    pub fn layer(&self, index: usize) -> Option<&LocalizationImage> {
        self.layers.get(index)
    }

    pub fn count(&self) -> usize {
        self.layers.len()
    }
}
