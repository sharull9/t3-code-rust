//! Compact settings surface for server configuration and appearance.
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ServerProvider;

pub enum SettingsEvent {
    RefreshProviders,
    ChooseManagedServer,
    SwitchServer,
    /// `true` means light theme; `false` means dark theme.
    Theme(bool),
}

pub struct SettingsPanel {
    providers: Vec<ServerProvider>,
    connected: bool,
    open: bool,
    light_theme: bool,
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

impl SettingsPanel {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self { providers: Vec::new(), connected: false, open: false, light_theme: false }
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

    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
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
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let max_height = window.viewport_size().height * 0.4;
        let managed_server_dir = dirs::data_local_dir()
            .map(|path| path.join("t3-gpui").join("server").display().to_string())
            .unwrap_or_else(|| "the local application data folder/t3-gpui/server".to_owned());
        let model_count: usize = self.providers.iter().map(|provider| provider.models.len()).sum();
        let providers = self.providers.iter().map(|provider| {
            let name = provider
                .display_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&provider.instance_id);
            let status = if !provider.enabled {
                "Disabled".to_owned()
            } else if !provider.installed {
                "Not installed".to_owned()
            } else {
                provider.availability.as_deref().unwrap_or("Available").to_owned()
            };
            v_flex()
                .id(format!("provider-row-{}", provider.instance_id))
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().text_sm().font_medium().child(name.to_owned()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(status)),
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

        let server_dir = div()
            .min_w_0()
            .truncate()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(format!("Local server data: {managed_server_dir}"));
        let server_actions = v_flex()
            .w_full()
            .gap_1()
            .child(
                Button::new("settings-managed-server")
                    .primary()
                    .small()
                    .w_full()
                    .label("Choose local server executable…")
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(SettingsEvent::ChooseManagedServer);
                    })),
            )
            .child(
                Button::new("settings-refresh-providers")
                    .ghost()
                    .small()
                    .w_full()
                    .label(if self.connected {
                        "Refresh providers"
                    } else {
                        "Refresh providers (offline)"
                    })
                    .disabled(!self.connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.connected {
                            cx.emit(SettingsEvent::RefreshProviders);
                        }
                    })),
            );
        let provider_list =
            v_flex().gap_2().children(providers).when(self.providers.is_empty(), |list| {
                list.child(div().p_2().text_sm().text_color(theme.muted_foreground).child(
                    if self.connected {
                        "No provider instances were reported by this server."
                    } else {
                        "Connect to a server to view provider instances."
                    },
                ))
            });

        let appearance =
            v_flex().gap_2().child(div().text_sm().font_medium().child("Appearance")).child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("theme-dark")
                            .small()
                            .when(!self.light_theme, |button| button.primary())
                            .label("Dark")
                            .on_click(cx.listener(|this, _, _, cx| this.set_theme(false, cx))),
                    )
                    .child(
                        Button::new("theme-light")
                            .small()
                            .when(self.light_theme, |button| button.primary())
                            .label("Light")
                            .on_click(cx.listener(|this, _, _, cx| this.set_theme(true, cx))),
                    ),
            );
        let server = v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_sm().font_medium().child("Server"))
                    .child(
                        Button::new("settings-switch-server")
                            .ghost()
                            .xsmall()
                            .label("Switch server")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SettingsEvent::SwitchServer);
                            })),
                    ),
            )
            .child(server_dir)
            .child(server_actions)
            .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                "{} provider instances · {model_count} models",
                self.providers.len()
            )))
            .child(div().id("settings-provider-list").child(provider_list));
        let scroll_content = v_flex().gap_3().child(appearance).child(server);

        v_flex()
            .id("settings-panel")
            .test_support()
            .on_click(|_, _, cx| cx.stop_propagation())
            .w_full()
            .h(max_height)
            .max_h(max_height)
            .min_h_0()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .bg(theme.sidebar)
            .shadow_lg()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_sm().font_semibold().child("Settings"))
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .xsmall()
                            .label("Done")
                            .on_click(cx.listener(|this, _, _, cx| this.set_open(false, cx))),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(scroll_content),
            )
            .into_any_element()
    }
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

    fn panel(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<SettingsPanel>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(SettingsPanel::new),
            )
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn offline_refresh_is_disabled_and_theme_choices_emit(cx: &mut TestAppContext) {
        let (handle, panel) = panel(cx);
        let providers = serde_json::from_value::<Vec<ServerProvider>>(json!([
            {"instanceId":"claudeAgent","driver":"claude","displayName":"Claude","enabled":true,"installed":true,"models":[{"slug":"m1","name":"Model 1"}]}
        ])).unwrap();
        panel.update(cx, |panel, cx| {
            panel.set_open(true, cx);
            panel.set_providers(providers, cx);
            panel.set_connected(false, cx);
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&panel, move |_, event: &SettingsEvent, _| match event {
                SettingsEvent::RefreshProviders => captured.borrow_mut().push("refresh"),
                SettingsEvent::ChooseManagedServer => captured.borrow_mut().push("managed"),
                SettingsEvent::SwitchServer => captured.borrow_mut().push("switch"),
                SettingsEvent::Theme(true) => captured.borrow_mut().push("light"),
                SettingsEvent::Theme(false) => captured.borrow_mut().push("dark"),
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-refresh-providers", cx);
            window.click("theme-light", cx);
            window.click("theme-dark", cx);
            window.click("settings-close", cx);
        })
        .unwrap();
        assert_eq!(*events.borrow(), ["light", "dark"]);
        assert!(!cx.update(|cx| panel.read(cx).is_open()));
    }
}
