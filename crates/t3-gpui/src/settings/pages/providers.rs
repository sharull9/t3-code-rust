//! Providers: agent provider instances reported by the connected server, and
//! which of their models this device's picker offers.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use t3_client::ServerProvider;
use t3_client::provider_config::{self as config, Control, Field, FieldValue, InstanceConfig};

use crate::prefs::Prefs;
use crate::settings::row::{SettingRow, SettingsGroup, choice_button};
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
        title: "Provider configuration",
        description: "Enable an instance and set its binary path, home directory, launch arguments and credentials.",
        keywords: &[
            "enable", "disable", "binary path", "launch arguments", "api key", "password", "server url",
            "home", "codex_home", "claude_config_dir", "sign-in", "gcp", "antigravity", "endpoint",
        ],
        section: Section::Providers,
    },
    SearchEntry {
        title: "Custom models",
        description: "Add model slugs an instance offers in addition to the ones it reports.",
        keywords: &["slug", "model", "add model", "remove model"],
        section: Section::Providers,
    },
    SearchEntry {
        title: "Add provider instance",
        description: "Create another instance of a driver, for example a second Codex account.",
        keywords: &["new instance", "second account", "driver", "remove instance", "delete"],
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

/// Editing state of the page: which instance's configuration is open, the text
/// inputs behind it (created lazily, see [`sync`]) and the add-instance form.
#[derive(Default)]
pub struct UiState {
    /// One grid of every instance instead of a grid per driver.
    ungrouped: bool,
    /// The instance whose configuration form is open.
    configuring: Option<String>,
    /// The instance awaiting a "remove" confirmation.
    confirm_remove: Option<String>,
    add_open: bool,
    add_driver: Option<&'static str>,
    add_error: Option<String>,
    /// `(instance id, field key)`; the pseudo keys are [`NAME`], [`MODEL`]
    /// (instance id set) and [`ADD_NAME`] (empty instance id).
    inputs: HashMap<(String, String), Entity<InputState>>,
    subscriptions: Vec<Subscription>,
}

const NAME: &str = "@name";
const MODEL: &str = "@model";
const ADD_NAME: &str = "@add-name";

struct Row<'a> {
    id: String,
    live: Option<&'a ServerProvider>,
    config: Option<InstanceConfig>,
}

