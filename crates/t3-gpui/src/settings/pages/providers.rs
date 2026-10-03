//! Providers: agent provider instances reported by the connected server, and
//! which of their models this device's picker offers.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ServerProvider;

use crate::prefs::Prefs;
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsEvent, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[
    SearchEntry {
        title: "Provider instances",
        description: "Agent providers reported by the connected server.",
        keywords: &["codex", "claude", "cursor", "grok", "opencode", "status", "account", "refresh"],
        section: Section::Providers,
    },
    SearchEntry {
        title: "Models shown in the picker",
        description: "Choose which models the composer's model picker offers. Saved on this device.",
        keywords: &["hide", "show", "visibility", "model picker"],
        section: Section::Providers,
    },
];

/// Hidden models are the only device preference on this page.
pub fn modified(_: &SettingsPage, cx: &App) -> bool {
    !Prefs::global(cx).hidden_models.is_empty()
}

pub fn restore_defaults(_: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    Prefs::update(cx, |prefs| prefs.hidden_models.clear());
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let theme = cx.theme();
    let model_count: usize = page
        .providers
        .iter()
        .map(|provider| provider.models.len())
        .sum();
    let providers = page.providers.iter().map(|provider| {
        let name = provider
            .display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&provider.instance_id);
        let instance = provider.instance_id.clone();
        let expanded = page.expanded_provider.as_deref() == Some(instance.as_str());
        let prefs = Prefs::global(cx);
        let hidden =
            provider.models.iter().filter(|m| prefs.is_hidden(&instance, &m.id)).count();
        let (status, status_color) = if !page.connected {
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
            .children(provider.account().map(|account| {
                h_flex()
                    .id(format!("provider-account-{instance}"))
                    .gap_1p5()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::User).xsmall())
                    .child(div().min_w_0().truncate().child(account.to_owned()))
            }))
            .child(
                h_flex()
                    .gap_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Driver: {}", provider.driver))
                    .child(if hidden > 0 {
                        format!("{} models · {hidden} hidden", provider.models.len())
                    } else {
                        format!("{} models", provider.models.len())
                    })
                    .child(div().flex_1())
                    .when(!provider.models.is_empty(), |row| {
                        let instance = instance.clone();
                        row.child(
                            Button::new(format!("provider-models-{instance}"))
                                .ghost()
                                .xsmall()
                                .label(if expanded { "Hide models" } else { "Choose models" })
                                .icon(Icon::new(if expanded {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.expanded_provider = if this.expanded_provider.as_deref()
                                        == Some(instance.as_str())
                                    {
                                        None
                                    } else {
                                        Some(instance.clone())
                                    };
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(expanded, |row| row.child(render_model_list(page, provider, cx)))
    });
    v_flex().gap_3()
        .child(section_label(format!("{} instances · {model_count} models", page.providers.len()), cx))
        .when(!page.connected && !page.providers.is_empty(), |content| {
            content.child(div().text_sm().text_color(theme.muted_foreground)
                .child("Showing the last reported configuration. Reconnect to refresh provider status."))
        })
        .child(Button::new("settings-refresh-providers").outline().small()
            .icon(Icon::new(IconName::RefreshCw)).label("Refresh providers").disabled(!page.connected)
            .on_click(cx.listener(|this, _, _, cx| {
                if this.connected { cx.emit(SettingsEvent::RefreshProviders); }
            })))
        .child(v_flex().id("settings-provider-list").gap_2().children(providers))
        .when(page.providers.is_empty(), |content| {
            content.child(div().text_sm().text_color(theme.muted_foreground).child(if page.connected {
                "No provider instances were reported by this server."
            } else { "Connect to a server to view provider instances." }))
        }).into_any_element()
}

/// Which of an instance's models the composer picker offers. Saved on
/// this device, like favorites.
fn render_model_list(
    _: &SettingsPage,
    provider: &ServerProvider,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let prefs = Prefs::global(cx);
    let instance = provider.instance_id.clone();
    let all_visible = provider.models.iter().all(|m| !prefs.is_hidden(&instance, &m.id));
    let bulk = {
        let instance = instance.clone();
        let models: Vec<String> = provider.models.iter().map(|m| m.id.clone()).collect();
        Button::new(format!("provider-models-bulk-{instance}"))
            .ghost()
            .xsmall()
            .label(if all_visible { "Hide all" } else { "Show all" })
            .on_click(cx.listener(move |_, _, _, cx| {
                Prefs::set_models_hidden(cx, &instance, models.iter().map(String::as_str), all_visible);
                cx.notify();
            }))
    };
    v_flex()
        .id(format!("provider-model-list-{instance}"))
        .gap_0p5()
        .pt_1()
        .border_t_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .gap_2()
                .py_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(div().flex_1().child("Models shown in the picker. Saved on this device."))
                .child(bulk),
        )
        .children(provider.models.iter().map(|model| {
            let visible = !prefs.is_hidden(&instance, &model.id);
            let (instance, model_id) = (instance.clone(), model.id.clone());
            h_flex()
                .gap_2()
                .py_1()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .text_sm()
                        .when(!visible, |row| row.text_color(theme.muted_foreground))
                        .child(div().flex_shrink_0().child(model.label.clone()))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(model.id.clone()),
                        ),
                )
                .child(
                    Switch::new(SharedString::from(format!("model-visible-{instance}-{model_id}")))
                        .checked(visible)
                        .tooltip(if visible { "Hide from picker" } else { "Show in picker" })
                        .on_change(cx.listener(move |_, checked: &bool, _, cx| {
                            Prefs::set_model_hidden(cx, &instance, &model_id, !*checked);
                            cx.notify();
                        })),
                )
        }))
}

fn section_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div().text_sm().font_medium().text_color(cx.theme().foreground).child(text.into())
}
