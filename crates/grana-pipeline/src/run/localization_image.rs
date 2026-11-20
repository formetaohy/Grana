use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grana_analysis::localization::{
    CompositionEntry, CompositionSpec, LocalizationComposition, LocalizationDepth,
    LocalizationGeometry, LocalizationLayout, LocalizationPreview, LocalizationRenderer,
};
use grana_archive::{Block, Channel, Fill, ImageSpec};
use grana_object::LocalizationSpec;
use grana_raster::{Bitmap, Overlay};
use grana_volume::{Extent, Level, Spacing, VoxelData};

use super::localization_window::LocalizationWindow;
use crate::event::Event;

const COMPOSITION_CHANNELS: [(&str, [f32; 3]); 3] = [
    ("Red", [1.0, 0.0, 0.0]),
    ("Green", [0.0, 1.0, 0.0]),
    ("Blue", [0.0, 0.0, 1.0]),
];

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
) -> Result<Option<LocalizationPreview>, String> {
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
    let report = std::cell::RefCell::new(&mut *sink);
    let tick = |done: usize, total: usize| {
        (report.borrow_mut())(Event::Progress {
            time: 0,
            done,
            total,
        });
        !cancel.load(Ordering::Relaxed)
    };
    let Some(layout) = LocalizationLayout::new(&window, spec, pixel, &tick)? else {
        sink(Event::Done);
        return Ok(None);
    };
    let geometry = layout.geometry();
    let depth = layout.depth();
    let mut renderer = LocalizationRenderer::new(
        &window,
        spec.filter(),
        geometry,
        depth,
        layout.groups().clone(),
        layout.weight(),
        &tick,
    )?;
    renderer.survey()?;
    if cancel.load(Ordering::Relaxed) {
        sink(Event::Done);
        return Ok(None);
    }
    let pixel = geometry.pixel();
    let slice = if depth.step() > 0.0 {
        depth.step()
    } else {
        pixel
    };
    let preview = match raster {
        Raster::Png => {
            renderer.begin(layer.min(depth.layers().saturating_sub(1) as usize) as u32)?;
            grana_raster::write_stream(&path, geometry.size(), &mut |_, row| renderer.row(row))
                .map_err(|error| error.to_string())?;
            None
        }
        Raster::Stack => Some(write_stack(
            &path,
            &name,
            &mut renderer,
            geometry,
            depth,
            [pixel, pixel, slice],
            cancel,
        )?),
    };
    sink(Event::Done);
    Ok(preview)
}

pub fn localization_overlay(
    preview: &LocalizationPreview,
    tint: [f32; 3],
    opacity: f32,
) -> Overlay {
    let bitmap = Bitmap::new(preview.size(), preview.shared());
    let depth = preview.depth();
    let layer = preview.layer();
    let [width, height] = preview.extent();
    let origin = preview.origin();
    let z = 0.5 * (depth.lower(layer) + depth.upper(layer));
    let corners = [
        [origin[0], origin[1], z],
        [origin[0] + width, origin[1], z],
        [origin[0] + width, origin[1] + height, z],
        [origin[0], origin[1] + height, z],
    ];
    Overlay::new(
        bitmap,
        corners,
        grana_display::srgb_to_linear(tint),
        opacity,
    )
}

fn write_stack(
    path: &Path,
    name: &str,
    layered: &mut impl Layered,
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    spacing: [f32; 3],
    cancel: &AtomicBool,
) -> Result<LocalizationPreview, String> {
    let size = geometry.size();
    let layers = depth.layers();
    let rendered = render_stack(layered, size, layers, cancel)?;
    let pixels = rendered
        .bytes()
        .ok_or_else(|| "a rendered stack keeps its mapped pixels".to_string())?;
    let layer = layers / 2;
    let plane = size[0] as usize * 3 * size[1] as usize;
    let preview = LocalizationPreview::of_layer(
        geometry,
        depth,
        layer,
        &pixels[plane * layer as usize..plane * (layer as usize + 1)],
    );
    let levels = [Level::new(
        Extent::new(size[0], size[1], layers),
        Spacing::new(spacing[0], spacing[1], spacing[2]),
    )];
    let channels = COMPOSITION_CHANNELS.map(|(name, tint)| Channel {
        name,
        tint,
        intensity: grana_volume::Intensity::new(0.0, 1.0),
        window: grana_volume::Intensity::new(0.0, 1.0),
    });
    let fill = |block: Block| -> Result<VoxelData, String> {
        let width = size[0] as usize;
        let origin = block.origin;
        let extent = block.extent;
        let mut values = vec![0u8; block.voxels()];
        for line in 0..extent[1] as usize {
            let row =
                &pixels[((origin[2] as usize * size[1] as usize) + line + origin[1] as usize)
                    * width
                    * 3..];
            let start = line * extent[0] as usize;
            let held = &mut values[start..start + extent[0] as usize];
            for (column, value) in held.iter_mut().enumerate() {
                *value = row[(origin[0] as usize + column) * 3 + block.channel];
            }
        }
        Ok(VoxelData::U8(values))
    };
    let mut progress = |_: usize, _: usize| {};
    grana_tiff::write_archive(
        path,
        &mut ImageSpec {
            name,
            channels: &channels,
            levels: &levels,
            placement: [0.0; 3],
            frames: 1,
            interval: None,
            sample: grana_volume::VoxelFormat::U8,
            calibration: grana_volume::Calibration::Recorded,
            fill: Fill::EveryLevel(&fill),
            cancel,
            progress: &mut progress,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(preview)
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
    let entries = sources
        .iter()
        .map(|source| CompositionEntry::new(&source.window, source.spec.filter(), source.tint))
        .collect::<Vec<_>>();
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
    write_stack(
        &path,
        &name,
        &mut composition,
        geometry,
        depth,
        [pixel, pixel, slice],
        cancel,
    )?;
    sink(Event::Done);
    Ok(())
}

trait Layered {
    fn begin_layer(&mut self, layer: u32) -> Result<(), String>;

    fn compose_row(&mut self, row: &mut [u8]) -> Result<(), String>;
}

impl Layered for LocalizationRenderer<'_> {
    fn begin_layer(&mut self, layer: u32) -> Result<(), String> {
        self.begin(layer)
    }

    fn compose_row(&mut self, row: &mut [u8]) -> Result<(), String> {
        self.row_rgb(row)
    }
}

impl Layered for LocalizationComposition<'_> {
    fn begin_layer(&mut self, layer: u32) -> Result<(), String> {
        self.begin(layer)
    }

    fn compose_row(&mut self, row: &mut [u8]) -> Result<(), String> {
        self.row_rgb(row)
    }
}

fn render_stack(
    layered: &mut impl Layered,
    size: [u32; 2],
    layers: u32,
    cancel: &AtomicBool,
) -> Result<grana_store::Scratch, String> {
    let stride = size[0] as usize * 3;
    let plane = stride * size[1] as usize;
    let mut pixels = grana_store::Scratch::acquire((plane * layers as usize) as u64)
        .map_err(|error| error.to_string())?;
    let held = pixels.bytes_mut().map_err(|error| error.to_string())?;
    for (layer, rows) in held.chunks_mut(plane).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("the super-resolution image was cancelled".to_string());
        }
        layered.begin_layer(layer as u32)?;
        for row in rows.chunks_mut(stride) {
            layered.compose_row(row)?;
        }
    }
    Ok(pixels)
}
