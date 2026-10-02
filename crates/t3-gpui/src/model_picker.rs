//! Composer model picker: a popover with a rail of provider instances, a
//! favorites tab, search across every provider, and Ctrl+1..9 quick select.
//!
//! A thread that has started is bound to its provider instance. Models from
//! other instances (or other models, when the provider needs a new thread to
//! change model) stay listed but emit `ContinueInNewThread` instead of
//! switching, so the conversation can carry on in a fresh thread.

use gpui_kit::assets::IconName;
use gpui_kit::base::actions::{Cancel, Confirm, SelectDown, SelectUp};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use t3_client::ServerProvider;

use crate::prefs::Prefs;
use crate::project_picker::{
    SelectSlot1, SelectSlot2, SelectSlot3, SelectSlot4, SelectSlot5, SelectSlot6, SelectSlot7,
    SelectSlot8, SelectSlot9,
};
use crate::ui::{self, icon};

const CONTEXT: &str = "ModelPicker";
const SLOT_COUNT: usize = 9;

pub fn init(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("enter", Confirm { secondary: false }, context),
        KeyBinding::new("up", SelectUp, context),
        KeyBinding::new("down", SelectDown, context),
        KeyBinding::new("ctrl-1", SelectSlot1, context),
        KeyBinding::new("ctrl-2", SelectSlot2, context),
        KeyBinding::new("ctrl-3", SelectSlot3, context),
        KeyBinding::new("ctrl-4", SelectSlot4, context),
        KeyBinding::new("ctrl-5", SelectSlot5, context),
        KeyBinding::new("ctrl-6", SelectSlot6, context),
        KeyBinding::new("ctrl-7", SelectSlot7, context),
        KeyBinding::new("ctrl-8", SelectSlot8, context),
        KeyBinding::new("ctrl-9", SelectSlot9, context),
    ]);
}

