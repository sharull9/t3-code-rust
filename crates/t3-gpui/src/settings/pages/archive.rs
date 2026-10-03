//! Archive: placeholder until this page is built. Everything for it lives in this
//! file; `SettingsPage` only calls `render`, `modified` and `restore_defaults`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use crate::settings::SettingsPage;
use crate::settings::search::SearchEntry;

pub const SEARCH: &[SearchEntry] = &[];

pub fn modified(_: &SettingsPage, _: &App) -> bool {
    false
}

pub fn restore_defaults(_: &mut SettingsPage, _: &mut Context<SettingsPage>) {}

pub fn render(_: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child("Archive settings are coming to this app.")
        .into_any_element()
}
