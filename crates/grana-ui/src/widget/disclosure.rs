use glam::Vec2;

use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::text::{Align, TextStyle};
use crate::theme::Level;
use crate::ui::{Block, Ui};

pub fn disclosure(
    ui: &mut Ui,
    id: &str,
    title: &str,
    open: bool,
    content: impl FnOnce(&mut Ui),
) -> bool {
    let key = ui.id(id);
    let palette = ui.theme().palette;
    let header = ui.allocate_full(26.0);
    let mut response = ui.interact(key, header);
    let corners = Corners::all(8.0);
    if ui.focusable(key, header, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let next = if response.clicked { !open } else { open };
    let arrow = ui.animate(
        key,
        if open { 1.0 } else { 0.0 },
        Motion::response(0.32, 0.18),
    );
    let font = ui.theme().font(Level::Callout).medium();
    let hover = ui.animate(
        key ^ 0x31,
        if response.hovered { 1.0 } else { 0.0 },
        Motion::quick(),
    );
    let color = palette.text_secondary.lerp(palette.text_primary, hover);
    let style = TextStyle::new(font, color);
    {
        let mut painter = ui.painter();
        if hover > 0.01 {
            painter.rect(
                header.expand(2.0),
                corners,
                palette.material_hover.amplify(0.45 * hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(header.min, Vec2::new(header.max.x - 18.0, header.max.y)),
            title,
            style,
            Align::Center,
        );
        let centre = Vec2::new(header.max.x - 6.0, header.center().y);
        let tint = palette.text_tertiary.lerp(palette.text_secondary, hover);
        crate::icon::chevron(
            &mut painter,
            centre,
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            tint,
            1.5,
        );
    }
    if next {
        ui.column(Block::column().gaps(6.0), content);
    }
    next
}