pub enum ModelPickerEvent {
    /// Switch the current thread (or draft) to this `modelSelection`.
    Select(Value),
    /// The thread is bound to another provider: continue in a new thread.
    ContinueInNewThread(Value),
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerTab {
    Favorites,
    Provider(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRow {
    pub instance_id: String,
    pub provider: String,
    pub driver: String,
    pub model: String,
    pub label: String,
    pub selected: bool,
    pub favorite: bool,
    /// Picking this model starts a new thread instead of switching.
    pub needs_new_thread: bool,
}

impl ModelRow {
    pub fn selection(&self) -> Value {
        json!({ "instanceId": self.instance_id, "model": self.model })
    }
}

/// Providers that can run a turn right now.
pub fn usable(provider: &ServerProvider) -> bool {
    provider.enabled && provider.installed && provider.availability.as_deref() != Some("unavailable")
}

pub fn provider_name(provider: &ServerProvider) -> String {
    provider
        .display_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| ui::provider_label(Some(&provider.driver)))
}

/// The rows for `tab`, or across every provider while searching.
pub fn build_rows(
    providers: &[ServerProvider],
    selection: Option<&Value>,
    started: bool,
    tab: &PickerTab,
    query: &str,
    favorites: &[String],
) -> Vec<ModelRow> {
    let query = query.trim().to_lowercase();
    let current_instance = selection.and_then(|s| s["instanceId"].as_str());
    let current_model = selection.and_then(|s| s["model"].as_str());
    let mut rows: Vec<ModelRow> = providers
        .iter()
        .filter(|provider| usable(provider))
        .filter(|provider| {
            !query.is_empty()
                || matches!(tab, PickerTab::Favorites)
                || matches!(tab, PickerTab::Provider(id) if *id == provider.instance_id)
        })
        .flat_map(|provider| {
            let name = provider_name(provider);
            provider.models.iter().map(move |model| {
                let same_instance = current_instance == Some(provider.instance_id.as_str());
                let selected = same_instance && current_model == Some(model.id.as_str());
                ModelRow {
                    instance_id: provider.instance_id.clone(),
                    provider: name.clone(),
                    driver: provider.driver.clone(),
                    model: model.id.clone(),
                    label: model.label.clone(),
                    selected,
                    favorite: favorites
                        .contains(&crate::prefs::favorite_key(&provider.instance_id, &model.id)),
                    needs_new_thread: started
                        && !selected
                        && (!same_instance || provider.requires_new_thread_for_model_change),
                }
            })
        })
        .filter(|row| {
            if !query.is_empty() {
                return row.label.to_lowercase().contains(&query)
                    || row.model.to_lowercase().contains(&query)
                    || row.provider.to_lowercase().contains(&query);
            }
            !matches!(tab, PickerTab::Favorites) || row.favorite
        })
        .collect();
    if matches!(tab, PickerTab::Favorites) && query.is_empty() {
        let position = |row: &ModelRow| {
            favorites
                .iter()
                .position(|key| *key == crate::prefs::favorite_key(&row.instance_id, &row.model))
        };
        rows.sort_by_key(position);
    }
    rows
}

pub struct ModelPicker {
    focus_handle: FocusHandle,
    search: Entity<InputState>,
    open: bool,
    providers: Vec<ServerProvider>,
    selection: Option<Value>,
    started: bool,
    tab: PickerTab,
    rows: Vec<ModelRow>,
    highlighted: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ModelPickerEvent> for ModelPicker {}

impl ModelPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models…"));
        let subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.highlighted = 0;
                this.recompute(cx);
            }
        })];
        Self {
            focus_handle: cx.focus_handle(),
            search,
            open: false,
            providers: Vec::new(),
            selection: None,
            started: false,
            tab: PickerTab::Favorites,
            rows: Vec::new(),
            highlighted: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The models on offer, the current selection, and whether the thread
    /// has started (which binds it to its provider instance).
    pub fn set_context(
        &mut self,
        providers: Vec<ServerProvider>,
        selection: Option<Value>,
        started: bool,
        cx: &mut Context<Self>,
    ) {
        self.providers = providers;
        self.selection = selection;
        self.started = started;
        self.recompute(cx);
    }

    /// Opens on the current model's provider, with an empty search.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        let current = self.selection.as_ref().and_then(|s| s["instanceId"].as_str());
        self.tab = match current {
            Some(id) if self.providers.iter().any(|p| p.instance_id == id && usable(p)) => {
                PickerTab::Provider(id.to_owned())
            }
            _ => self
                .providers
                .iter()
                .find(|p| usable(p))
                .map_or(PickerTab::Favorites, |p| PickerTab::Provider(p.instance_id.clone())),
        };
        self.search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.recompute(cx);
        self.highlighted = self.rows.iter().position(|row| row.selected).unwrap_or(0);
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        self.rows = build_rows(
            &self.providers,
            self.selection.as_ref(),
            self.started,
            &self.tab,
            &query,
            &Prefs::global(cx).favorite_models,
        );
        self.highlighted = self.highlighted.min(self.rows.len().saturating_sub(1));
        cx.notify();
    }

    fn set_tab(&mut self, tab: PickerTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.highlighted = 0;
        self.search.update(cx, |state, cx| state.set_value("", window, cx));
        self.recompute(cx);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.open = false;
        cx.emit(if row.needs_new_thread {
            ModelPickerEvent::ContinueInNewThread(row.selection())
        } else {
            ModelPickerEvent::Select(row.selection())
        });
        cx.notify();
    }

    fn toggle_favorite(&mut self, instance_id: &str, model: &str, cx: &mut Context<Self>) {
        Prefs::toggle_favorite(cx, instance_id, model);
        self.recompute(cx);
    }

    fn on_cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        cx.emit(ModelPickerEvent::Dismiss);
        cx.notify();
    }

    fn on_confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(self.highlighted, cx);
    }

    fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_highlight(-1, cx);
    }

    fn on_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_highlight(1, cx);
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
        cx.notify();
    }
}

macro_rules! slot_handler {
    ($fn_name:ident, $action:ty, $index:expr) => {
        impl ModelPicker {
            fn $fn_name(&mut self, _: &$action, _: &mut Window, cx: &mut Context<Self>) {
                self.choose($index, cx);
            }
        }
    };
}

slot_handler!(on_slot_1, SelectSlot1, 0);
slot_handler!(on_slot_2, SelectSlot2, 1);
slot_handler!(on_slot_3, SelectSlot3, 2);
slot_handler!(on_slot_4, SelectSlot4, 3);
slot_handler!(on_slot_5, SelectSlot5, 4);
slot_handler!(on_slot_6, SelectSlot6, 5);
slot_handler!(on_slot_7, SelectSlot7, 6);
slot_handler!(on_slot_8, SelectSlot8, 7);
slot_handler!(on_slot_9, SelectSlot9, 8);

