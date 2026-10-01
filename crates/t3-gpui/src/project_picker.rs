//! "New thread" project picker: a centered modal over a searchable list of
//! projects, after the T3 desktop app's `Cmd+K`-style picker.
//!
//! Keyboard-first by design (the mouse is optional): typing filters, Up/Down
//! moves the highlight, Enter confirms it, Escape closes, and Ctrl+1..9 jump
//! straight to one of the first nine rows. That last one needs real
//! `Action`/`KeyBinding`s (see [`init`]) rather than a raw key-down
//! listener, so the bindings compose correctly with the search `Input`'s own
//! text handling — GPUI dispatches an `Action` by walking the focused node's
//! key-context ancestors, so the bindings still fire while the `Input` (a
//! descendant of this view's `key_context`) holds literal keyboard focus.
//!
//! Kept out of `sidebar.rs`: the picker overlays the whole window, not just
//! the sidebar column, and `T3App` mounts it as a sibling of the sidebar/main
//! split (see `app.rs`).

use gpui_kit::assets::IconName;
use gpui_kit::base::actions::{Cancel, Confirm, SelectDown, SelectUp};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ProjectShell;

use crate::ui::{self, icon};

const CONTEXT: &str = "ProjectPicker";
/// Rows beyond this many still render but have no Ctrl+N shortcut.
const SLOT_COUNT: usize = 9;

gpui_kit::actions!(
    project_picker,
    [
        SelectSlot1,
        SelectSlot2,
        SelectSlot3,
        SelectSlot4,
        SelectSlot5,
        SelectSlot6,
        SelectSlot7,
        SelectSlot8,
        SelectSlot9
    ]
);

/// Binds the picker's keys once at startup, same as `gpui_kit::init` and the
/// component library's own `List`/`Command` do for their contexts.
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

pub enum ProjectPickerEvent {
    Select(ProjectShell),
    Cancel,
}

pub struct ProjectPicker {
    focus_handle: FocusHandle,
    search: Entity<InputState>,
    open: bool,
    all: Vec<ProjectShell>,
    /// Projects matching the query, cached so navigation and render don't
    /// refilter `all` every frame.
    filtered: Vec<ProjectShell>,
    selected: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectPickerEvent> for ProjectPicker {}

impl ProjectPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
        let subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.recompute(cx);
            }
        })];

        Self {
            focus_handle: cx.focus_handle(),
            search,
            open: false,
            all: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the picker over `projects`, sorted by title so the Ctrl+N slots
    /// are predictable.
    pub fn open(
        &mut self,
        mut projects: Vec<ProjectShell>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        projects.sort_by(|a, b| a.title.cmp(&b.title));
        self.all = projects;
        self.open = true;
        self.search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.recompute(cx);
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_lowercase();
        self.filtered = self
            .all
            .iter()
            .filter(|p| query.is_empty() || p.title.to_lowercase().contains(&query))
            .cloned()
            .collect();
        self.selected = 0;
        cx.notify();
    }

    fn confirm_selected(&mut self, cx: &mut Context<Self>) {
        self.select_slot(self.selected, cx);
    }

    fn select_slot(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(project) = self.filtered.get(index).cloned() else { return };
        self.open = false;
        cx.emit(ProjectPickerEvent::Select(project));
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.filtered.is_empty() {
            return;
        }
        let len = self.filtered.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(len);
        self.selected = next as usize;
        cx.notify();
    }

    fn on_cancel(&mut self, _: &Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        cx.emit(ProjectPickerEvent::Cancel);
        cx.notify();
    }

    fn on_confirm(&mut self, _: &Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_selected(cx);
    }

    fn on_select_up(&mut self, _: &SelectUp, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn on_select_down(&mut self, _: &SelectDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }
}

/// Defines `on_slot_N` handlers for `SelectSlotN`, one per Ctrl+N binding.
macro_rules! slot_handler {
    ($fn_name:ident, $action:ty, $index:expr) => {
        impl ProjectPicker {
            fn $fn_name(&mut self, _: &$action, _window: &mut Window, cx: &mut Context<Self>) {
                self.select_slot($index, cx);
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

impl Render for ProjectPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let rows: Vec<_> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(ix, project)| self.render_row(ix, project, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        let theme = cx.theme();

        div()
            .id("project-picker-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(140.))
            .bg(ui::hex(0x000000).opacity(0.5))
            .on_click(cx.listener(|this, _, _, cx| {
                this.open = false;
                cx.emit(ProjectPickerEvent::Cancel);
                cx.notify();
            }))
            .child(
                v_flex()
                    .id("project-picker-panel")
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
                    // Swallow clicks on the panel so they don't bubble to the backdrop's close handler.
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(480.))
                    .max_h(px(420.))
                    .rounded_xl()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                Button::new("project-picker-back")
                                    .ghost()
                                    .small()
                                    .icon(icon(IconName::ArrowLeft))
                                    .tooltip("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open = false;
                                        cx.emit(ProjectPickerEvent::Cancel);
                                        cx.notify();
                                    })),
                            )
                            .child(Input::new(&self.search).appearance(false).cleanable(false)),
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_2()
                            .text_xs()
                            .font_semibold()
                            .text_color(theme.muted_foreground)
                            .child("Projects"),
                    )
                    .child(
                        div()
                            .id("project-picker-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .px_2()
                            .py_1()
                            .child(v_flex().gap_0p5().children(rows).when(empty, |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child("No matching projects"),
                                )
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(theme.border)
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("↑ ↓ Navigate")
                            .child("Enter Select")
                            .child("Esc Close"),
                    ),
            )
            .into_any_element()
    }
}

impl ProjectPicker {
    fn render_row(&self, ix: usize, project: &ProjectShell, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let selected = ix == self.selected;
        let project_id = project.id.clone();

        h_flex()
            .id(("project-picker-row", ix))
            .gap_2()
            .px_2()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |row| row.bg(theme.sidebar_accent))
            .when(!selected, |row| row.hover(|style| style.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                let Some(index) = this.filtered.iter().position(|p| p.id == project_id) else {
                    return;
                };
                this.select_slot(index, cx);
            }))
            .child(ui::project_tag(&project.id, &project.title))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().truncate().child(project.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(format!("Local · {}", project.workspace_root)),
                    ),
            )
            .when(ix < SLOT_COUNT, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("Ctrl+{}", ix + 1)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Shadows the `gpui::test` macro brought in by `use super::*`.
    use core::prelude::v1::test;

    fn project(id: &str, title: &str) -> ProjectShell {
        serde_json::from_value(serde_json::json!({
            "id": id, "title": title, "workspaceRoot": format!("/repos/{id}"),
        }))
        .unwrap()
    }

    /// The pure filter predicate `recompute` applies, exercised directly so
    /// the matching rule has a test without spinning up a `Context`.
    fn matches(project: &ProjectShell, query: &str) -> bool {
        query.is_empty() || project.title.to_lowercase().contains(&query.to_lowercase())
    }

    #[test]
    fn filters_by_title_case_insensitively() {
        let projects = [project("p1", "T3 Code"), project("p2", "construction-erp")];
        let matched: Vec<_> =
            projects.iter().filter(|p| matches(p, "code")).map(|p| p.id.as_str()).collect();
        assert_eq!(matched, ["p1"]);
    }

    #[test]
    fn empty_query_matches_everything() {
        let projects = [project("p1", "T3 Code"), project("p2", "construction-erp")];
        assert!(projects.iter().all(|p| matches(p, "")));
    }
}
