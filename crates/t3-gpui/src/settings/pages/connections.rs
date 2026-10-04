//! Connections: which server this client talks to.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsEvent, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Server connection",
        description: "Connect to another server.",
        keywords: &["switch", "pair", "pairing", "remote", "offline"],
        section: Section::Connections,
    },
    SearchEntry {
        title: "Local server",
        description: "Run T3 on this machine with the shared ~/.t3 data.",
        keywords: &["managed", "executable", "data directory", "local", "embedded"],
        section: Section::Connections,
    },
];

pub fn modified(_: &SettingsPage, _: &App) -> bool {
    false
}

pub fn restore_defaults(_: &mut SettingsPage, _: &mut Context<SettingsPage>) {}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let t3_home = crate::managed_server::default_t3_home()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.t3".to_owned());
    v_flex().gap_4()
        .child(v_flex().gap_2().child(section_label("Server connection", cx))
            .child(div().text_sm().child(if page.connected { "Connected" } else { "Offline" }))
            .child(Button::new("settings-switch-server").outline().small().label("Switch server")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::SwitchServer)))))
        .child(v_flex().gap_2().child(section_label("Local server", cx))
            .child(div().text_sm().text_color(cx.theme().muted_foreground)
                .child("Run T3 on this machine with the same projects and threads as the T3 Code \n                        app. If T3 Code is already running, this connects to its server; otherwise \n                        it starts one."))
            .child(h_flex().gap_2()
                .child(Button::new("settings-local-server").outline().small().icon(Icon::new(IconName::Server))
                    .label("Use T3 on this machine")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::StartLocalServer))))
                .child(Button::new("settings-managed-server").ghost().small()
                    .label("Choose executable…")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::ChooseManagedServer)))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child(format!("Data directory: {t3_home}"))))
        .into_any_element()
}

fn section_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div().text_sm().font_medium().text_color(cx.theme().foreground).child(text.into())
}
