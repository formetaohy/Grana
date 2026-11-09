use std::path::Path;

use grana_raster::{Axis, Line, Series, line_plot};
use grana_scene::Scene;
use grana_text::Fonts;

const SIZE: [u32; 2] = [960, 540];

const X_TINT: [f32; 3] = [0.62, 0.68, 0.78];

const Y_TINT: [f32; 3] = [0.70, 0.76, 0.86];

const FALLBACK_TINT: [f32; 3] = [0.72, 0.78, 0.88];

pub(super) fn write(scene: &Scene, folder: &Path) -> Result<(), String> {
    let tints = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.tint())
        .collect::<Vec<_>>();
    let mut fonts = Fonts::system();
    for (index, annotation) in scene.annotations.iter().enumerate() {
        let Some(profile) = annotation.profile() else {
            continue;
        };
        let stem = format!(
            "profile-{:02}-{}",
            index,
            grana_scene::slug(annotation.name())
        );
        let offsets = profile
            .samples()
            .iter()
            .map(|sample| sample.offset())
            .collect::<Vec<_>>();
        let values = (0..profile.names().len())
            .map(|channel| {
                profile
                    .samples()
                    .iter()
                    .map(|sample| sample.values()[channel])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut header = vec!["offset".to_string()];
        header.extend(profile.names().iter().cloned());
        let mut lines = vec![grana_measure::csv::line(
            &header.iter().map(String::as_str).collect::<Vec<_>>(),
        )];
        for (sample, offset) in profile.samples().iter().zip(&offsets) {
            let mut row = vec![format!("{offset:.4}")];
            row.extend(sample.values().iter().map(|value| format!("{value:.6}")));
            lines.push(grana_measure::csv::line(
                &row.iter().map(String::as_str).collect::<Vec<_>>(),
            ));
        }
        let csv = format!("{stem}.csv");
        std::fs::write(folder.join(&csv), lines.concat())
            .map_err(|error| format!("failed to write {csv}: {error}"))?;
        let series = (0..profile.names().len())
            .map(|channel| Series {
                name: profile.names()[channel].as_str(),
                tint: tints.get(channel).copied().unwrap_or(FALLBACK_TINT),
                values: &values[channel],
            })
            .collect::<Vec<_>>();
        let subtitle = format!(
            "{} samples · {} µm spacing",
            profile.samples().len(),
            profile.spacing()
        );
        let caption = format!(
            "{} · {:.2} µm",
            annotation.kind().tag().label(),
            profile.length()
        );
        let plot = Line {
            x: Axis::new("distance (µm)", X_TINT),
            y: Axis::new("intensity", Y_TINT),
            offsets: &offsets,
            series: &series,
            title: annotation.name(),
            subtitle: &subtitle,
            caption: &caption,
        };
        let png = format!("{stem}.png");
        let raster = line_plot(&mut fonts, &plot, SIZE);
        grana_raster::write(&folder.join(&png), &raster)
            .map_err(|error| format!("failed to write {png}: {error}"))?;
    }
    Ok(())
}
