//! Appearance: device-local look of this app. Everything here is stored in
//! this device's preferences, not on the server.

use std::ops::RangeInclusive;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::prefs::{
    CODE_FONT_SIZE, ChatWidth, DEFAULT_CODE_FONT_SIZE, DEFAULT_INTERFACE_FONT_SIZE,
    DEFAULT_PROMPT_FONT_SIZE, INTERFACE_FONT_SIZE, PROMPT_FONT_SIZE, Prefs, ThemeMode,
};
use crate::settings::row::{SettingRow, SettingsGroup};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Color scheme",
        description: "Choose the appearance of this app on this device.",
        keywords: &["theme", "dark", "light", "mode", "system", "os"],
        section: Section::Appearance,
    },
    SearchEntry {
        title: "Interface font size",
        description: "Size of text and controls across the app.",
        keywords: &["font", "text", "zoom", "scale", "ui"],
        section: Section::Appearance,
    },
    SearchEntry {
        title: "Prompt font size",
        description: "Size of the text you type in the message box.",
        keywords: &["font", "composer", "input", "message"],
        section: Section::Appearance,
    },
    SearchEntry {
        title: "Code font size",
        description: "Size of diffs, file previews, terminal output and tool details.",
        keywords: &["font", "monospace", "diff", "terminal", "tool"],
        section: Section::Appearance,
    },
    SearchEntry {
        title: "Chat width",
        description: "How wide the conversation and message box may grow.",
        keywords: &["comfortable", "wide", "full", "transcript", "column", "layout"],
        section: Section::Appearance,
    },
    SearchEntry {
        title: "Confirm before archiving",
        description: "Ask before archiving a thread from the sidebar.",
        keywords: &["confirmation", "dialog", "thread", "archive", "prompt"],
        section: Section::Appearance,
    },
];

const THEMES: &[(ThemeMode, &str, IconName)] = &[
    (ThemeMode::System, "System", IconName::Monitor),
    (ThemeMode::Light, "Light", IconName::Sun),
    (ThemeMode::Dark, "Dark", IconName::Moon),
];
const WIDTHS: &[(ChatWidth, &str)] =
    &[(ChatWidth::Comfortable, "Comfortable"), (ChatWidth::Wide, "Wide"), (ChatWidth::Full, "Full")];

pub fn modified(_: &SettingsPage, cx: &App) -> bool {
    Prefs::global(cx).appearance_modified()
}

pub fn restore_defaults(_: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    Prefs::update(cx, Prefs::reset_appearance);
    cx.notify();
}

/// Applies `change` to the device preferences and redraws the page.
fn set(cx: &mut Context<SettingsPage>, change: impl FnOnce(&mut Prefs)) {
    Prefs::update(cx, change);
    cx.notify();
}

