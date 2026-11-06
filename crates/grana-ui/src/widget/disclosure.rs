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
    let header = ui.allocate_full(30.0);
    let mut response = ui.interact(key, header);
    let corners = Corners::all(9.0);
    if ui.focusable(key, header, corners) && ui.activated(key) {
        response.clicked = true;
    }
    let next = if response.clicked { !open } else { open };
    let arrow = ui.animate(
        key,
        if open { 1.0 } else { 0.0 },
        Motion::response(0.34, 0.2),
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
                header.shrink2(Vec2::new(1.0, 0.5)),
                corners,
                palette.material_hover.amplify(0.5 * hover),
            );
        }
        painter.text_in(
            Rect::from_min_max(Vec2::new(header.min.x + 24.0, header.min.y), header.max),
            title,
            style,
            Align::Center,
        );
        let centre = Vec2::new(header.min.x + 10.0, header.center().y);
        let tint = palette.text_tertiary.lerp(palette.text_secondary, hover);
        crate::icon::chevron(
            &mut painter,
            centre,
            8.0,
            std::f32::consts::FRAC_PI_2 * arrow,
            tint,
            1.6,
        );
    }
    if next {
        ui.column(Block::column().gaps(6.0), content);
    }
    next
}
