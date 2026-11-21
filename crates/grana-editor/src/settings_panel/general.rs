use grana_ui::{BuildContext, Element, Icon, Length, Role};

use super::row;
use crate::message::Message;
use crate::state::Workspace;

pub(super) fn view<'a>(
    ctx: &mut BuildContext<'_, Message>,
    workspace: &Workspace,
    open: bool,
) -> Vec<Element<'a, Message>> {
    let language = ctx.language();
    let language_options = grana_i18n::Language::ALL.map(|candidate| candidate.label());
    let language_selected = grana_i18n::Language::ALL
        .iter()
        .position(|candidate| *candidate == workspace.language)
        .unwrap_or(0);
    let selector = crate::view::segmented(
        ctx,
        "settings.language",
        &language_options,
        language_selected,
        |index| {
            Message::SetLanguage(
                grana_i18n::Language::ALL
                    .get(index)
                    .copied()
                    .unwrap_or(grana_i18n::Language::English),
            )
        },
    )
    .enabled(open);
    let language_row = row::control(
        ctx,
        Icon::Globe,
        grana_i18n::settings::language(language),
        selector,
        open,
    )
    .named("settings.language.row");

    let autosave = row::switch(
        ctx,
        Icon::Save,
        grana_i18n::settings::autosave(language),
        workspace.settings.autosave,
        open,
        Message::SetAutosave,
    )
    .named("settings.autosave");

    let data = workspace.storage.data.as_deref();
    let data_row = row::action(
        ctx,
        Icon::Folder,
        grana_i18n::settings::data(language),
        Some(data.unwrap_or("—")),
        Icon::ChevronRight,
        Message::SettingsAction(crate::state::SettingsRequest::RevealData),
        open && data.is_some(),
    );
    let data_row = match data {
        Some(path) => data_row.tooltip(path),
        None => data_row,
    }
    .named("settings.data");

    let cache = workspace.storage.cache;
    let cache_detail = if workspace.storage.clearing {
        grana_i18n::settings::clearing(language).to_string()
    } else if let Some(freed) = workspace.storage.freed {
        grana_i18n::settings::freed(language, &super::bytes(freed))
    } else {
        cache.map_or_else(|| "—".to_string(), super::bytes)
    };
    let cache_enabled = open
        && !workspace.storage.clearing
        && !workspace.storage.measuring
        && cache.is_some_and(|bytes| bytes > 0);
    let clear = ctx
        .button_width(
            grana_i18n::common::clear(language),
            Role::Outlined,
            Length::Fixed(76.0),
        )
        .named("settings.cache.clear")
        .enabled(cache_enabled)
        .press(Message::SettingsAction(
            crate::state::SettingsRequest::ClearCache,
        ));
    let cache_row = row::button(
        ctx,
        Icon::Layers,
        grana_i18n::settings::cache(language),
        Some(&cache_detail),
        clear,
        cache_enabled,
    )
    .named("settings.cache");

    vec![
        row::section(
            ctx,
            Some(grana_i18n::settings::interface(language)),
            vec![language_row],
        ),
        row::section(
            ctx,
            Some(grana_i18n::settings::files(language)),
            vec![autosave, data_row, cache_row],
        ),
    ]
}
