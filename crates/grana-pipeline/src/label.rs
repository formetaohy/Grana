use std::sync::{Arc, Mutex};

use grana_object::{LabelRaster, Shape};
use grana_scene::ObjectSource;
use grana_volume::Level;

const CACHED_FRAMES: usize = 4;

struct FrameShape {
    shape: Shape,
    visible: Vec<bool>,
}

pub struct Labeling {
    source: ObjectSource,
    labels: usize,
    levels: Vec<Level>,
    title: String,
    frames: Mutex<Vec<(u32, Arc<FrameShape>)>>,
}

impl Labeling {
    pub fn new(source: ObjectSource, labels: usize, levels: Vec<Level>, title: String) -> Self {
        assert!(labels > 0, "a label raster holds at least one label");
        assert!(
            u32::try_from(labels).is_ok(),
            "a label raster numbers at most {} labels",
            u32::MAX
        );
        assert!(
            !levels.is_empty(),
            "a label raster names a stored resolution level"
        );
        Self {
            source,
            labels,
            levels,
            title,
            frames: Mutex::new(Vec::new()),
        }
    }

    pub fn labels(&self) -> usize {
        self.labels
    }

    pub fn properties(&self) -> Vec<(u32, String)> {
        (1..=self.labels)
            .map(|label| {
                let number =
                    u32::try_from(label).expect("a label raster numbers its labels within a u32");
                (number, format!("{} {label}", self.title))
            })
            .collect()
    }

    pub fn fill(
        &self,
        level: usize,
        time: u32,
        origin: [u32; 3],
        extent: [u32; 3],
        data: &mut [u32],
    ) -> Result<(), String> {
        let held = self.frame(time)?;
        let level =
            self.levels.get(level).copied().ok_or_else(|| {
                "the label image names a level the dataset does not hold".to_string()
            })?;
        let raster = LabelRaster::new(&held.shape, level);
        let rendered = raster.render_visible(
            Some(&held.visible),
            origin[0]..origin[0] + extent[0],
            origin[1]..origin[1] + extent[1],
            origin[2]..origin[2] + extent[2],
        );
        data.copy_from_slice(&rendered);
        Ok(())
    }

    fn frame(&self, time: u32) -> Result<Arc<FrameShape>, String> {
        let mut frames = self
            .frames
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((_, held)) = frames.iter().find(|(frame, _)| *frame == time) {
            return Ok(Arc::clone(held));
        }
        let frame = self
            .source
            .frame(time)
            .ok_or_else(|| format!("the object set holds no objects at frame {time}"))?;
        let visible = (0..frame.object_count())
            .map(|index| self.source.visible_object(time, index))
            .collect::<Vec<_>>();
        let held = Arc::new(FrameShape {
            shape: frame.shape().map_err(|error| error.to_string())?,
            visible,
        });
        if frames.len() == CACHED_FRAMES {
            frames.remove(0);
        }
        frames.push((time, Arc::clone(&held)));
        Ok(held)
    }
}
