//! Connections: which server this client talks to.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, v_flex};
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
        description: "Start a server on this machine from a T3 server executable.",
        keywords: &["managed", "executable", "data directory"],
        section: Section::Connections,
    },
];

pub fn modified(_: &SettingsPage, _: &App) -> bool {
    false
}

pub fn restore_defaults(_: &mut SettingsPage, _: &mut Context<SettingsPage>) {}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let managed_server_dir = dirs::data_local_dir()
        .map(|path| path.join("t3-gpui").join("server").display().to_string())
        .unwrap_or_else(|| "the local application data folder/t3-gpui/server".to_owned());
    v_flex().gap_4()
        .child(v_flex().gap_2().child(section_label("Server connection", cx))
            .child(div().text_sm().child(if page.connected { "Connected" } else { "Offline" }))
            .child(Button::new("settings-switch-server").outline().small().label("Switch server")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::SwitchServer)))))
        .child(v_flex().gap_2().child(section_label("Local server", cx))
            .child(div().text_sm().text_color(cx.theme().muted_foreground)
                .child("Choose a compatible T3 server executable to start a server on this machine."))
            .child(Button::new("settings-managed-server").outline().small().icon(Icon::new(IconName::Server))
                .label("Choose local server executable…")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::ChooseManagedServer))))
            .child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child(format!("Data directory: {managed_server_dir}"))))
        .into_any_element()
}

fn section_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div().text_sm().font_medium().text_color(cx.theme().foreground).child(text.into())
}
