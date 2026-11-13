use grana_object::{LocalizationSpec, SpotWeight};

use super::{
    FLOOR, Groups, LocalizationDepth, LocalizationFrame, LocalizationGeometry, Survey,
    localization_weight, rasterize,
};

pub struct LocalizationLayout {
    survey: Survey,
    low: [f32; 2],
    span: [f32; 2],
    blur: f32,
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    weight: SpotWeight,
}

impl LocalizationLayout {
    pub fn new(
        frames: &[LocalizationFrame<'_>],
        spec: LocalizationSpec,
        camera_pixel: f32,
    ) -> Result<Option<Self>, String> {
        assert!(
            camera_pixel.is_finite() && camera_pixel > 0.0,
            "a localization layout knows its camera pixel"
        );
        let survey = Survey::of(frames);
        if survey.localizations == 0 {
            return Ok(None);
        }
        let (low, high) = survey
            .bounds
            .ok_or_else(|| "the localizations lie at an infinite position".to_string())?;
        let span = [0, 1].map(|axis| (high[axis] - low[axis]).max(FLOOR));
        let blur = spec.blur() * 0.001;
        let pixel = spec.pixel() * 0.001;
        let geometry = rasterize(&spec, low, span, pixel, blur)?;
        let depth = survey.depth(spec.depth() * 0.001)?;
        let weight = localization_weight(frames);
        Ok(Some(Self {
            survey,
            low,
            span,
            blur,
            geometry,
            depth,
            weight,
        }))
    }

    pub fn geometry(&self) -> LocalizationGeometry {
        self.geometry
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn groups(&self) -> &Groups {
        &self.survey.groups
    }

    pub fn weight(&self) -> SpotWeight {
        self.weight
    }

    pub fn localizations(&self) -> u64 {
        self.survey.localizations
    }

    pub(super) fn survey(&self) -> &Survey {
        &self.survey
    }

    pub(super) fn low(&self) -> [f32; 2] {
        self.low
    }

    pub(super) fn span(&self) -> [f32; 2] {
        self.span
    }

    pub(super) fn blur(&self) -> f32 {
        self.blur
    }
}
