use glam::Vec2;
use grana_object::{TrackLayout, TrackSet};
use grana_ui::{
    Align, Color, Corners, Mark, Painter, Palette, Rect, TextStyle, TypeStyle, Ui, Vec2 as UiVec2,
};

const MARGIN: f32 = 28.0;
const HEADER: f32 = 78.0;
const AXIS: f32 = 24.0;
const ROW: f32 = 15.0;
const FOOTER: f32 = 30.0;
const LABEL: f32 = 46.0;
const TICKS: u32 = 3;
const NODE_SPACING: f32 = 3.2;
const MIN_WIDTH: u32 = 320;
const MAX_WIDTH: u32 = 4096;
const MAX_HEIGHT: f32 = 14_400.0;
const MIN_ROW: f32 = 3.0;
const DENSE_ROW: f32 = 9.0;

pub const DEFAULT_WIDTH: u32 = 960;

pub struct Lineage<'a> {
    pub title: &'a str,
    pub set: &'a TrackSet,
    pub colors: &'a [[f32; 3]],
    pub frames: u32,
    pub time: u32,
}

pub fn page_size(tracks: usize, width: u32) -> Result<[u32; 2], String> {
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return Err(format!(
            "a lineage page spans {MIN_WIDTH} to {MAX_WIDTH} points wide, not {width}"
        ));
    }
    if tracks == 0 {
        return Err("a lineage holds no track to draw".to_string());
    }
    let height = HEADER + AXIS + tracks as f32 * row_height(tracks) + FOOTER;
    if height > MAX_HEIGHT {
        return Err(format!(
            "a lineage of {tracks} tracks is taller than the {MAX_HEIGHT} point limit of a page"
        ));
    }
    Ok([width, height.ceil() as u32])
}

fn row_height(tracks: usize) -> f32 {
    let available = MAX_HEIGHT - HEADER - AXIS - FOOTER;
    (available / tracks.max(1) as f32).clamp(MIN_ROW, ROW)
}

pub fn marks<A: Clone>(ui: &mut Ui<A>, size: [u32; 2], lineage: &Lineage<'_>) -> Vec<Mark> {
    assert_eq!(
        lineage.colors.len(),
        lineage.set.len(),
        "every track carries one colour"
    );
    assert_eq!(
        size,
        page_size(lineage.set.len(), size[0]).expect("the lineage fits its page"),
        "the page covers its lineage"
    );
    let palette = ui.theme().palette;
    let points = Vec2::new(size[0] as f32, size[1] as f32);
    ui.figure_marks(points, 1.0, |painter| {
        paint(painter, points, &palette, lineage)
    })
}

fn paint(painter: &mut Painter, size: Vec2, palette: &Palette, lineage: &Lineage<'_>) {
    let page = Rect::from_min_size(Vec2::ZERO, size);
    let layout = TrackLayout::of(lineage.set);
    let frames = lineage.frames.max(1);
    let span = frames - 1;
    let row = row_height(layout.len());
    let dense = row < DENSE_ROW;
    let top = HEADER + AXIS;
    let bottom = top + layout.len() as f32 * row;
    let left = MARGIN + LABEL;
    let right = size.x - MARGIN;
    painter.rect(page, Corners::ZERO, palette.scene_backdrop);
    let header = Rect::from_min_size(
        Vec2::new(MARGIN, 18.0),
        Vec2::new(size.x - 2.0 * MARGIN, 26.0),
    );
    painter.text_in(
        header,
        &format!("{} lineage", lineage.title),
        TextStyle::new(TypeStyle::new(17.0, 600.0, 22.0), palette.text_primary).truncating(),
        Align::Start,
    );
    painter.text_in(
        Rect::from_min_size(
            Vec2::new(MARGIN, 47.0),
            Vec2::new(size.x - 2.0 * MARGIN, 16.0),
        ),
        &format!(
            "{} tracks · {} divisions · {frames} frames",
            layout.len(),
            lineage.set.divisions()
        ),
        TextStyle::new(TypeStyle::new(10.0, 400.0, 13.0), palette.text_secondary).truncating(),
        Align::Start,
    );
    let x = |time: u32| {
        if span == 0 {
            left
        } else {
            left + (time as f32 / span as f32) * (right - left)
        }
    };
    let y = |lane: usize| top + lane as f32 * row + row * 0.5;
    let mut ticks = Vec::new();
    for part in 0..=TICKS {
        let time = span * part / TICKS;
        if ticks.last() != Some(&time) {
            ticks.push(time);
        }
    }
    let label = TextStyle::new(TypeStyle::new(9.5, 500.0, 12.0), palette.text_tertiary);
    for time in ticks {
        let column = x(time);
        painter.segment(
            Vec2::new(column, top - 8.0),
            Vec2::new(column, bottom),
            0.5,
            palette.separator,
        );
        painter.text_in(
            Rect::from_min_size(Vec2::new(column - 32.0, top - AXIS), Vec2::new(64.0, AXIS)),
            &format!("F{}", time + 1),
            label.aligned(Align::Center),
            Align::Center,
        );
    }
    painter.segment(
        Vec2::new(left, bottom),
        Vec2::new(right, bottom),
        0.5,
        palette.separator,
    );
    for (lane, index) in layout.order().iter().enumerate() {
        let Some(track) = lineage.set.track(*index) else {
            continue;
        };
        if !dense {
            painter.text_in(
                Rect::from_min_size(
                    Vec2::new(MARGIN, top + lane as f32 * row),
                    Vec2::new(LABEL - 8.0, row),
                ),
                &format!("#{}", index + 1),
                label.aligned(Align::End),
                Align::Center,
            );
        }
        let color = Color::from_rgb_f32(lineage.colors[*index]);
        let start = x(track.start());
        let end = x(track.end());
        for link in lineage.set.parents(*index) {
            let parent = link.parent() as usize;
            let lane_of_parent = layout.lane(parent);
            if let Some(parent) = lineage.set.track(parent) {
                painter.segment(
                    UiVec2::new(x(parent.end()), y(lane_of_parent)),
                    UiVec2::new(start, y(lane)),
                    1.0,
                    color.with_alpha(0.35),
                );
            }
        }
        painter.segment(
            UiVec2::new(start, y(lane)),
            UiVec2::new(end.max(start + 1.0), y(lane)),
            if dense { 1.0 } else { 1.6 },
            color,
        );
        if dense {
            continue;
        }
        let nodes = track.nodes();
        let spacing = if nodes.len() > 1 {
            (end - start) / (nodes.len() - 1) as f32
        } else {
            0.0
        };
        if spacing >= NODE_SPACING {
            for node in nodes {
                painter.rect(
                    Rect::from_center_size(
                        UiVec2::new(x(node.time()), y(lane)),
                        UiVec2::splat(2.2),
                    ),
                    Corners::capsule(),
                    color.with_alpha(0.88),
                );
            }
        } else if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
            for node in [first, last] {
                painter.rect(
                    Rect::from_center_size(
                        UiVec2::new(x(node.time()), y(lane)),
                        UiVec2::splat(2.8),
                    ),
                    Corners::capsule(),
                    color,
                );
            }
        }
    }
    let current = x(lineage.time.min(span));
    painter.segment(
        UiVec2::new(current, top - 8.0),
        UiVec2::new(current, bottom),
        1.2,
        palette.accent.with_alpha(0.85),
    );
    painter.text_at(
        Vec2::new(MARGIN, size.y - FOOTER + 10.0),
        &format!("Grana · frame {} of {frames}", lineage.time.min(span) + 1),
        TextStyle::new(TypeStyle::new(9.0, 400.0, 12.0), palette.text_quaternary),
    );
}
