use std::path::Path;

use grana_chart::{DistributionStyle, Heading, Page, SeriesPage};
use grana_i18n::Language;
use grana_profile::Profile;
use grana_scene::{Annotation, Scene};

const FALLBACK_TINT: [f32; 3] = [0.72, 0.78, 0.88];

pub(super) fn write(scene: &Scene, folder: &Path, language: Language) -> Result<Vec<Page>, String> {
    let tints = scene
        .dataset
        .channels()
        .iter()
        .map(|channel| channel.tint())
        .collect::<Vec<_>>();
    let mut pages = Vec::new();
    for (index, annotation) in scene.annotations.iter().enumerate() {
        let Some(profile) = annotation.profile() else {
            continue;
        };
        let stem = format!(
            "profile-{:02}-{}",
            index,
            grana_scene::slug(annotation.name())
        );
        let csv = format!("{stem}.csv");
        grana_file::write(&folder.join(&csv), table(profile).as_bytes())
            .map_err(|error| format!("failed to write {csv}: {error}"))?;
        let page = page(annotation, profile, &tints, scene.dataset.name(), language);
        grana_page::write_page(&page, &folder.join(format!("{stem}.svg")))?;
        pages.push(page);
    }
    Ok(pages)
}

fn table(profile: &Profile) -> String {
    let mut header = vec!["offset".to_string()];
    header.extend(profile.names().iter().cloned());
    let mut lines = vec![grana_measure::csv::line(
        &header.iter().map(String::as_str).collect::<Vec<_>>(),
    )];
    for sample in profile.samples() {
        let mut row = vec![format!("{:.4}", sample.offset())];
        row.extend(sample.values().iter().map(|value| format!("{value:.6}")));
        lines.push(grana_measure::csv::line(
            &row.iter().map(String::as_str).collect::<Vec<_>>(),
        ));
    }
    lines.concat()
}

fn page(
    annotation: &Annotation,
    profile: &Profile,
    tints: &[[f32; 3]],
    dataset: &str,
    language: Language,
) -> Page {
    let heading = Heading::new(
        annotation.name(),
        grana_i18n::annotation::profile_axis(language),
        grana_i18n::annotation::profile_offset(language),
    )
    .with_summary(format!(
        "{dataset} · {}",
        grana_i18n::annotation::profile_span(language, profile.length(), profile.spacing())
    ))
    .with_empty(grana_i18n::annotation::empty_profile(language))
    .with_language(language);
    let tints = (0..profile.names().len())
        .map(|channel| tints.get(channel).copied().unwrap_or(FALLBACK_TINT))
        .collect::<Vec<_>>();
    Page::Series(Box::new(SeriesPage::of_profile(
        profile,
        &tints,
        heading,
        DistributionStyle::Box,
    )))
}
