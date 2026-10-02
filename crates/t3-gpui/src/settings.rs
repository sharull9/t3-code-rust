//! Full-page settings for device preferences and the connected environment.
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ServerProvider;

pub enum SettingsEvent {
    Close,
    RefreshProviders,
    ChooseManagedServer,
    SwitchServer,
    /// `true` means light theme; `false` means dark theme.
    Theme(bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Appearance,
    Providers,
    Connections,
    Keyboard,
}
impl Section {
    const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Providers,
        Self::Connections,
        Self::Keyboard,
    ];
    fn id(self) -> &'static str {
        match self {
            Self::Appearance => "settings-nav-appearance",
            Self::Providers => "settings-nav-providers",
            Self::Connections => "settings-nav-connections",
            Self::Keyboard => "settings-nav-keyboard",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Providers => "Providers",
            Self::Connections => "Connections",
            Self::Keyboard => "Keyboard",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Choose the appearance of this app on this device.",
            Self::Providers => "Agent providers reported by the connected server.",
            Self::Connections => "Connect to another server or start a local server.",
            Self::Keyboard => "Shortcuts for navigating and composing in this app.",
        }
    }
}

pub struct SettingsPage {
    providers: Vec<ServerProvider>,
    connected: bool,
    open: bool,
    light_theme: bool,
    section: Section,
    focus_handle: FocusHandle,
}
impl EventEmitter<SettingsEvent> for SettingsPage {}
impl SettingsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            providers: Vec::new(),
            connected: false,
            open: false,
            light_theme: crate::prefs::Prefs::global(cx).light_theme,
            section: Section::Appearance,
            focus_handle: cx.focus_handle(),
        }
    }
    pub fn set_providers(&mut self, providers: Vec<ServerProvider>, cx: &mut Context<Self>) {
        self.providers = providers;
        cx.notify();
    }
    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    fn set_theme(&mut self, light: bool, cx: &mut Context<Self>) {
        if self.light_theme != light {
            self.light_theme = light;
            cx.emit(SettingsEvent::Theme(light));
            cx.notify();
        }
    }
    fn render_appearance(&self, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_3()
            .child(section_label("Color scheme", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("theme-dark")
                            .outline()
                            .icon(Icon::new(IconName::Moon))
                            .when(!self.light_theme, |button| button.primary())
                            .label("Dark")
                            .on_click(cx.listener(|this, _, _, cx| this.set_theme(false, cx))),
                    )
                    .child(
                        Button::new("theme-light")
                            .outline()
                            .icon(Icon::new(IconName::Sun))
                            .when(self.light_theme, |button| button.primary())
                            .label("Light")
                            .on_click(cx.listener(|this, _, _, cx| this.set_theme(true, cx))),
                    ),
            )
            .into_any_element()
    }
    fn render_providers(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let model_count: usize = self
            .providers
            .iter()
            .map(|provider| provider.models.len())
            .sum();
        let providers = self.providers.iter().map(|provider| {
            let name = provider
                .display_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&provider.instance_id);
            let (status, status_color) = if !self.connected {
                ("Offline".to_owned(), theme.muted_foreground)
            } else if !provider.enabled {
                ("Disabled".to_owned(), theme.muted_foreground)
            } else if !provider.installed {
                ("Not installed".to_owned(), theme.warning)
            } else {
                match provider.availability.as_deref() {
                    Some("unavailable") => ("Unavailable".to_owned(), theme.danger),
                    Some(other) => (other.to_owned(), theme.success),
                    None => ("Available".to_owned(), theme.success),
                }
            };
            v_flex()
                .id(format!("provider-row-{}", provider.instance_id))
                .gap_2()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(crate::provider_logo::logo(&provider.driver, px(18.), theme.foreground))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_medium()
                                .child(name.to_owned()),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .text_xs()
                                .text_color(status_color)
                                .child(div().size_1p5().rounded_full().bg(status_color))
                                .child(status),
                        ),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("Driver: {}", provider.driver))
                        .child(format!("{} models", provider.models.len())),
                )
        });
        v_flex().gap_3()
            .child(section_label(format!("{} instances · {model_count} models", self.providers.len()), cx))
            .when(!self.connected && !self.providers.is_empty(), |content| {
                content.child(div().text_sm().text_color(theme.muted_foreground)
                    .child("Showing the last reported configuration. Reconnect to refresh provider status."))
            })
            .child(Button::new("settings-refresh-providers").outline().small()
                .icon(Icon::new(IconName::RefreshCw)).label("Refresh providers").disabled(!self.connected)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.connected { cx.emit(SettingsEvent::RefreshProviders); }
                })))
            .child(v_flex().id("settings-provider-list").gap_2().children(providers))
            .when(self.providers.is_empty(), |content| {
                content.child(div().text_sm().text_color(theme.muted_foreground).child(if self.connected {
                    "No provider instances were reported by this server."
                } else { "Connect to a server to view provider instances." }))
            }).into_any_element()
    }
    fn render_connections(&self, cx: &Context<Self>) -> AnyElement {
        let managed_server_dir = dirs::data_local_dir()
            .map(|path| path.join("t3-gpui").join("server").display().to_string())
            .unwrap_or_else(|| "the local application data folder/t3-gpui/server".to_owned());
        v_flex().gap_4()
            .child(v_flex().gap_2().child(section_label("Server connection", cx))
                .child(div().text_sm().child(if self.connected { "Connected" } else { "Offline" }))
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
    fn render_keyboard(&self, cx: &App) -> AnyElement {
        let shortcuts = [
            ("New thread", "ctrl-n"),
            ("Toggle sidebar", "ctrl-b"),
            ("Focus composer", "ctrl-l"),
            ("Toggle workspace", "ctrl-j"),
            ("Open or close Settings", "ctrl-,"),
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
}
impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let wide = window.viewport_size().width >= px(1000.);
        let navigation = div()
            .flex()
            .gap_1()
            .p_3()
            .flex_shrink_0()
            .when(wide, |nav| nav.flex_col().w(px(164.)).border_r_1())
            .when(!wide, |nav| nav.flex_row().flex_wrap().border_b_1())
            .border_color(theme.border)
            .children(Section::ALL.into_iter().map(|section| {
                Button::new(section.id())
                    .ghost()
                    .small()
                    .label(section.label())
                    .when(wide, |button| button.w_full())
                    .when(self.section == section, |button| button.primary())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.section = section;
                        this.focus(window, cx);
                        cx.notify();
                    }))
            }));
        let content = match self.section {
            Section::Appearance => self.render_appearance(cx),
            Section::Providers => self.render_providers(cx),
            Section::Connections => self.render_connections(cx),
            Section::Keyboard => self.render_keyboard(cx),
        };
        v_flex()
            .id("settings-page")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(theme.background)
            .child(
                h_flex()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Icon::new(IconName::Settings)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().flex_1().text_base().font_semibold().child("Settings"))
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::ArrowLeft))
                            .label("Back")
                            .tooltip("Back to previous view (Esc)")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .when(!wide, |body| body.flex_col())
                    .child(navigation)
                    .child(
                        div()
                            .id(self.section.id().to_owned() + "-content")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scrollbar()
                            .child(
                                v_flex()
                                    .w_full()
                                    .max_w(px(760.))
                                    .p_4()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_lg()
                                                    .font_semibold()
                                                    .child(self.section.label()),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(theme.muted_foreground)
                                                    .child(self.section.description()),
                                            ),
                                    )
                                    .child(content),
                            ),
                    ),
            )
            .into_any_element()
    }
}
fn section_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .font_medium()
        .text_color(cx.theme().foreground)
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[gpui_kit::test]
    fn offline_refresh_is_disabled_and_category_navigation_keeps_theme(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(SettingsPage::new),
            )
            .unwrap()
        });
        let providers = serde_json::from_value::<Vec<ServerProvider>>(json!([
            {"instanceId":"claudeAgent","driver":"claude","displayName":"Claude","enabled":true,"installed":true,"models":[{"slug":"m1","name":"Model 1"}]}
        ])).unwrap();
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_providers(providers, cx);
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&page, move |_, event: &SettingsEvent, _| {
                captured.borrow_mut().push(match event {
                    SettingsEvent::Close => "close",
                    SettingsEvent::RefreshProviders => "refresh",
                    SettingsEvent::ChooseManagedServer => "managed",
                    SettingsEvent::SwitchServer => "switch",
                    SettingsEvent::Theme(true) => "light",
                    SettingsEvent::Theme(false) => "dark",
                });
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-light", cx);
            window.click("settings-nav-providers", cx);
            window.render_frame(cx);
            window.click("settings-refresh-providers", cx);
            page.update(cx, |page, cx| page.set_connected(true, cx));
            window.render_frame(cx);
            window.click("settings-refresh-providers", cx);
            window.click("settings-nav-connections", cx);
            window.render_frame(cx);
            window.click("settings-managed-server", cx);
            window.click("settings-switch-server", cx);
            window.click("settings-nav-appearance", cx);
            window.render_frame(cx);
            assert!(page.read(cx).light_theme);
            window.click("theme-dark", cx);
            window.click("settings-close", cx);
        })
        .unwrap();
        assert_eq!(
            *events.borrow(),
            ["light", "refresh", "managed", "switch", "dark", "close"]
        );
    }
}
