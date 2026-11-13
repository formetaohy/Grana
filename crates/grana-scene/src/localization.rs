use grana_object::{ClusterSpec, LocalizationSpec};

use crate::drift::Drift;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalizationSource {
    pixel: f32,
    width: u32,
    height: u32,
    volume: bool,
    spec: LocalizationSpec,
    clusters: ClusterSpec,
    projected: bool,
    clustered: bool,
    applied: Drift,
}

impl LocalizationSource {
    pub fn new(
        pixel: f32,
        width: u32,
        height: u32,
        volume: bool,
        spec: LocalizationSpec,
        applied: Drift,
    ) -> Self {
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a localization table carries a positive camera pixel"
        );
        assert!(
            width > 0 && height > 0,
            "a localization table covers a positive camera field"
        );
        Self {
            pixel,
            width,
            height,
            volume,
            spec,
            clusters: ClusterSpec::default(),
            projected: false,
            clustered: false,
            applied,
        }
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn volume(&self) -> bool {
        self.volume
    }

    pub fn spec(&self) -> LocalizationSpec {
        self.spec
    }

    pub fn clusters(&self) -> ClusterSpec {
        self.clusters
    }

    pub fn projected(&self) -> bool {
        self.projected
    }

    pub fn clustered(&self) -> bool {
        self.clustered
    }

    pub fn applied(&self) -> &Drift {
        &self.applied
    }

    pub fn projected_frames(&self, time: u32, frames: u32) -> [u32; 2] {
        if self.projected {
            self.spec.fitted(frames).frames()
        } else {
            [time, time]
        }
    }

    pub fn with_spec(&self, spec: LocalizationSpec) -> Self {
        Self {
            spec,
            ..self.clone()
        }
    }

    pub fn with_projected(&self, projected: bool) -> Self {
        Self {
            projected,
            ..self.clone()
        }
    }

    pub fn with_clusters(&self, clusters: ClusterSpec) -> Self {
        Self {
            clusters,
            ..self.clone()
        }
    }

    pub fn with_clustered(&self, clustered: bool) -> Self {
        Self {
            clustered,
            ..self.clone()
        }
    }

    pub(crate) fn set_applied(&mut self, applied: Drift) {
        self.applied = applied;
    }
}
