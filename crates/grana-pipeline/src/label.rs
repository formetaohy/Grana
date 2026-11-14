use grana_object::{LabelRaster, Shape};
use grana_scene::ObjectSource;
use grana_volume::Level;

pub struct Labeling {
    source: ObjectSource,
    labels: usize,
    levels: Vec<Level>,
    title: String,
    cached: Option<(u32, Shape, Vec<bool>)>,
}

impl Labeling {
    pub fn new(source: ObjectSource, labels: usize, levels: Vec<Level>, title: String) -> Self {
        assert!(labels > 0, "a label raster holds at least one label");
        assert!(
            !levels.is_empty(),
            "a label raster names a stored resolution level"
        );
        Self {
            source,
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
        if self.cached.as_ref().map(|(cached, ..)| *cached) != Some(time) {
            let frame = self
                .source
                .frame(time)
                .ok_or_else(|| format!("the object set holds no objects at frame {time}"))?;
            let shape = frame.shape().map_err(|error| error.to_string())?;
            let visible = (0..frame.object_count())
                .map(|index| self.source.visible_object(time, index))
                .collect::<Vec<_>>();
            self.cached = Some((time, shape, visible));
        }
        let (_, shape, visible) = self.cached.as_ref().expect("the frame shape is cached");
        let level =
            self.levels.get(level).copied().ok_or_else(|| {
                "the label image names a level the dataset does not hold".to_string()
            })?;
        let raster = LabelRaster::new(shape, level);
        let rendered = raster.render_visible(
            Some(visible),
            origin[0]..origin[0] + extent[0],
            origin[1]..origin[1] + extent[1],
            origin[2]..origin[2] + extent[2],
        );
        data.copy_from_slice(&rendered);
        Ok(())
    }
}
