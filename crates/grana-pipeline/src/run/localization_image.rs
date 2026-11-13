use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::localization::{
    CompositionEntry, CompositionSpec, LocalizationComposition, LocalizationFrame,
    LocalizationLayout, LocalizationRenderer,
};
use grana_object::{LocalizationFilter, LocalizationSpec, Spot};

use super::localization_window::LocalizationWindow;
use crate::event::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Raster {
    Png,
    Stack,
}

impl Raster {
    fn of(path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("png") => Ok(Self::Png),
            Some("tif" | "tiff") => Ok(Self::Stack),
            Some(other) => Err(format!(
                "a super-resolution image writes a PNG or an OME-TIFF, not `.{other}`"
            )),
            None => Err("a super-resolution image names a file extension to write".to_string()),
        }
    }
}

pub struct LocalizationImageRequest {
    window: LocalizationWindow,
    spec: LocalizationSpec,
    pixel: f32,
    name: String,
    layer: usize,
    raster: Raster,
    path: PathBuf,
}

impl LocalizationImageRequest {
    pub fn new(
        window: LocalizationWindow,
        spec: LocalizationSpec,
        pixel: f32,
        name: &str,
        layer: usize,
        path: &Path,
    ) -> Result<Self, String> {
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a super-resolution image knows its positive camera pixel"
        );
        assert!(
            !name.trim().is_empty(),
            "a super-resolution image carries a name"
        );
        Ok(Self {
            window,
            spec,
            pixel,
            name: name.to_string(),
            layer,
            raster: Raster::of(path)?,
            path: path.to_path_buf(),
        })
    }
}

pub fn localization_image(
    request: LocalizationImageRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let LocalizationImageRequest {
        window,
        spec,
        pixel,
        name,
        layer,
        raster,
        path,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let held = frames(&window, spec.filter());
    let report = std::cell::RefCell::new(&mut *sink);
    let tick = |done: usize, total: usize| {
        (report.borrow_mut())(Event::Progress {
            time: 0,
            done,
            total,
        });
        !cancel.load(Ordering::Relaxed)
    };
    let Some(layout) = LocalizationLayout::new(&held, spec, pixel)? else {
        sink(Event::Done);
        return Ok(());
    };
    let geometry = layout.geometry();
    let depth = layout.depth();
    let mut renderer = LocalizationRenderer::new(
        &held,
        geometry,
        depth,
        layout.groups().clone(),
        layout.weight(),
        &tick,
    )?;
    renderer.survey()?;
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    let pixel = geometry.pixel();
    let slice = if depth.step() > 0.0 {
        depth.step()
    } else {
        pixel
    };
    let written = match raster {
        Raster::Png => {
            renderer.begin(layer.min(depth.layers().saturating_sub(1) as usize) as u32)?;
            grana_raster::write_stream(&path, geometry.size(), &mut |_, row| renderer.row(row))
                .map_err(|error| error.to_string())
        }
        Raster::Stack => {
            let mut current = u32::MAX;
            grana_tiff::write_stack(
                &path,
                &grana_tiff::Stack {
                    size: geometry.size(),
                    samples: 3,
                    name: &name,
                    physical: Some([pixel, pixel, slice]),
                    interval: None,
                    depth: depth.layers(),
                },
                &mut |layer, _, bytes| {
                    if layer != current {
                        renderer.begin(layer)?;
                        current = layer;
                    }
                    renderer.row_rgb(bytes)
                },
            )
            .map_err(|error| error.to_string())
        }
    };
    written?;
    sink(Event::Done);
    Ok(())
}

pub struct CompositionSource {
    window: LocalizationWindow,
    spec: LocalizationSpec,
    tint: [f32; 3],
}

impl CompositionSource {
    pub fn new(window: LocalizationWindow, spec: LocalizationSpec, tint: [f32; 3]) -> Self {
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "a composed source tints within the unit interval"
        );
        Self { window, spec, tint }
    }
}

pub struct LocalizationCompositionRequest {
    sources: Vec<CompositionSource>,
    name: String,
    path: PathBuf,
}

impl LocalizationCompositionRequest {
    pub fn new(sources: Vec<CompositionSource>, name: &str, path: &Path) -> Result<Self, String> {
        if sources.is_empty() {
            return Err("a composed image layers at least one source".to_string());
        }
        assert!(!name.trim().is_empty(), "a composed image carries a name");
        if Raster::of(path)? != Raster::Stack {
            return Err("a composed image writes an OME-TIFF".to_string());
        }
        Ok(Self {
            sources,
            name: name.to_string(),
            path: path.to_path_buf(),
        })
    }
}

pub fn localization_composition(
    request: LocalizationCompositionRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let LocalizationCompositionRequest {
        sources,
        name,
        path,
    } = request;
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 0,
    });
    let held = sources
        .iter()
        .map(|source| frames(&source.window, source.spec.filter()))
        .collect::<Vec<_>>();
    let entries = held
        .iter()
        .zip(&sources)
        .map(|(frames, source)| CompositionEntry::new(frames, source.tint))
        .collect::<Vec<_>>();
    let spec =
        CompositionSpec::covering(&sources.iter().map(|source| source.spec).collect::<Vec<_>>())
            .ok_or_else(|| "a composed image layers at least one source".to_string())?;
    let report = std::cell::RefCell::new(&mut *sink);
    let tick = |done: usize, total: usize| {
        (report.borrow_mut())(Event::Progress {
            time: 0,
            done,
            total,
        });
        !cancel.load(Ordering::Relaxed)
    };
    let mut composition = LocalizationComposition::new(&entries, spec, &tick)?;
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(());
    }
    let geometry = composition.geometry();
    let depth = composition.depth();
    let pixel = geometry.pixel();
    let slice = if depth.step() > 0.0 {
        depth.step()
    } else {
        pixel
    };
    let mut current = u32::MAX;
    grana_tiff::write_stack(
        &path,
        &grana_tiff::Stack {
            size: geometry.size(),
            samples: 3,
            name: &name,
            physical: Some([pixel, pixel, slice]),
            interval: None,
            depth: depth.layers(),
        },
        &mut |layer, _, bytes| {
            if layer != current {
                composition.begin(layer)?;
                current = layer;
            }
            composition.row_rgb(bytes)
        },
    )
    .map_err(|error| error.to_string())?;
    sink(Event::Done);
    Ok(())
}

fn frames(window: &LocalizationWindow, filter: LocalizationFilter) -> Vec<LocalizationFrame<'_>> {
    let empty: &[Spot] = &[];
    (0..window.len())
        .map(|index| {
            let spots = window
                .frame(index)
                .and_then(|frame| frame.spots())
                .map_or(empty, |set| set.spots());
            LocalizationFrame::new(spots, window.visible(index), filter)
        })
        .collect()
}
