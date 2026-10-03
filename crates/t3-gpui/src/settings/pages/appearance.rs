//! Appearance: device-local look of this app.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::row::{SettingRow, SettingsGroup};
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[SearchEntry {
    title: "Color scheme",
    description: "Choose the appearance of this app on this device.",
    keywords: &["theme", "dark", "light", "mode"],
    section: Section::Appearance,
}];

pub fn modified(page: &SettingsPage, _: &App) -> bool {
    page.light_theme
}

pub fn restore_defaults(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    page.set_theme(false, cx);
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let light = page.light_theme;
    SettingsGroup::new("Theme")
        .child(
            SettingRow::new("setting-color-scheme", "Color scheme")
                .description("Saved on this device.")
                .modified(light)
                .on_reset(cx.listener(|this, _, _, cx| this.set_theme(false, cx)))
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("theme-dark")
                                .outline()
                                .icon(Icon::new(IconName::Moon))
                                .when(!light, |button| button.primary())
                                .label("Dark")
                                .on_click(cx.listener(|this, _, _, cx| this.set_theme(false, cx))),
                        )
                        .child(
                            Button::new("theme-light")
                                .outline()
                                .icon(Icon::new(IconName::Sun))
                                .when(light, |button| button.primary())
                                .label("Light")
                                .on_click(cx.listener(|this, _, _, cx| this.set_theme(true, cx))),
                        ),
                ),
        )
        .into_any_element()
}
