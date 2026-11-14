use grana_measure::{Aggregate, Group, Kind, Objects, Table};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_report::share::{Report, Section};

use crate::support::measure_source;

fn spots(frame: u32, count: usize) -> SpotSet {
    let spots = (0..count)
        .map(|index| {
            let offset = index as f32 + frame as f32 * 0.5;
            Spot::new(
                [offset, 0.5 * offset, 1.0],
                0.5 + 0.25 * offset,
                0.25 * offset,
            )
        })
        .collect::<Vec<_>>();
    let intensities = (0..count)
        .map(|index| {
            let mean = 1_000.0 + 2_500.0 * index as f32;
            SampleStatistics::new(mean, mean, mean, 0.0, f64::from(mean))
        })
        .collect::<Vec<_>>();
    SpotSet::new(0, spots, IntensityTable::new(1, intensities))
}

#[test]
fn writes_a_self_contained_report() {
    let first = spots(0, 3);
    let second = spots(1, 4);
    let third = spots(2, 5);
    let frames = measure_source(
        Kind::Spots,
        &["signal"],
        [
            Objects::Spots(&first),
            Objects::Spots(&second),
            Objects::Spots(&third),
        ],
    );
    let table = Table::build(&frames, None, 1.0, Group::Object, Aggregate::MEAN, None)
        .expect("the object table builds");
    let facts = vec![
        ("dimensions".to_string(), "64 × 32 × 32 voxels".to_string()),
        ("channels".to_string(), "signal".to_string()),
    ];
    let steps = vec!["Spots on signal".to_string()];
    let files = vec!["measurements/spots-objects.csv".to_string()];
    let sections = [Section {
        name: "Spots",
        kind: "Spots",
        origin: "Detection · v1",
        objects: 7,
        tracks: Some(2),
        table: &table,
        file: "measurements/spots-objects.csv",
    }];
    let page = grana_report::share::html(&Report {
        title: "<b>nuclei</b>",
        facts: &facts,
        thumbnail: Some(b"png-bytes"),
        sections: &sections,
        steps: &steps,
        files: &files,
        version: "0.0.0",
        created: "2026-01-02T03:04:05Z",
    })
    .expect("the report composes");

    assert!(page.starts_with("<!doctype html>"), "the report is a page");
    assert!(
        page.contains("&lt;b&gt;nuclei&lt;/b&gt;") && !page.contains("<b>nuclei</b>"),
        "the report escapes its title"
    );
    assert!(
        page.contains("data:image/png;base64,cG5nLWJ5dGVz"),
        "the report embeds its thumbnail"
    );
    assert!(
        page.contains("<svg") && page.contains("grana-report-0-clip"),
        "the report embeds a namespaced chart"
    );
    assert!(
        page.matches("<div class=\"chart\">").count() == 2,
        "the report charts a distribution and a frame series"
    );
    assert!(
        page.contains("measurements/spots-objects.csv")
            && page.contains("Statistics of Spots")
            && page.contains("diameter"),
        "the report links its tables and measures them"
    );
    assert!(
        page.contains("Detection · v1") && page.contains("7 objects · 2 tracks"),
        "the report numbers its set"
    );
}