pub fn render(_: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let prefs = Prefs::global(cx).clone();
    let defaults = Prefs::default();

    let theme = SettingRow::new("setting-color-scheme", "Color scheme")
        .description("System follows your OS. Saved on this device.")
        .modified(prefs.theme != defaults.theme)
        .on_reset(cx.listener(|_, _, _, cx| set(cx, |prefs| prefs.theme = ThemeMode::System)))
        .control(h_flex().gap_2().children(THEMES.iter().map(|&(mode, label, icon)| {
            Button::new(match mode {
                ThemeMode::System => "theme-system",
                ThemeMode::Light => "theme-light",
                ThemeMode::Dark => "theme-dark",
            })
            .outline()
            .icon(Icon::new(icon))
            .when(prefs.theme == mode, |button| button.primary())
            .label(label)
            .on_click(cx.listener(move |_, _, _, cx| set(cx, |prefs| prefs.theme = mode)))
        })));

    let width = SettingRow::new("setting-chat-width", "Chat width")
        .description("How wide the conversation and message box may grow.")
        .modified(prefs.chat_width != defaults.chat_width)
        .on_reset(cx.listener(|_, _, _, cx| set(cx, |prefs| prefs.chat_width = ChatWidth::Comfortable)))
        .control(h_flex().gap_2().children(WIDTHS.iter().map(|&(choice, label)| {
            Button::new(match choice {
                ChatWidth::Comfortable => "chat-width-comfortable",
                ChatWidth::Wide => "chat-width-wide",
                ChatWidth::Full => "chat-width-full",
            })
            .outline()
            .small()
            .when(prefs.chat_width == choice, |button| button.primary())
            .label(label)
            .on_click(cx.listener(move |_, _, _, cx| set(cx, |prefs| prefs.chat_width = choice)))
        })));

    let interface = size_row(
        "interface",
        "Interface font size",
        "Size of text and controls across the app.",
        prefs.font_size_interface,
        DEFAULT_INTERFACE_FONT_SIZE,
        INTERFACE_FONT_SIZE,
        |prefs, size| prefs.font_size_interface = size,
        cx,
    );
    let prompt = size_row(
        "prompt",
        "Prompt font size",
        "Size of the text you type in the message box.",
        prefs.font_size_prompt,
        DEFAULT_PROMPT_FONT_SIZE,
        PROMPT_FONT_SIZE,
        |prefs, size| prefs.font_size_prompt = size,
        cx,
    );
    let code = size_row(
        "code",
        "Code font size",
        "Size of diffs, file previews, terminal output and tool details.",
        prefs.font_size_code,
        DEFAULT_CODE_FONT_SIZE,
        CODE_FONT_SIZE,
        |prefs, size| prefs.font_size_code = size,
        cx,
    );

    let archive = SettingRow::new("setting-confirm-archive", "Confirm before archiving")
        .description("Ask before archiving a thread from the sidebar.")
        .modified(prefs.confirm_thread_archive != defaults.confirm_thread_archive)
        .on_reset(cx.listener(|_, _, _, cx| set(cx, |prefs| prefs.confirm_thread_archive = false)))
        .control(
            Switch::new("setting-confirm-archive-switch")
                .checked(prefs.confirm_thread_archive)
                .on_change(cx.listener(|_, checked: &bool, _, cx| {
                    let checked = *checked;
                    set(cx, |prefs| prefs.confirm_thread_archive = checked)
                })),
        );

    v_flex()
        .gap_6()
        .child(SettingsGroup::new("Theme").row(theme))
        .child(
            SettingsGroup::new("Layout and text")
                .row(width)
                .row(interface)
                .row(prompt)
                .row(code),
        )
        .child(SettingsGroup::new("Confirmations").row(archive))
        .into_any_element()
}

/// A font-size row: minus, the current size, plus.
#[allow(clippy::too_many_arguments)]
fn size_row(
    key: &'static str,
    title: &'static str,
    description: &'static str,
    value: u32,
    default: u32,
    range: RangeInclusive<u32>,
    apply: fn(&mut Prefs, u32),
    cx: &Context<SettingsPage>,
) -> SettingRow {
    let (min, max) = (*range.start(), *range.end());
    SettingRow::new(format!("setting-font-{key}"), title)
        .description(format!("{description} {min}-{max} px, default {default}."))
        .modified(value != default)
        .on_reset(cx.listener(move |_, _, _, cx| set(cx, |prefs| apply(prefs, default))))
        .control(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("font-{key}-down")))
                        .outline()
                        .small()
                        .icon(Icon::new(IconName::Minus))
                        .disabled(value <= min)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set(cx, |prefs| apply(prefs, value.saturating_sub(1).max(min)))
                        })),
                )
                .child(div().w_12().text_sm().text_center().child(format!("{value} px")))
                .child(
                    Button::new(SharedString::from(format!("font-{key}-up")))
                        .outline()
                        .small()
                        .icon(Icon::new(IconName::Plus))
                        .disabled(value >= max)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            set(cx, |prefs| apply(prefs, (value + 1).min(max)))
                        })),
                ),
        )
}