impl Row<'_> {
    fn driver(&self) -> String {
        match (&self.config, self.live) {
            (Some(config), _) => config.driver.clone(),
            (None, Some(live)) => live.driver.clone(),
            (None, None) => String::new(),
        }
    }
    fn enabled(&self) -> bool {
        match (&self.config, self.live) {
            (Some(config), _) => config.enabled(),
            (None, Some(live)) => live.enabled,
            (None, None) => false,
        }
    }
    fn name(&self) -> String {
        let driver_label = config::driver(&self.driver()).map(|driver| driver.label.to_owned());
        self.config
            .as_ref()
            .and_then(|config| config.display_name().map(str::to_owned))
            .or_else(|| {
                self.live
                    .and_then(|live| live.display_name.as_deref())
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
            .or(driver_label)
            .unwrap_or_else(|| self.id.clone())
    }
}

/// Configured instances joined with what the server reports, then reported
/// instances nothing is configured for. Cursor's default slot only shows once
/// the server reports it, like upstream.
fn rows(page: &SettingsPage) -> Vec<Row<'_>> {
    let settings = page.server_settings.is_some().then(|| page.effective_settings().into_owned());
    let mut rows: Vec<Row> = settings
        .iter()
        .flat_map(|settings| settings.provider_instances())
        .filter(|instance| {
            !(instance.is_default
                && instance.driver == "cursor"
                && !page.providers.iter().any(|live| live.instance_id == instance.id))
        })
        .map(|instance| Row {
            id: instance.id.clone(),
            live: page.providers.iter().find(|live| live.instance_id == instance.id),
            config: Some(instance),
        })
        .collect();
    for live in &page.providers {
        if !rows.iter().any(|row| row.id == live.instance_id) {
            rows.push(Row { id: live.instance_id.clone(), live: Some(live), config: None });
        }
    }
    rows
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let theme = cx.theme();
    let rows = rows(page);
    let model_count: usize = page.providers.iter().map(|provider| provider.models.len()).sum();
    let ready = page.server_ready();
    let grouped = !page.providers_ui.ungrouped;
    let list = if grouped {
        // Drivers in the order their first instance appears.
        let mut drivers: Vec<String> = Vec::new();
        for row in &rows {
            let driver = row.driver();
            if !drivers.contains(&driver) {
                drivers.push(driver);
            }
        }
        v_flex()
            .id("settings-provider-list")
            .gap_5()
            .children(drivers.into_iter().map(|driver| {
                let members: Vec<&Row> = rows.iter().filter(|row| row.driver() == driver).collect();
                let label = config::driver(&driver)
                    .map(|driver| driver.label.to_owned())
                    .unwrap_or_else(|| crate::ui::provider_label(Some(&driver)));
                v_flex()
                    .id(SharedString::from(format!("provider-group-{driver}")))
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(crate::provider_logo::logo(&driver, px(16.), theme.foreground))
                            .child(div().text_sm().font_semibold().child(label))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(match members.len() {
                                        1 => "1 instance".to_owned(),
                                        count => format!("{count} instances"),
                                    }),
                            ),
                    )
                    .child(card_grid(members.iter().map(|row| render_card(page, row, grouped, cx))))
            }))
            .into_any_element()
    } else {
        div()
            .id("settings-provider-list")
            .child(card_grid(rows.iter().map(|row| render_card(page, row, grouped, cx))))
            .into_any_element()
    };
    v_flex()
        .gap_3()
        .child(section_label(format!("{} instances · {model_count} models", rows.len()), cx))
        .when(!page.connected && !rows.is_empty(), |content| {
            content.child(div().text_sm().text_color(theme.muted_foreground)
                .child("Showing the last reported configuration. Reconnect to refresh provider status and change settings."))
        })
        .child(
            h_flex()
                .gap_2()
                .child(Button::new("settings-refresh-providers").outline().small()
                    .icon(Icon::new(IconName::RefreshCw)).label("Refresh providers").disabled(!page.connected)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.connected { cx.emit(SettingsEvent::RefreshProviders); }
                    })))
                .child(
                    Button::new("provider-add-instance")
                        .outline()
                        .small()
                        .icon(Icon::new(IconName::Plus))
                        .label("Add instance")
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let ui = &mut this.providers_ui;
                            ui.add_open = !ui.add_open;
                            ui.add_error = None;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    Button::new("provider-group-toggle")
                        .small()
                        .when(grouped, |button| button.primary())
                        .when(!grouped, |button| button.outline())
                        .icon(Icon::new(IconName::Layers))
                        .label("Group by provider")
                        .tooltip(if grouped {
                            "Show every instance in one grid"
                        } else {
                            "Group instances by provider"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.providers_ui.ungrouped = !this.providers_ui.ungrouped;
                            cx.notify();
                        })),
                ),
        )
        .when(page.providers_ui.add_open, |content| content.child(render_add_form(page, cx)))
        .child(list)
        .when(rows.is_empty(), |content| {
            content.child(div().text_sm().text_color(theme.muted_foreground).child(if page.connected {
                "No provider instances were reported by this server."
            } else { "Connect to a server to view provider instances." }))
        }).into_any_element()
}

/// Instance cards two to a row; an open card spans the row (see `render_card`).
fn card_grid(cards: impl IntoIterator<Item = impl IntoElement>) -> impl IntoElement {
    div().grid().grid_cols(2).gap_2().children(cards)
}

