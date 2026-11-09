use grana_session::{Command, PlateView, Well};
use grana_ui::{
    Align, BuildContext, Corners, Element, Fill, Icon, Length, Level, Role, Shadow, TextStyle,
};

use crate::message::Message;
use crate::state::View;
use crate::view;

const GAP: f32 = 5.0;
const PER_ROW: usize = 6;

pub fn view<'a>(ctx: &mut BuildContext<'_, Message>, state: &'a View<'a>) -> Element<'a, Message> {
    let Some(plate) = state.plate.as_ref() else {
        return view::empty_state(
            ctx,
            Icon::Grid,
            "No plate loaded",
            "Open an OME-Zarr plate to browse its wells and jump between fields of view.",
        );
    };
    let mut children = vec![summary(ctx, plate)];
    children.push(grid(ctx, plate));
    if let Some(well) = plate.selected_well() {
        children.push(fields(ctx, plate, well));
    }
    grana_ui::column()
        .gap(12.0)
        .width(Length::Fill)
        .children(children)
}

fn summary<'a>(ctx: &mut BuildContext<'_, Message>, view: &PlateView<'_>) -> Element<'a, Message> {
    let plate = view.plate;
    let fields = plate.len();
    let wells = plate.wells().len();
    let title = view::card_title(
        ctx,
        Icon::Grid,
        plate.name(),
        Some(&format!(
            "{} × {} wells · {wells} wells · {fields} fields",
            plate.rows(),
            plate.columns()
        )),
    );
    let hint = view::weak(
        ctx,
        "Choose a well to open its first field of view; the fields of the selected well are listed below.",
    );
    view::card(ctx, 6.0, vec![title, hint])
}

fn grid<'a>(ctx: &mut BuildContext<'_, Message>, view: &PlateView<'_>) -> Element<'a, Message> {
    let plate = view.plate;
    let selected = view.selected_well();
    let side = cell_side(plate.columns());
    let mut rows = Vec::with_capacity(plate.rows());
    for row in 0..plate.rows() {
        let mut cells = Vec::with_capacity(plate.columns());
        for column in 0..plate.columns() {
            cells.push(cell(
                ctx,
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
    let element = grana_ui::canvas(Length::Fixed(side), side, move |painter, info| {
        let rect = info.rect;
        let engaged = info.hover.max(info.press);
        if present {
            let fill = if active {
                palette.accent.with_alpha(0.24)
            } else {
                palette
                    .material_fill
                    .lerp(palette.accent.with_alpha(0.14), 0.6 * engaged)
            };
            let border = if active {
                palette.accent.with_alpha(0.9)
            } else {
                palette.material_border.amplify(0.4 * engaged)
            };
            painter.outlined(
                rect,
                Corners::all(8.0),
                Fill::Solid(fill),
                grana_ui::Border {
                    width: 1.0,
                    color: border,
                },
                Shadow::NONE,
            );
            painter.text_in(
                rect,
                &drawn,
                TextStyle::new(
                    font,
                    palette.text_secondary.lerp(palette.text_primary, engaged),
                )
                .aligned(Align::Center),
                Align::Center,
            );
        } else {
            painter.outlined(
                rect,
                Corners::all(8.0),
                Fill::Solid(palette.card.with_alpha(0.35)),
                grana_ui::Border {
                    width: 1.0,
                    color: palette.separator.with_alpha(0.5),
                },
                Shadow::NONE,
            );
        }
    })
    .named(&format!("plate.{row}.{column}"));
    match well.and_then(|well| well.fields().first()) {
        Some(field) => {
            let detail = format!(
                "{} · {} fields",
                label,
                well.map_or(0, |well| well.fields().len())
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
    let count = well.fields().len();
    let mut children = vec![view::section_header(
        ctx,
        &format!("Well {}", well.label()),
        Some(&format!(
            "{count} field{}",
            if count == 1 { "" } else { "s" }
        )),
    )];
    if well.fields().is_empty() {
        children.push(view::weak(ctx, "This well holds no field of view."));
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
            || format!("Field {}", index + 1),
            |suffix| format!("Field {}", suffix.trim_start_matches('/')),
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

fn cell_side(columns: usize) -> f32 {
    match columns {
        0..=8 => 52.0,
        9..=12 => 44.0,
        13..=16 => 38.0,
        17..=24 => 32.0,
        _ => 26.0,
    }
}