/// A provider instance's SVG logo with a colored initials badge,
/// so two instances of one driver stay distinguishable.
pub fn provider_mark(instance_id: &str, driver: &str, name: &str, size: Pixels) -> Div {
    let color = ui::project_color(instance_id);
    // Readable even on the smallest marks.
    let badge_text = if size * 0.45 < px(7.) { px(7.) } else { size * 0.45 };
    div()
        .relative()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(size)
        .child(crate::provider_logo::logo(driver, size * 0.8, color))
        .child(
            div()
                .absolute()
                .right(-size * 0.3)
                .bottom(-size * 0.2)
                .px(px(2.))
                .rounded(px(3.))
                .bg(color)
                .text_color(ui::hex(0xffffff))
                .text_size(badge_text)
                .line_height(badge_text * 1.15)
                .font_bold()
                .child(ui::initials(name)),
        )
}

impl Render for ModelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| self.render_row(ix, row, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();

        let theme = cx.theme();
        let searching = !self.search.read(cx).value().trim().is_empty();
        let current_instance = self.selection.as_ref().and_then(|s| s["instanceId"].as_str());
        let current_provider = self
            .providers
            .iter()
            .find(|p| Some(p.instance_id.as_str()) == current_instance)
            .map(provider_name);

        let rail_item = |id: SharedString, active: bool, tooltip: String, content: AnyElement| {
            h_flex()
                .id(id)
                .w_full()
                .h(px(40.))
                .items_center()
                .justify_center()
                .relative()
                .cursor_pointer()
                .hover(|style| style.bg(theme.list_hover))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .when(active, |item| {
                    item.child(
                        div()
                            .absolute()
                            .right_0()
                            .top(px(8.))
                            .h(px(24.))
                            .w(px(2.))
                            .rounded_full()
                            .bg(theme.primary),
                    )
                })
                .child(content)
        };

        let mut rail = v_flex().w(px(52.)).flex_shrink_0().py_1().border_r_1().border_color(theme.border);
        rail = rail.child(
            rail_item(
                "model-tab-favorites".into(),
                !searching && self.tab == PickerTab::Favorites,
                "Favorites".into(),
                icon(IconName::Star)
                    .size(px(18.))
                    .text_color(if self.tab == PickerTab::Favorites {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    })
                    .into_any_element(),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.set_tab(PickerTab::Favorites, window, cx)
            })),
        );
        for provider in self.providers.iter().filter(|p| usable(p)) {
            let instance = provider.instance_id.clone();
            let name = provider_name(provider);
            let active =
                !searching && matches!(&self.tab, PickerTab::Provider(id) if *id == instance);
            rail = rail.child(
                rail_item(
                    format!("model-tab-{instance}").into(),
                    active,
                    format!("{name} · {} models", provider.models.len()),
                    provider_mark(&instance, &provider.driver, &name, px(22.))
                        .when(!active, |mark| mark.opacity(0.75))
                        .into_any_element(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_tab(PickerTab::Provider(instance.clone()), window, cx)
                })),
            );
        }

        let locked_banner = (self.started
            && !searching
            && matches!(&self.tab, PickerTab::Provider(id) if Some(id.as_str()) != current_instance))
        .then(|| {
            h_flex()
                .mx_2()
                .mt_2()
                .gap_2()
                .p_2()
                .rounded_md()
                .bg(theme.primary.opacity(0.08))
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(icon(IconName::Lock).xsmall().flex_shrink_0().text_color(theme.primary))
                .child(div().min_w_0().child(format!(
                    "This thread runs on {}. Picking a model here continues the work in a new thread.",
                    current_provider.clone().unwrap_or_else(|| "another provider".into())
                )))
        });

        h_flex()
            .id("model-picker-panel")
            .test_support()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_slot_1))
            .on_action(cx.listener(Self::on_slot_2))
            .on_action(cx.listener(Self::on_slot_3))
            .on_action(cx.listener(Self::on_slot_4))
            .on_action(cx.listener(Self::on_slot_5))
            .on_action(cx.listener(Self::on_slot_6))
            .on_action(cx.listener(Self::on_slot_7))
            .on_action(cx.listener(Self::on_slot_8))
            .on_action(cx.listener(Self::on_slot_9))
            .w(px(400.))
            .h(px(340.))
            .items_stretch()
            .child(rail)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .px_2()
                            .py_1p5()
                            .border_b_1()
                            .border_color(theme.primary.opacity(0.5))
                            .child(
                                Input::new(&self.search).small().appearance(false).prefix(
                                    icon(IconName::Search)
                                        .xsmall()
                                        .text_color(theme.muted_foreground),
                                ),
                            ),
                    )
                    .children(locked_banner)
                    .child(
                        div()
                            .id("model-picker-list")
                            .flex_1()
                            .min_h_0()
                            .p_1p5()
                            .overflow_y_scrollbar()
                            .child(v_flex().gap_0p5().children(rows).when(empty, |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(if self.providers.is_empty() {
                                            "Models unavailable. Reconnect to retry."
                                        } else if self.tab == PickerTab::Favorites && !searching {
                                            "Star a model to keep it here."
                                        } else {
                                            "No matching models"
                                        }),
                                )
                            })),
                    ),
            )
    }
}