fn render_card(
    page: &SettingsPage,
    row: &Row,
    grouped: bool,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let name = row.name();
    let instance = row.id.clone();
    let driver = row.driver();
    let enabled = row.enabled();
    let models = row.live.map_or(0, |live| live.models.len());
    let expanded = page.expanded_provider.as_deref() == Some(instance.as_str());
    let configuring = page.providers_ui.configuring.as_deref() == Some(instance.as_str());
    let prefs = Prefs::global(cx);
    let hidden = row
        .live
        .map_or(0, |live| live.models.iter().filter(|m| prefs.is_hidden(&instance, &m.id)).count());
    let (status, status_color) = if !page.connected {
        ("Offline".to_owned(), theme.muted_foreground)
    } else if !enabled {
        ("Disabled".to_owned(), theme.muted_foreground)
    } else if let Some(live) = row.live {
        if !live.installed {
            ("Not installed".to_owned(), theme.warning)
        } else {
            match live.availability.as_deref() {
                Some("unavailable") => ("Unavailable".to_owned(), theme.danger),
                Some(other) => (other.to_owned(), theme.success),
                None => ("Available".to_owned(), theme.success),
            }
        }
    } else {
        ("Not reported yet".to_owned(), theme.muted_foreground)
    };
    let can_edit = page.server_ready() && row.config.is_some();
    let switch = {
        let instance = instance.clone();
        Switch::new(SharedString::from(format!("provider-enabled-{instance}")))
            .checked(enabled)
            .disabled(!can_edit)
            .tooltip(if enabled { "Disable this instance" } else { "Enable this instance" })
            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                let checked = *checked;
                edit_instance(this, &instance, cx, |envelope| config::set_enabled(envelope, checked));
            }))
    };
    v_flex()
        .id(format!("provider-row-{}", row.id))
        .min_w_0()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        // The configuration form and model list need the full width.
        .when(configuring || expanded, |card| card.col_span_full())
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(crate::provider_logo::logo(&driver, px(18.), theme.foreground))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_medium()
                        .child(name.clone()),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(status_color)
                        .child(div().size_1p5().rounded_full().bg(status_color))
                        .child(status),
                )
                .child(switch),
        )
        .children(row.live.and_then(|live| live.account()).map(|account| {
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
                // Grouped, the group header already names the driver.
                .when(!grouped, |line| line.child(format!("Driver: {driver}")))
                .child(if hidden > 0 {
                    format!("{models} models · {hidden} hidden")
                } else {
                    format!("{models} models")
                }),
        )
        // Pinned to the card's bottom so actions line up across a row.
        .child(div().flex_1())
        .child(
            h_flex()
                .gap_1()
                .justify_end()
                .when(row.config.is_some(), |line| {
                    let instance = instance.clone();
                    line.child(
                        Button::new(format!("provider-config-{instance}"))
                            .ghost()
                            .xsmall()
                            .label(if configuring { "Hide configuration" } else { "Configure" })
                            .icon(Icon::new(if configuring {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let ui = &mut this.providers_ui;
                                ui.configuring = if ui.configuring.as_deref() == Some(instance.as_str()) {
                                    None
                                } else {
                                    Some(instance.clone())
                                };
                                ui.confirm_remove = None;
                                cx.notify();
                            })),
                    )
                })
                .when(models > 0, |line| {
                    let instance = instance.clone();
                    line.child(
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
        .when_some(row.config.as_ref().filter(|_| configuring), |card, config| {
            card.child(render_config(page, config, can_edit, cx))
        })
        .when(expanded, |card| match row.live {
            Some(live) => card.child(render_model_list(page, live, cx)),
            None => card,
        })
}

/// The configuration form for one instance: name, the driver's fields,
/// custom models and (for custom instances) removal.
fn render_config(
    page: &SettingsPage,
    instance: &InstanceConfig,
    can_edit: bool,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = instance.id.clone();
    let driver = config::driver(&instance.driver);
    let input_for = |key: &str| page.providers_ui.inputs.get(&(id.clone(), key.to_owned()));
    let mut group = SettingsGroup::new("Configuration");
    group = group.row(
        SettingRow::new(format!("provider-field-{id}-name"), "Display name")
            .description("Shown in the model picker and thread headers.")
            .control(div().children(
                input_for(NAME).map(|state| Input::new(state).small().w(px(260.)).disabled(!can_edit)),
            )),
    );
    for field in driver.map_or(&[][..], |driver| driver.fields) {
        group = group.row(render_field(page, instance, field, input_for(field.key), can_edit, cx));
    }
    if driver.is_none() {
        group = group.child(
            div()
                .px_4()
                .py_3()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!(
                    "The driver \"{}\" is not known to this app. Its configuration is kept as it is.",
                    instance.driver
                )),
        );
    }
    let mut panel = v_flex()
        .id(format!("provider-config-panel-{id}"))
        .test_support()
        .gap_3()
        .pt_2()
        .border_t_1()
        .border_color(theme.border)
        .child(group);
    if driver.is_some_and(|driver| driver.has_custom_models()) {
        panel = panel.child(render_custom_models(instance, input_for(MODEL), can_edit, cx));
    }
    if !instance.is_default {
        panel = panel.child(render_remove(page, instance, can_edit, cx));
    }
    panel
}

fn render_field(
    page: &SettingsPage,
    instance: &InstanceConfig,
    field: &'static Field,
    input: Option<&Entity<InputState>>,
    can_edit: bool,
    cx: &Context<SettingsPage>,
) -> SettingRow {
    let id = instance.id.clone();
    let row_id = format!("provider-field-{id}-{}", field.key);
    let row = SettingRow::new(row_id.clone(), field.label).description(field.description);
    match field.control {
        Control::Text | Control::Password => row.control(div().children(input.map(|state| {
            let mut input = Input::new(state).small().w(px(260.)).disabled(!can_edit);
            if field.control == Control::Password {
                input = input.mask_toggle();
            }
            input
        }))),
        Control::Switch => row.control(
            Switch::new(SharedString::from(format!("{row_id}-switch")))
                .checked(instance.bool_value(field.key))
                .disabled(!can_edit)
                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                    let checked = *checked;
                    edit_instance(this, &id, cx, |envelope| {
                        config::set_field(envelope, field, &FieldValue::Bool(checked))
                    });
                })),
        ),
        Control::Select(options) => {
            let stored = instance.text_value(field.key);
            let current = options
                .iter()
                .find(|(value, _)| *value == stored)
                .or(options.first())
                .map_or(stored.clone(), |(_, label)| (*label).to_owned());
            let selected = if stored.is_empty() { options.first().map(|(value, _)| *value) } else { None };
            let view = cx.entity();
            let _ = page;
            row.control(choice_button(format!("{row_id}-select"), current, !can_edit, move |mut menu| {
                for &(value, label) in options {
                    let (view, id) = (view.clone(), id.clone());
                    let checked = stored == value || (selected == Some(value));
                    menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                // The first option is the default: stored as no value.
                                let text = if Some(&(value, label)) == options.first() {
                                    String::new()
                                } else {
                                    value.to_owned()
                                };
                                edit_instance(this, &id, cx, |envelope| {
                                    config::set_field(envelope, field, &FieldValue::Text(text))
                                });
                            });
                        },
                    ));
                }
                menu
            }))
        }
    }
}

