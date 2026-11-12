use glam::Vec2;
use grana_session::{Command, PlateView, Well};
use grana_ui::{
    Align, BuildContext, Color, Corners, Element, Fill, Icon, Length, Level, Rect, Role, Shadow,
    TextStyle,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const GAP: f32 = 6.0;
const PER_ROW: usize = 6;

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    state: &'a View<'a>,
    width: f32,
) -> Element<'a, Message> {
    let language = ctx.language();
    let Some(plate) = state.plate.as_ref() else {
        return view::empty_state(
            ctx,
            Icon::Grid,
            grana_i18n::plate::no_plate(language),
            grana_i18n::plate::no_plate_detail(language),
        );
    };
    let mut children = vec![summary(ctx, plate)];
    children.push(grid(ctx, plate, width));
    if let Some(well) = plate.selected_well() {
        children.push(fields(ctx, plate, well));
    }
    grana_ui::column()
        .gap(12.0)
        .width(Length::Fill)
        .children(children)
}

fn summary<'a>(ctx: &mut BuildContext<'_, Message>, view: &PlateView<'_>) -> Element<'a, Message> {
    let language = ctx.language();
    let plate = view.plate;
    let fields = plate.len();
    let wells = plate.wells().len();
    let previews = if view.previews == 0 || view.previews == fields {
        grana_i18n::plate::summary(language, fields)
    } else {
        grana_i18n::plate::summary_previews(language, fields, view.previews)
    };
    let title = view::card_title(
        ctx,
        Icon::Grid,
        plate.name(),
        Some(&grana_i18n::plate::grid_detail(
            language,
            plate.rows(),
            plate.columns(),
            wells,
            &previews,
        )),
    );
    let hint = view::weak(ctx, grana_i18n::plate::every_well(language));
    let export = view::button(
        ctx,
        grana_i18n::plate::export_montage(language),
        grana_ui::Role::Control,
    )
    .named("plate.export")
    .press(Message::Command(Command::ExportPlate));
    view::card(ctx, 8.0, vec![title, hint, export])
}

fn grid<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &PlateView<'_>,
    width: f32,
) -> Element<'a, Message> {
    let plate = view.plate;
    let selected = view.selected_well();
    let side = cell_side(plate.columns(), width);
    let mut rows = Vec::with_capacity(plate.rows());
    for row in 0..plate.rows() {
        let mut cells = Vec::with_capacity(plate.columns());
        for column in 0..plate.columns() {
            cells.push(cell(
                ctx,
                view,
                plate.well(row, column),
                selected,
                row,
                column,
                side,
            ));
        }
        rows.push(
            grana_ui::row()
                .gap(GAP)
                .align(Align::Center)
                .children(cells),
        );
    }
    view::card(ctx, 8.0, vec![grana_ui::column().gap(GAP).children(rows)])
}

fn cell<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &PlateView<'_>,
    well: Option<&Well>,
    selected: Option<&Well>,
    row: usize,
    column: usize,
    side: f32,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let active = well.is_some_and(|well| selected.is_some_and(|held| std::ptr::eq(held, well)));
    let present = well.is_some();
    let label = well.map_or_else(String::new, |well| well.label().to_string());
    let drawn = label.clone();
    let font = theme.font(Level::Micro);
    let thumbnail = well
        .and_then(|well| well.fields().first())
        .and_then(|field| view.thumbnail(field.series()))
        .cloned();
    let lit = thumbnail.is_some();
    let element = grana_ui::canvas(Length::Fixed(side), side, move |painter, info| {
        let rect = info.rect;
        let engaged = info.hover.max(info.press);
        let corners = Corners::all(6.0);
        let border = if active {
            palette.accent.with_alpha(0.95)
        } else {
            palette.material_border.amplify(0.25 + 0.55 * engaged)
        };
        if present {
            let fill = if active {
                palette.accent.with_alpha(0.24)
            } else {
                palette
                    .material_fill
                    .lerp(palette.accent.with_alpha(0.14), 0.6 * engaged)
            };
            painter.rect(rect, corners, fill);
        } else {
            painter.outlined(
                rect,
                corners,
                Fill::Solid(palette.card.with_alpha(0.35)),
                grana_ui::Border {
                    width: 1.0,
                    color: palette.separator.with_alpha(0.5),
                },
                Shadow::NONE,
            );
        }
        if let Some(bitmap) = &thumbnail {
            painter.bitmap(rect, corners, bitmap);
        }
        if present {
            painter.outlined(
                rect,
                corners,
                Fill::None,
                grana_ui::Border {
                    width: if active { 1.5 } else { 1.0 },
                    color: border,
                },
                Shadow::NONE,
            );
        }
        if present {
            let tint = if lit {
                Color::white_alpha(0.92)
            } else {
                palette.text_secondary.lerp(palette.text_primary, engaged)
            };
            let style = TextStyle::new(font, tint).aligned(Align::Center);
            let ink = painter.measure(&drawn, style);
            let pill = if lit {
                Rect::from_min_size(
                    Vec2::new(rect.min.x + 5.0, rect.max.y - 19.0),
                    Vec2::new(ink.x + 10.0, 14.0),
                )
            } else {
                Rect::from_center_size(rect.center(), Vec2::new(ink.x + 16.0, 18.0))
            };
            if lit {
                painter.rect(pill, Corners::all(3.5), Color::black_alpha(0.65));
            }
            painter.text_in(pill, &drawn, style, Align::Center);
        }
    })
    .named(&format!("plate.{row}.{column}"));
    match well.and_then(|well| well.fields().first()) {
        Some(field) => {
            let detail = grana_i18n::plate::well_fields(
                ctx.language(),
                &label,
                well.map_or(0, |well| well.fields().len()),
            );
            let series = field.series();
            element
                .tooltip(&detail)
                .press(Message::Command(Command::SelectField(series)))
        }
        None => element,
    }
}

fn fields<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &PlateView<'_>,
    well: &Well,
) -> Element<'a, Message> {
    let language = ctx.language();
    let count = well.fields().len();
    let mut children = vec![view::section_header(
        ctx,
        &grana_i18n::plate::well_header(language, well.label()),
        Some(&grana_i18n::plate::field_count(language, count)),
    )];
    if well.fields().is_empty() {
        children.push(view::weak(ctx, grana_i18n::plate::no_field(language)));
    }
    let mut buttons = Vec::new();
    for (index, field) in well.fields().iter().enumerate() {
        let selected = field.series() == view.selected;
        let role = if selected {
            Role::Tinted
        } else {
            Role::Control
        };
        let name = field.name().strip_prefix(well.label()).map_or_else(
            || grana_i18n::plate::field_name(language, index + 1),
            |suffix| grana_i18n::plate::field_suffix(language, suffix.trim_start_matches('/')),
        );
        buttons.push(
            view::button(ctx, &name, role)
                .named(&format!("plate.field.{index}"))
                .press(Message::Command(Command::SelectField(field.series()))),
        );
    }
    let mut rows: Vec<Vec<Element<'a, Message>>> = Vec::new();
    for button in buttons {
        if rows.last().is_none_or(|row| row.len() == PER_ROW) {
            rows.push(Vec::new());
        }
        rows.last_mut().expect("a wrap row is open").push(button);
    }
    for row in rows {
        children.push(grana_ui::row().gap(6.0).align(Align::Center).children(row));
    }
    view::card(ctx, 8.0, children)
}

fn cell_side(columns: usize, width: f32) -> f32 {
    let count = columns.max(1) as f32;
    let span = (width - 24.0 - GAP * (count - 1.0)).max(0.0) / count;
    span.clamp(24.0, 116.0)
}
