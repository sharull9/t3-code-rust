//! Shared building blocks for settings pages: a titled card of rows
//! ([`SettingsGroup`]), one row ([`SettingRow`]), and controls bound to a
//! server setting at the page's current scope.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use super::SettingsPage;

type ResetHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One setting: title, muted description and a right-aligned control.
///
/// `id` must be unique on the page; the reset button is `"{id}-reset"`.
#[derive(IntoElement)]
pub struct SettingRow {
    id: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    control: Option<AnyElement>,
    overridable: bool,
    inherited: bool,
    modified: bool,
    hidden: bool,
    on_reset: Option<ResetHandler>,
}

impl SettingRow {
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            control: None,
            overridable: false,
            inherited: false,
            modified: false,
            hidden: false,
            on_reset: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// Marks a setting that projects can override (layers icon).
    pub fn overridable(mut self, overridable: bool) -> Self {
        self.overridable = overridable;
        self
    }

    /// The shown value comes from the environment, not from this scope.
    pub fn inherited(mut self, inherited: bool) -> Self {
        self.inherited = inherited;
        self
    }

    /// Shows the reset button; `false` hides it. Pair with [`Self::on_reset`].
    pub fn modified(mut self, modified: bool) -> Self {
        self.modified = modified;
        self
    }

    /// Not shown at the current scope. [`SettingsGroup::row`] leaves it out.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn on_reset(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reset = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reset = self.on_reset.filter(|_| self.modified).map(|handler| {
            Button::new(SharedString::from(format!("{}-reset", self.id)))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::RotateCcw))
                .tooltip("Reset to default")
                .on_click(move |event, window, cx| handler(event, window, cx))
        });
        let layers_id = SharedString::from(format!("{}-overridable", self.id));
        h_flex()
            .id(self.id.clone())
            .gap_4()
            .px_4()
            .py_3()
            .items_center()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().text_sm().font_medium().child(self.title))
                            .when(self.overridable, |title| {
                                title.child(
                                    div()
                                        .id(layers_id)
                                        .text_color(theme.muted_foreground)
                                        .tooltip(|window, cx| {
                                            Tooltip::new("Projects can override this")
                                                .build(window, cx)
                                        })
                                        .child(Icon::new(IconName::Layers).xsmall()),
                                )
                            })
                            .children(reset),
                    )
                    .children(self.description.map(|description| {
                        div().text_xs().text_color(theme.muted_foreground).child(description)
                    })),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .items_center()
                    .when(self.inherited, |control| {
                        control.child(
                            div().text_xs().text_color(theme.muted_foreground).child("Inherited"),
                        )
                    })
                    .children(self.control),
            )
    }
}

/// A heading above a rounded card whose rows are separated by dividers.
#[derive(IntoElement)]
pub struct SettingsGroup {
    title: Option<SharedString>,
    rows: Vec<AnyElement>,
}

impl SettingsGroup {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: Some(title.into()), rows: Vec::new() }
    }

    /// Adds a setting row unless it is [`SettingRow::hidden`].
    pub fn row(self, row: SettingRow) -> Self {
        if row.hidden { self } else { self.child(row) }
    }

    pub fn child(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }
}

impl RenderOnce for SettingsGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let mut card = v_flex()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(theme.secondary.opacity(0.35))
            .overflow_hidden();
        for (index, row) in self.rows.into_iter().enumerate() {
            if index > 0 {
                card = card.child(div().h_px().w_full().bg(border));
            }
            card = card.child(row);
        }
        v_flex()
            .gap_2()
            .children(self.title.map(|title| {
                div().px_1().text_sm().font_medium().text_color(theme.foreground).child(title)
            }))
            .child(card)
    }
}

/// A row for a server setting at the page's scope: marked overridable at the
/// environment, "Inherited" at a project without an override, with a reset
/// button that restores the default (environment) or clears the override
/// (project), and hidden when a project cannot override `key`.
pub fn server_row(
    page: &SettingsPage,
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    key: &'static str,
    cx: &Context<SettingsPage>,
) -> SettingRow {
    let project_scoped = t3_client::settings::is_project_scoped(key);
    let in_project = page.project_scope().is_some();
    SettingRow::new(id, title)
        .overridable(project_scoped && !in_project)
        .inherited(in_project && project_scoped && !page.server_value(key).overridden)
        .modified(page.server_ready() && page.server_modified(key))
        .hidden(in_project && !project_scoped)
        .on_reset(cx.listener(move |this, _, _, cx| this.reset_server_keys(&[key], cx)))
}

/// A switch bound to a boolean server setting at the page's scope.
pub fn server_switch(
    page: &SettingsPage,
    id: impl Into<SharedString>,
    key: &'static str,
    cx: &Context<SettingsPage>,
) -> Switch {
    let id: SharedString = id.into();
    let checked = page.server_value(key).value.as_bool().unwrap_or(false);
    Switch::new(id)
        .checked(checked)
        .disabled(!page.server_ready())
        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
            this.set_server_value(key, serde_json::Value::Bool(*checked), cx)
        }))
}

/// A dropdown choosing one of `options` (`(string value, label)`) for a server
/// setting. A `null` value shows `null_label` (what the server resolves it to);
/// any other stored value not in `options` shows as its JSON text.
pub fn server_choice(
    page: &SettingsPage,
    id: impl Into<SharedString>,
    key: &'static str,
    options: &'static [(&'static str, &'static str)],
    null_label: &'static str,
    cx: &Context<SettingsPage>,
) -> impl IntoElement {
    let current = page.server_value(key).value;
    let label = options
        .iter()
        .find(|(value, _)| current.as_str() == Some(*value))
        .map_or_else(
            || if current.is_null() { null_label.to_owned() } else { current_label(&current) },
            |(_, label)| (*label).to_owned(),
        );
    let view = cx.entity();
    choice_button(id, label, !page.server_ready(), move |mut menu| {
        for &(value, label) in options {
            let view = view.clone();
            menu = menu.item(
                PopupMenuItem::new(label).checked(current.as_str() == Some(value)).on_click(
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.set_server_value(key, Value::String(value.to_owned()), cx)
                        });
                    },
                ),
            );
        }
        menu
    })
}

fn current_label(value: &Value) -> String {
    match value {
        Value::Null => "Default".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// An outline button with a chevron that opens `build`'s menu.
pub fn choice_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    disabled: bool,
    build: impl Fn(gpui_kit::component::menu::PopupMenu) -> gpui_kit::component::menu::PopupMenu
    + 'static,
) -> impl IntoElement {
    Button::new(id.into())
        .outline()
        .small()
        .label(label)
        .icon(Icon::new(IconName::ChevronDown))
        .disabled(disabled)
        .dropdown_menu(move |menu, _, _| build(menu.scrollable(true).max_h(px(320.))))
}