fn render_custom_models(
    instance: &InstanceConfig,
    input: Option<&Entity<InputState>>,
    can_edit: bool,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = instance.id.clone();
    let models = instance.custom_models();
    let add = {
        let id = id.clone();
        Button::new(format!("provider-model-add-{id}"))
            .outline()
            .small()
            .icon(Icon::new(IconName::Plus))
            .label("Add")
            .disabled(!can_edit)
            .on_click(cx.listener(move |this, _, window, cx| add_custom_model(this, &id, window, cx)))
    };
    let mut group = SettingsGroup::new("Custom models").child(
        SettingRow::new(format!("provider-custom-models-{id}"), "Add a model")
            .description("A model slug this instance offers besides the ones it reports. Press Enter to add.")
            .control(
                h_flex()
                    .gap_2()
                    .children(input.map(|state| Input::new(state).small().w(px(200.)).disabled(!can_edit)))
                    .child(add),
            ),
    );
    for slug in models {
        let remove_id = id.clone();
        let remove_slug = slug.clone();
        group = group.child(
            h_flex()
                .id(format!("provider-custom-model-{id}-{slug}"))
                .gap_2()
                .px_4()
                .py_2()
                .items_center()
                .child(div().flex_1().min_w_0().truncate().text_sm().child(slug.clone()))
                .child(div().text_xs().text_color(theme.muted_foreground).child("Custom"))
                .child(
                    Button::new(format!("provider-model-remove-{id}-{slug}"))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X))
                        .tooltip("Remove model")
                        .disabled(!can_edit)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            edit_instance(this, &remove_id, cx, |envelope| {
                                config::remove_custom_model(envelope, &remove_slug)
                            });
                        })),
                ),
        );
    }
    group
}

