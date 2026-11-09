use std::sync::Arc;

use grana_object::{LabelRaster, Shape};
use grana_scene::ObjectFrame;
use grana_volume::Level;

pub struct Labeling {
    shapes: Vec<Option<Arc<ObjectFrame>>>,
    visible: Vec<Vec<bool>>,
    labels: usize,
    levels: Vec<Level>,
    title: String,
    cached: Option<(u32, Shape)>,
}

impl Labeling {
    pub fn new(
        shapes: Vec<Option<Arc<ObjectFrame>>>,
        visible: Vec<Vec<bool>>,
        labels: usize,
        levels: Vec<Level>,
        title: String,
    ) -> Self {
        assert!(labels > 0, "a label raster holds at least one label");
        assert!(
            !levels.is_empty(),
            "a label raster names a stored resolution level"
        );
        Self {
            shapes,
            visible,
            labels,
            levels,
            title,
            cached: None,
        }
    }

    pub fn labels(&self) -> usize {
        self.labels
    }

    pub fn properties(&self) -> Vec<(u32, String)> {
        (1..=self.labels)
            .map(|label| (label as u32, format!("{} {label}", self.title)))
            .collect()
    }

    pub fn fill(
        &mut self,
        level: usize,
        time: u32,
        origin: [u32; 3],
        extent: [u32; 3],
        data: &mut [u16],
    ) -> Result<(), String> {
        if self.cached.as_ref().map(|(cached, _)| *cached) != Some(time) {
            let frame = self
                .shapes
                .get(time as usize)
                .and_then(|frame| frame.as_ref())
                .ok_or_else(|| format!("the object set holds no objects at frame {time}"))?;
            let shape = frame.shape().map_err(|error| error.to_string())?;
            self.cached = Some((time, shape));
        }
        let shape = &self.cached.as_ref().expect("the frame shape is cached").1;
        let level =
            self.levels.get(level).copied().ok_or_else(|| {
                "the label image names a level the dataset does not hold".to_string()
            })?;
        let raster = LabelRaster::new(shape, level);
        let keep = self.visible.get(time as usize).map(Vec::as_slice);
        let rendered = raster.render_visible(
            keep,
            origin[0]..origin[0] + extent[0],
            origin[1]..origin[1] + extent[1],
            origin[2]..origin[2] + extent[2],
        );
        data.copy_from_slice(&rendered);
        Ok(())
    }
}
