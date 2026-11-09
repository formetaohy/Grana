use glam::Vec2;
use grana_session::{Command, Kind, SessionView};
use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Icon, Layer, Length, Level, Rect, Role, TextStyle,
};

use crate::message::Message;
use crate::state::Workspace;

const CARD_WIDTH: f32 = 468.0;
const CARD_PADDING: f32 = 24.0;
const CARD_GAP: f32 = 14.0;
const BRAND_HEIGHT: f32 = 42.0;
const STATE_HEIGHT: f32 = 24.0;

struct Status {
    text: String,
    detail: String,
    busy: bool,
    error: bool,
}

pub(crate) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    view: &SessionView<'_>,
    workspace: &Workspace,
    full: Rect,
) -> Vec<Element<'a, Message>> {
    let palette = ctx.theme().palette;
    let backdrop = palette.scene_backdrop;
    let status = status(view);
    let detail_style = TextStyle::new(
        ctx.theme().font(Level::Callout),
        if status.error {
            palette.warning
        } else {
            palette.text_tertiary
        },
    )
    .wrapping();
    let detail = ctx.layout_text(
        &status.detail,
        detail_style,
        CARD_WIDTH - 2.0 * CARD_PADDING,
    );
    let detail_height = detail.size.y.max(18.0);
    let button = ctx.theme().metrics.control_height;
    let height = 2.0 * CARD_PADDING
        + BRAND_HEIGHT
        + CARD_GAP
        + STATE_HEIGHT
        + CARD_GAP
        + detail_height
        + CARD_GAP
        + button
        + CARD_GAP
        + button;
    let rect = Rect::from_min_size(
        Vec2::new(
            full.center().x - 0.5 * CARD_WIDTH,
            full.center().y - 0.54 * height,
        ),
        Vec2::new(CARD_WIDTH, height),
    );
    let glow = Rect::from_min_size(full.min, Vec2::new(full.width(), 0.62 * full.height()));
    let accent = palette.accent;
    let mut elements = vec![
        grana_ui::canvas(Length::Fill, full.height(), move |painter, info| {
            painter.rect(info.rect, Corners::ZERO, backdrop);
            painter.gradient(glow, Corners::ZERO, backdrop.lerp(accent, 0.16), backdrop);
        })
        .named("shell.backdrop")
        .at(full)
        .layer(Layer::Content),
    ];
    let card = grana_ui::column()
        .padding(Edges::all(CARD_PADDING))
        .gap(CARD_GAP)
        .children([
            brand(ctx),
            state(ctx, &status),
            grana_ui::text(status.detail.clone(), detail_style),
            actions(ctx),
            grana_ui::row().gap(8.0).children([
                grana_ui::widget::action_button(
                    ctx.theme(),
                    Icon::Folder,
                    "OME-Zarr Folder",
                    Role::Control,
                )
                .named("shell.open.dataset")
                .press(Message::Command(Command::OpenDataset)),
                grana_ui::widget::action_button(
                    ctx.theme(),
                    Icon::Globe,
                    "Remote OME-Zarr",
                    Role::Control,
                )
                .named("shell.open.remote")
                .press(Message::BeginRemote),
                grana_ui::widget::action_button(
                    ctx.theme(),
                    Icon::Folder,
                    "Project",
                    Role::Control,
                )
                .named("shell.open.project")
                .press(Message::Command(Command::OpenProject)),
            ]),
        ])
        .at(rect)
        .named("shell.card");
    elements.push(surface(ctx, rect));
    elements.push(card);
    elements.extend(crate::panel::remote(ctx, full, workspace.remote_open));
    elements
}

fn surface<'a>(ctx: &mut BuildContext<'_, Message>, rect: Rect) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let corners = Corners::all(ctx.theme().metrics.card_radius);
    let top = palette.card;
    let bottom = palette.card.amplify(0.7);
    let border = palette.material_border;
    grana_ui::canvas(Length::Fill, rect.height(), move |painter, info| {
        painter.outlined(
            info.rect,
            corners,
            grana_ui::Fill::Gradient(top, bottom),
            grana_ui::Border {
                width: 1.0,
                color: border,
            },
            grana_ui::Shadow::NONE,
        );
    })
    .named("shell.surface")
    .at(rect)
    .layer(Layer::Content)
}

fn status(view: &SessionView<'_>) -> Status {
    if let Some((label, done, total)) = view.io.document {
        return Status {
            text: format!("Reading {label}"),
            detail: format!("{done} of {total} steps complete."),
            busy: true,
            error: false,
        };
    }
    if let Some(label) = view.io.loading {
        return Status {
            text: format!("Opening {label}"),
            detail: "The dataset streams in as it is explored, so the window stays live."
                .to_string(),
            busy: true,
            error: false,
        };
    }
    if let Some((label, done, total)) = view.io.label {
        return Status {
            text: format!("Writing {label}"),
            detail: format!("{done} of {total} planes stored."),
            busy: true,
            error: false,
        };
    }
    if let Some(message) = view.io.message {
        return Status {
            text: match message.kind() {
                Kind::Error => "Nothing could be opened".to_string(),
                Kind::Info => "Ready".to_string(),
            },
            detail: message.text().to_string(),
            busy: false,
            error: message.kind() == Kind::Error,
        };
    }
    Status {
        text: "No dataset open".to_string(),
        detail: "OME-Zarr, CZI, ND2, LIF and OME-TIFF data open here, locally or over HTTP."
            .to_string(),
        busy: false,
        error: false,
    }
}

fn brand<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let title = ctx.theme().font(Level::Title).semibold();
    let caption = ctx.theme().font(Level::Caption);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    grana_ui::canvas(Length::Fill, BRAND_HEIGHT, move |painter, info| {
        let rect = info.rect;
        let mut y = rect.center().y - 8.0;
        for width in [6.5, 7.6, 8.7, 9.8, 10.9] {
            painter.rect(
                Rect::from_min_size(Vec2::new(rect.min.x, y), Vec2::new(width, 2.4)),
                Corners::capsule(),
                palette.brand,
            );
            y += 3.8;
        }
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.min.x + 20.0, rect.min.y),
                Vec2::new(180.0, rect.height()),
            ),
            "grana",
            TextStyle::new(title, palette.text_primary),
            Align::Center,
        );
        painter.text_in(
            Rect::from_min_size(
                Vec2::new(rect.max.x - 140.0, rect.min.y),
                Vec2::new(140.0, rect.height()),
            ),
            &version,
            TextStyle::new(caption, palette.text_quaternary).aligned(Align::End),
            Align::Center,
        );
    })
    .named("shell.brand")
}

fn state<'a>(ctx: &mut BuildContext<'_, Message>, status: &Status) -> Element<'a, Message> {
    let palette = ctx.theme().palette;
    let font = ctx.theme().font(Level::Headline).medium();
    let color = if status.error {
        palette.warning
    } else {
        palette.text_secondary
    };
    let mut children = Vec::new();
    if status.busy {
        children.push(crate::view::spinner(ctx, 16.0));
    }
    children.push(grana_ui::text(
        status.text.clone(),
        TextStyle::new(font, color).truncating(),
    ));
    grana_ui::row()
        .gap(10.0)
        .align(Align::Center)
        .height(Length::Fixed(STATE_HEIGHT))
        .children(children)
}

fn actions<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    grana_ui::widget::button_width(ctx.theme(), "Open Image…", Role::Prominent, Length::Fill)
        .named("shell.open.image")
        .press(Message::Command(Command::OpenImage))
}