fn render_remove(
    page: &SettingsPage,
    instance: &InstanceConfig,
    can_edit: bool,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = instance.id.clone();
    let confirming = page.providers_ui.confirm_remove.as_deref() == Some(id.as_str());
    h_flex()
        .id(format!("provider-remove-{id}"))
        .gap_2()
        .items_center()
        .when(!confirming, |row| {
            let id = id.clone();
            row.child(
                Button::new(format!("provider-remove-start-{id}"))
                    .outline()
                    .small()
                    .label("Remove instance")
                    .disabled(!can_edit)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.providers_ui.confirm_remove = Some(id.clone());
                        cx.notify();
                    })),
            )
        })
        .when(confirming, |row| {
            let (confirm_id, cancel_id) = (id.clone(), id.clone());
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(format!(
                        "Remove \"{}\"? Its configuration is deleted. Threads that used it keep their history.",
                        instance.display_name().unwrap_or(&instance.id)
                    )),
            )
            .child(
                Button::new(format!("provider-remove-cancel-{cancel_id}"))
                    .ghost()
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.providers_ui.confirm_remove = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new(format!("provider-remove-confirm-{confirm_id}"))
                    .danger()
                    .small()
                    .label("Remove")
                    .disabled(!can_edit)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        remove_instance(this, &confirm_id, cx);
                    })),
            )
        })
}

