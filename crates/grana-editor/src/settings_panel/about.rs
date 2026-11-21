use grana_ui::{
    Align, BuildContext, Corners, Edges, Element, Fill, Icon, Length, Level, Role, State, TextStyle,
};

use super::row;
use crate::message::Message;
use crate::state::Workspace;

const LOGO: f32 = 56.0;
const HERO_INSET: f32 = 18.0;
const HERO_GAP: f32 = 16.0;

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let graphics = row::value(
        ctx,
        Icon::Cube,
        grana_i18n::settings::graphics(language),
        workspace.system.adapter.as_deref().unwrap_or("—"),
    )
    .named("settings.graphics");
    let graphics = match workspace.system.adapter.as_deref() {
        Some(adapter) => graphics.tooltip(adapter),
        None => graphics,
    };
    vec![
        identity(ctx),
        row::section(
            ctx,
            None,
            vec![
                graphics,
                row::value(
                    ctx,
                    Icon::Info,
                    grana_i18n::settings::license(language),
                    "MIT",
                )
                .named("settings.license"),
                row::action(
                    ctx,
                    Icon::Globe,
                    grana_i18n::settings::project_page(language),
                    None,
                    Icon::ChevronRight,
                    Message::SettingsAction(crate::state::SettingsRequest::OpenRepository),
                    open,
                )
                .named("settings.project"),
            ],
        ),
    ]
}

fn identity<'a>(ctx: &mut BuildContext<'_, Message>) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let mark = grana_ui::canvas(Length::Fixed(LOGO), LOGO)
        .named("settings.brand")
        .view_static(|painter, info| grana_brand::paint(painter, info.rect));
    let name = ctx
        .text(
            "Grana",
            TextStyle::new(theme.font(Level::Display), palette.on_surface),
        )
        .width(Length::Fill);
    let version = ctx.text(
        format!(
            "{} {}",
            grana_i18n::settings::version(language),
            env!("CARGO_PKG_VERSION")
        ),
        TextStyle::new(theme.font(Level::Caption), palette.on_surface_variant),
    );
    let mut surface = theme.surface(Role::CardElevated, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_high);
    surface.corners = Corners::all(24.0);
    grana_ui::row()
        .width(Length::Fill)
        .padding(Edges::all(HERO_INSET))
        .gap(HERO_GAP)
        .align(Align::Center)
        .background(surface)
        .children([
            mark,
            grana_ui::column()
                .width(Length::Fill)
                .gap(2.0)
                .children([name, version]),
        ])
}
