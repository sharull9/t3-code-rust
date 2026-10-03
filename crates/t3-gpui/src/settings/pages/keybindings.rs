//! Keybindings: a reference for the fixed shortcuts of this app.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[SearchEntry {
    title: "Keyboard shortcuts",
    description: "Shortcuts for navigating and composing in this app.",
    keywords: &["keybindings", "keys", "hotkeys", "new thread", "sidebar", "composer", "workspace"],
    section: Section::Keybindings,
}];

pub fn modified(_: &SettingsPage, _: &App) -> bool {
    false
}

pub fn restore_defaults(_: &mut SettingsPage, _: &mut Context<SettingsPage>) {}

pub fn render(_: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let shortcuts = [
        ("New thread", "ctrl-n"),
        ("Toggle sidebar", "ctrl-b"),
        ("Focus composer", "ctrl-l"),
        ("Toggle workspace", "ctrl-j"),
        ("Open or close Settings", "ctrl-,"),
        ("Search Settings", "/"),
        ("Back from Settings", "escape"),
        ("Choose a question answer", "ctrl-1"),
    ];
    v_flex()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Question choices use Ctrl+1 through Ctrl+9."),
        )
        .children(shortcuts.into_iter().map(|(label, key)| {
            h_flex()
                .gap_2()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().flex_1().text_sm().child(label))
                .children(Keystroke::parse(key).ok().map(Kbd::new))
        }))
        .into_any_element()
}