fn render_add_form(page: &SettingsPage, cx: &Context<SettingsPage>) -> impl IntoElement {
    let theme = cx.theme();
    let ui = &page.providers_ui;
    let driver = ui.add_driver.and_then(config::driver).unwrap_or(&config::DRIVERS[0]);
    let name = ui
        .inputs
        .get(&(String::new(), ADD_NAME.to_owned()))
        .map(|state| state.read(cx).value().to_string())
        .unwrap_or_default();
    let derived = config::derive_instance_id(driver.kind, &name);
    let view = cx.entity();
    let kind = driver.kind;
    v_flex()
        .id("provider-add-form")
        .gap_2()
        .child(
            SettingsGroup::new("Add provider instance")
                .row(
                    SettingRow::new("provider-add-driver-row", "Driver")
                        .description("Which agent this instance runs.")
                        .control(choice_button(
                            "provider-add-driver",
                            driver.label,
                            false,
                            move |mut menu| {
                                for option in config::DRIVERS {
                                    let view = view.clone();
                                    let driver = option.kind;
                                    menu = menu.item(
                                        PopupMenuItem::new(option.label).checked(option.kind == kind).on_click(
                                            move |_, _, cx| {
                                                view.update(cx, |this, cx| {
                                                    this.providers_ui.add_driver = Some(driver);
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                menu
                            },
                        )),
                )
                .row(
                    SettingRow::new("provider-add-name-row", "Name")
                        .description(if derived.is_empty() {
                            "The instance id is derived from the name.".to_owned()
                        } else {
                            format!("Instance id: {derived}")
                        })
                        .control(div().children(
                            ui.inputs
                                .get(&(String::new(), ADD_NAME.to_owned()))
                                .map(|state| Input::new(state).small().w(px(260.))),
                        )),
                ),
        )
        .children(
            ui.add_error
                .clone()
                .map(|error| div().id("provider-add-error").text_xs().text_color(theme.danger).child(error)),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("provider-add-confirm")
                        .small()
                        .label("Add instance")
                        .disabled(!page.server_ready())
                        .on_click(cx.listener(|this, _, window, cx| add_instance(this, window, cx))),
                )
                .child(
                    Button::new("provider-add-cancel").ghost().small().label("Cancel").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.providers_ui.add_open = false;
                            this.providers_ui.add_error = None;
                            cx.notify();
                        }),
                    ),
                ),
        )
}

// Writes.

/// Applies `edit` to an instance's envelope and saves the result. Nothing is
/// sent when the edit changes nothing.
fn edit_instance(
    page: &mut SettingsPage,
    id: &str,
    cx: &mut Context<SettingsPage>,
    edit: impl FnOnce(&mut Value),
) {
    let settings = page.effective_settings().into_owned();
    let Some(instance) = settings.provider_instance(id) else { return };
    let mut envelope = instance.envelope.clone();
    edit(&mut envelope);
    if envelope == instance.envelope {
        return;
    }
    let patch = settings.instance_update_patch(&instance, envelope);
    page.save(patch, cx);
}

fn remove_instance(page: &mut SettingsPage, id: &str, cx: &mut Context<SettingsPage>) {
    let settings = page.effective_settings().into_owned();
    // Default slots are not removable: they would come back from the legacy object.
    if settings.provider_instance(id).is_none_or(|instance| instance.is_default) {
        return;
    }
    let patch = settings.instance_remove_patch(id);
    let ui = &mut page.providers_ui;
    ui.confirm_remove = None;
    if ui.configuring.as_deref() == Some(id) {
        ui.configuring = None;
    }
    ui.inputs.retain(|(instance, _), _| instance != id);
    page.save(patch, cx);
}

fn add_instance(page: &mut SettingsPage, window: &mut Window, cx: &mut Context<SettingsPage>) {
    let settings = page.effective_settings().into_owned();
    let driver = page.providers_ui.add_driver.unwrap_or(config::DRIVERS[0].kind);
    let key = (String::new(), ADD_NAME.to_owned());
    let name = page
        .providers_ui
        .inputs
        .get(&key)
        .map(|state| state.read(cx).value().trim().to_owned())
        .unwrap_or_default();
    let id = config::derive_instance_id(driver, &name);
    if id.is_empty() {
        page.providers_ui.add_error = Some("Enter a name for the instance.".to_owned());
        cx.notify();
        return;
    }
    match settings.instance_add_patch(&id, driver, &name) {
        Ok(patch) => {
            page.providers_ui.add_open = false;
            page.providers_ui.add_error = None;
            page.providers_ui.configuring = Some(id);
            if let Some(state) = page.providers_ui.inputs.get(&key) {
                state.update(cx, |state, cx| state.set_value("", window, cx));
            }
            page.save(patch, cx);
        }
        Err(error) => {
            page.providers_ui.add_error = Some(error);
            cx.notify();
        }
    }
}

fn add_custom_model(
    page: &mut SettingsPage,
    id: &str,
    window: &mut Window,
    cx: &mut Context<SettingsPage>,
) {
    let key = (id.to_owned(), MODEL.to_owned());
    let Some(state) = page.providers_ui.inputs.get(&key).cloned() else { return };
    let slug = state.read(cx).value().to_string();
    if slug.trim().is_empty() {
        return;
    }
    edit_instance(page, id, cx, |envelope| {
        config::add_custom_model(envelope, &slug);
    });
    state.update(cx, |state, cx| state.set_value("", window, cx));
}

/// Saves a text input's value when it loses focus or Enter is pressed (never
/// per keystroke).
fn commit_input(
    page: &mut SettingsPage,
    key: &(String, String),
    event: &InputEvent,
    window: &mut Window,
    cx: &mut Context<SettingsPage>,
) {
    let enter = matches!(event, InputEvent::PressEnter { .. });
    if !enter && !matches!(event, InputEvent::Blur) {
        return;
    }
    let (id, field_key) = key;
    if field_key == MODEL {
        if enter {
            add_custom_model(page, id, window, cx);
        }
        return;
    }
    if field_key == ADD_NAME {
        if enter {
            add_instance(page, window, cx);
        }
        return;
    }
    let Some(value) = page.providers_ui.inputs.get(key).map(|state| state.read(cx).value().to_string())
    else {
        return;
    };
    if field_key == NAME {
        edit_instance(page, id, cx, |envelope| config::set_display_name(envelope, &value));
        return;
    }
    let driver = page.effective_settings().provider_instance(id).and_then(|i| config::driver(&i.driver));
    let Some(field) = driver.and_then(|driver| driver.fields.iter().find(|f| f.key == field_key)) else {
        return;
    };
    edit_instance(page, id, cx, |envelope| config::set_field(envelope, field, &FieldValue::Text(value)));
}

/// Creates the text inputs the open forms need and refreshes the ones that are
/// not being edited from the settings. Runs before each render of this page
/// (inputs need a `Window`, which `render` functions do not get).
pub fn sync(page: &mut SettingsPage, window: &mut Window, cx: &mut Context<SettingsPage>) {
    // (key, placeholder, masked, value to show; `None` keeps what is typed)
    let mut wanted: Vec<((String, String), String, bool, Option<String>)> = Vec::new();
    let settings = page.effective_settings().into_owned();
    if let Some(instance) = page
        .providers_ui
        .configuring
        .as_deref()
        .and_then(|id| settings.provider_instance(id))
    {
        let id = instance.id.clone();
        let driver = config::driver(&instance.driver);
        let placeholder = driver.map_or("Instance label", |driver| driver.label);
        wanted.push((
            (id.clone(), NAME.to_owned()),
            placeholder.to_owned(),
            false,
            Some(instance.envelope.get("displayName").and_then(Value::as_str).unwrap_or_default().to_owned()),
        ));
        for field in driver.map_or(&[][..], |driver| driver.fields) {
            if matches!(field.control, Control::Text | Control::Password) {
                wanted.push((
                    (id.clone(), field.key.to_owned()),
                    field.placeholder.to_owned(),
                    field.control == Control::Password,
                    Some(instance.text_value(field.key)),
                ));
            }
        }
        if driver.is_some_and(|driver| driver.has_custom_models()) {
            wanted.push(((id, MODEL.to_owned()), "model-slug".to_owned(), false, None));
        }
    }
    if page.providers_ui.add_open {
        wanted.push(((String::new(), ADD_NAME.to_owned()), "e.g. Work".to_owned(), false, None));
    }
    for (key, placeholder, masked, value) in wanted {
        let state = match page.providers_ui.inputs.get(&key) {
            Some(state) => state.clone(),
            None => {
                let state = cx.new(|cx| {
                    InputState::new(window, cx).placeholder(placeholder).masked(masked)
                });
                let subscription_key = key.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event: &InputEvent, window, cx| {
                        commit_input(this, &subscription_key, event, window, cx)
                    },
                );
                page.providers_ui.subscriptions.push(subscription);
                page.providers_ui.inputs.insert(key, state.clone());
                state
            }
        };
        if let Some(value) = value
            && state.read(cx).value() != value
            && !state.read(cx).focus_handle(cx).is_focused(window)
        {
            state.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }
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
