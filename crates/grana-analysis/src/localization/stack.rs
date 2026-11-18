use super::depth::LocalizationDepth;
use super::preview::LocalizationPreview;
use super::report::{LocalizationGeometry, LocalizationImage};

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationStack {
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    layers: Vec<LocalizationImage>,
}

impl LocalizationStack {
    pub(super) fn new(
        geometry: LocalizationGeometry,
        depth: LocalizationDepth,
        layers: Vec<LocalizationImage>,
    ) -> Self {
        assert_eq!(
            layers.len(),
            depth.layers() as usize,
            "a localization stack paints every slice it names"
        );
        assert!(
            !layers.is_empty(),
            "a localization stack holds at least one slice"
        );
        assert!(
            layers.iter().all(|layer| layer.size() == geometry.size()),
            "every slice of a localization stack covers the raster it is placed on"
        );
        Self {
            geometry,
            depth,
            layers,
        }
    }

    pub fn geometry(&self) -> LocalizationGeometry {
        self.geometry
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn size(&self) -> [u32; 2] {
        self.geometry.size()
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

    pub fn preview(&self, layer: usize) -> Option<LocalizationPreview> {
        let image = self.layer(layer)?;
        let size = self.geometry.size();
        Some(LocalizationPreview::of_bitmap(
            self.geometry.origin(),
            [
                size[0] as f32 * self.geometry.pixel(),
                size[1] as f32 * self.geometry.pixel(),
            ],
            size,
            self.depth,
            u32::try_from(layer).expect("a localization stack holds fewer than 4G slices"),
            image.shared(),
        ))
    }
}
