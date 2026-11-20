use glam::Vec2;
use grana_ui::{
    Align, BuildContext, Corners, Element, Fill, Icon, IconStyle, Key, Layer, Length, Level,
    Motion, Rect, Role, State, TextStyle, icon,
};

use crate::message::Message;
use crate::state::Workspace;

const WIDTH: f32 = 408.0;
const MINIMUM_WIDTH: f32 = 300.0;
const GUTTER: f32 = 24.0;
const PADDING: f32 = 24.0;
const TITLE_GAP: f32 = 12.0;
const ACTION_GAP: f32 = 22.0;
const ACTION_HEIGHT: f32 = 32.0;
const BUTTON_GAP: f32 = 8.0;
const SYMBOL: f32 = 18.0;
const SYMBOL_GAP: f32 = 10.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    Unsaved,
    UnsavedField { name: String, savable: bool },
    ExportRunning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Cancel,
    Discard,
    Save,
    Acknowledge,
}

pub fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    full: Rect,
    workspace: &Workspace,
) -> Vec<Element<'a, Message>> {
    let open = workspace.prompt.is_some();
    let target = if open { 1.0 } else { 0.0 };
    let spatial = ctx.animate(
        Key::new("prompt.spatial").id(),
        target,
        Motion::spatial_default(),
    );
    let effect = ctx.animate(
        Key::new("prompt.effect").id(),
        target,
        Motion::effect_default(),
    );
    if !open && effect <= 0.005 && spatial <= 0.005 {
        return Vec::new();
    }
    let Some(prompt) = workspace.prompt.as_ref().or(workspace.prompt_last.as_ref()) else {
        return Vec::new();
    };
    let theme = *ctx.theme();
    let palette = theme.palette;
    let language = ctx.language();
    let alpha = effect.clamp(0.0, 1.0);
    let slide = (1.0 - spatial.clamp(0.0, 1.0)) * -12.0;
    let width = (full.width() - 2.0 * GUTTER)
        .clamp(MINIMUM_WIDTH, WIDTH)
        .min(full.width());
    let inner_width = (width - 2.0 * PADDING).max(80.0);
    let title_style = TextStyle::new(theme.font(Level::Headline), palette.on_surface);
    let body_style = TextStyle::new(theme.font(Level::Body), palette.on_surface_variant).wrapping();
    let (title, body) = match prompt {
        Prompt::Unsaved => (
            grana_i18n::app::unsaved_analysis(language),
            grana_i18n::app::unsaved_detail(language).to_string(),
        ),
        Prompt::UnsavedField { name, savable } => (
            grana_i18n::app::unsaved_analysis(language),
            if *savable {
                grana_i18n::app::unsaved_field(language, name)
            } else {
                grana_i18n::app::unsaved_unsavable(language, name)
            },
        ),
        Prompt::ExportRunning => (
            grana_i18n::app::export_running(language),
            grana_i18n::app::export_running_detail(language).to_string(),
        ),
    };
    let text_width = (inner_width - SYMBOL - SYMBOL_GAP).max(60.0);
    let title_layout = ctx.layout_text(title, title_style, text_width);
    let body_layout = ctx.layout_text(&body, body_style, inner_width);
    let title_height = title_layout.size.y.max(SYMBOL);
    let height =
        PADDING * 2.0 + title_height + TITLE_GAP + body_layout.size.y + ACTION_GAP + ACTION_HEIGHT;
    let rect = Rect::from_center_size(
        full.center() + Vec2::new(0.0, slide),
        Vec2::new(width, height),
    );
    let inner = rect.shrink(PADDING);
    let title_rect = Rect::from_min_size(inner.min, Vec2::new(inner_width, title_height));
    let body_rect = Rect::from_min_size(
        Vec2::new(inner.min.x, title_rect.max.y + TITLE_GAP),
        Vec2::new(inner_width, body_layout.size.y),
    );
    let action_rect = Rect::from_min_size(
        Vec2::new(inner.min.x, inner.max.y - ACTION_HEIGHT),
        Vec2::new(inner_width, ACTION_HEIGHT),
    );
    let (symbol, tint) = match prompt {
        Prompt::ExportRunning => (Icon::Info, palette.tertiary),
        Prompt::Unsaved | Prompt::UnsavedField { .. } => (Icon::Warning, palette.warning),
    };
    let scrim = palette.scrim.with_alpha(palette.scrim.alpha * alpha);
    let backdrop = grana_ui::canvas(Length::Fill, full.height())
        .view(alpha.to_bits(), move |_, painter, info| {
            painter.rect(info.rect, Corners::ZERO, scrim);
        })
        .named("prompt.scrim")
        .at(full)
        .layer(Layer::Hud);
    let backdrop = if open {
        backdrop.hit_target()
    } else {
        backdrop
    };
    let mut surface = theme.surface(Role::CardElevated, State::Rest);
    surface.fill = Fill::Solid(palette.surface_container_high.with_alpha(alpha));
    surface.corners = Corners::all(theme.metrics.corner_xxl);
    let title_element = grana_ui::canvas(Length::Fill, title_height)
        .view(
            (title.to_string(), alpha.to_bits()),
            move |(title, bits): &(String, u32), painter, info| {
                let alpha = alpha_at(*bits);
                let layout = painter.layout(title, title_style, text_width);
                icon::paint(
                    symbol,
                    painter,
                    Vec2::new(info.rect.min.x + SYMBOL * 0.5, info.rect.center().y),
                    SYMBOL,
                    tint.with_alpha(alpha),
                    IconStyle::REGULAR,
                );
                painter.text(
                    &layout,
                    Vec2::new(
                        info.rect.min.x + SYMBOL + SYMBOL_GAP,
                        info.rect.min.y + (info.rect.height() - layout.size.y) * 0.5,
                    ),
                    palette.on_surface.with_alpha(alpha),
                );
            },
        )
        .named("prompt.title")
        .at(title_rect);
    let body_element = grana_ui::canvas(Length::Fill, body_layout.size.y)
        .view(
            (body, alpha.to_bits()),
            move |(text, bits): &(String, u32), painter, info| {
                let layout = painter.layout(text, body_style, inner_width);
                painter.text(
                    &layout,
                    info.rect.min,
                    palette.on_surface_variant.with_alpha(alpha_at(*bits)),
                );
            },
        )
        .named("prompt.body")
        .at(body_rect);
    let cancel = action(
        ctx,
        "prompt.cancel",
        grana_i18n::common::cancel(language),
        Role::Standard,
        Choice::Cancel,
        open,
    );
    let discard = action(
        ctx,
        "prompt.discard",
        grana_i18n::common::discard(language),
        Role::Destructive,
        Choice::Discard,
        open,
    );
    let save = action(
        ctx,
        "prompt.save",
        grana_i18n::common::save(language),
        Role::Filled,
        Choice::Save,
        open,
    );
    let acknowledge = action(
        ctx,
        "prompt.acknowledge",
        grana_i18n::common::ok(language),
        Role::Filled,
        Choice::Acknowledge,
        open,
    );
    let actions = match prompt {
        Prompt::Unsaved | Prompt::UnsavedField { savable: true, .. } => {
            vec![cancel, discard, save]
        }
        Prompt::UnsavedField { savable: false, .. } => vec![cancel, discard],
        Prompt::ExportRunning => vec![acknowledge],
    };
    let actions = grana_ui::row()
        .at(action_rect)
        .gap(BUTTON_GAP)
        .align(Align::Center)
        .justify(Align::End)
        .children(actions);
    let dialog = grana_ui::column()
        .at(rect)
        .named("prompt")
        .background(surface)
        .children([title_element, body_element, actions])
        .layer(Layer::Hud)
        .blocks_pointer()
        .modal();
    vec![backdrop, dialog]
}

fn alpha_at(bits: u32) -> f32 {
    f32::from_bits(bits)
}

fn action<'a>(
    ctx: &mut BuildContext<'_, Message>,
    name: &str,
    label: &str,
    role: Role,
    choice: Choice,
    open: bool,
) -> Element<'a, Message> {
    let theme = *ctx.theme();
    let element = grana_ui::widget::button(&theme, label, role)
        .named(name)
        .height(Length::Fixed(ACTION_HEIGHT));
    if open {
        element.press(Message::ResolvePrompt(choice))
    } else {
        element
    }
}