impl ModelPicker {
    fn render_row(&self, ix: usize, row: &ModelRow, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let highlighted = ix == self.highlighted;
        let (instance, model) = (row.instance_id.clone(), row.model.clone());
        let star = div()
            .id(SharedString::from(format!("model-star-{}-{}", row.instance_id, row.model)))
            .flex_shrink_0()
            .p_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(theme.secondary_hover))
            .tooltip(|window, cx| Tooltip::new("Favorite").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_favorite(&instance, &model, cx);
            }))
            .child(icon(IconName::Star).xsmall().text_color(if row.favorite {
                theme.primary
            } else {
                theme.muted_foreground.opacity(0.6)
            }));

        h_flex()
            .id(("model-row", ix))
            .test_support()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .when(highlighted, |item| item.bg(theme.secondary_hover))
            .when(!highlighted, |item| item.hover(|style| style.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(ix, cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_sm()
                            .font_semibold()
                            .when(row.needs_new_thread, |label| {
                                label.text_color(theme.foreground.opacity(0.7))
                            })
                            .child(div().min_w_0().truncate().child(row.label.clone()))
                            .when(row.selected, |label| {
                                label.child(icon(IconName::Check).xsmall().text_color(theme.primary))
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(provider_mark(&row.instance_id, &row.driver, &row.provider, px(11.)))
                            .child(div().min_w_0().truncate().child(row.provider.clone())),
                    ),
            )
            .when(row.needs_new_thread, |item| {
                item.child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .px_1p5()
                        .py_0p5()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("New thread")
                        .child(icon(IconName::ArrowUpRight).xsmall()),
                )
            })
            .when(!row.needs_new_thread && ix < SLOT_COUNT, |item| {
                item.child(
                    div()
                        .flex_shrink_0()
                        .px_1p5()
                        .py_0p5()
                        .rounded_md()
                        .bg(theme.secondary)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("Ctrl+{}", ix + 1)),
                )
            })
            .child(star)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn providers() -> Vec<ServerProvider> {
        serde_json::from_value(json!([
            {"instanceId":"claude-a","driver":"claudeAgent","displayName":"Ai Guru","enabled":true,"installed":true,
             "models":[{"slug":"opus","name":"Claude Opus"},{"slug":"sonnet","name":"Claude Sonnet"}]},
            {"instanceId":"codex-b","driver":"codex","displayName":"Work","enabled":true,"installed":true,
             "models":[{"slug":"gpt","name":"GPT"}]},
            {"instanceId":"off","driver":"codex","enabled":false,"installed":true,"models":[{"slug":"x","name":"X"}]}
        ]))
        .unwrap()
    }

    #[test]
    fn a_started_thread_keeps_its_provider_and_offers_others_as_new_threads() {
        let selection = json!({ "instanceId": "claude-a", "model": "opus" });
        let tab = PickerTab::Provider("codex-b".into());
        let rows = build_rows(&providers(), Some(&selection), true, &tab, "", &[]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].needs_new_thread);

        let tab = PickerTab::Provider("claude-a".into());
        let rows = build_rows(&providers(), Some(&selection), true, &tab, "", &[]);
        assert!(rows.iter().all(|row| !row.needs_new_thread));
        assert!(rows[0].selected);

        let rows = build_rows(&providers(), Some(&selection), false, &PickerTab::Provider("codex-b".into()), "", &[]);
        assert!(!rows[0].needs_new_thread, "an unstarted thread can switch freely");
    }

    #[test]
    fn search_spans_usable_providers_and_favorites_keep_star_order() {
        let rows = build_rows(&providers(), None, false, &PickerTab::Favorites, "gpt", &[]);
        assert_eq!(rows.iter().map(|r| r.model.as_str()).collect::<Vec<_>>(), ["gpt"]);
        let favorites = vec!["codex-b/gpt".to_owned(), "claude-a/sonnet".to_owned()];
        let rows = build_rows(&providers(), None, false, &PickerTab::Favorites, "", &favorites);
        assert_eq!(rows.iter().map(|r| r.model.as_str()).collect::<Vec<_>>(), ["gpt", "sonnet"]);
        assert!(rows.iter().all(|row| row.favorite));
    }
}
