//! "Add Action": a project script editor, saved through `project.meta.update`.
//! Mirrors the web app's `projectScriptEditor.tsx` fields, minus the
//! keybinding, which lives in the server's keybindings file.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ProjectScript;

use crate::info_panel::script_icon;
use crate::ui::{self, icon};

/// `keybindings.ts`'s `MAX_SCRIPT_ID_LENGTH`.
const MAX_SCRIPT_ID_LENGTH: usize = 24;
const ICONS: [(&str, &str); 6] = [
    ("play", "Run"),
    ("test", "Test"),
    ("lint", "Lint"),
    ("configure", "Configure"),
    ("build", "Build"),
    ("debug", "Debug"),
];

#[derive(Debug, Clone)]
pub enum ScriptDialogEvent {
    /// The project's full action list, with the new one appended.
    Save { project_id: String, scripts: Vec<ProjectScript> },
}

pub struct ScriptDialog {
    open: bool,
    project_id: String,
    existing: Vec<ProjectScript>,
    name: Entity<InputState>,
    command: Entity<InputState>,
    preview_url: Entity<InputState>,
    icon: &'static str,
    run_on_worktree_create: bool,
    wait_for_finish: bool,
    auto_open_preview: bool,
    error: Option<&'static str>,
}

impl EventEmitter<ScriptDialogEvent> for ScriptDialog {}

impl ScriptDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            project_id: String::new(),
            existing: Vec::new(),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Test")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder("bun test")),
            preview_url: cx
                .new(|cx| InputState::new(window, cx).placeholder("http://localhost:5173")),
            icon: "play",
            run_on_worktree_create: false,
            wait_for_finish: false,
            auto_open_preview: false,
            error: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(
        &mut self,
        project_id: String,
        existing: Vec<ProjectScript>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = true;
        self.project_id = project_id;
        self.existing = existing;
        self.icon = "play";
        self.run_on_worktree_create = false;
        self.wait_for_finish = false;
        self.auto_open_preview = false;
        self.error = None;
        for input in [&self.name, &self.command, &self.preview_url] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.name.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        let command = self.command.read(cx).value().trim().to_owned();
        let preview_url = self.preview_url.read(cx).value().trim().to_owned();
        if name.is_empty() || command.is_empty() {
            self.error = Some("Name and command are required.");
            return cx.notify();
        }
        let id = next_script_id(&name, self.existing.iter().map(|script| script.id.as_str()));
        let has_preview = !preview_url.is_empty();
        let mut scripts = self.existing.clone();
        scripts.push(ProjectScript {
            id,
            name,
            command,
            icon: self.icon.to_owned(),
            run_on_worktree_create: self.run_on_worktree_create,
            r#async: self.run_on_worktree_create.then_some(!self.wait_for_finish),
            preview_url: has_preview.then_some(preview_url),
            auto_open_preview: (has_preview && self.auto_open_preview).then_some(true),
        });
        cx.emit(ScriptDialogEvent::Save { project_id: self.project_id.clone(), scripts });
        self.close(cx);
    }
}

impl Render for ScriptDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        let icon_picker = Button::new("script-icon")
            .outline()
            .small()
            .icon(icon(script_icon(self.icon)))
            .tooltip("Icon")
            .dropdown_menu(move |mut menu, _, _| {
                for (id, label) in ICONS {
                    let view = view.clone();
                    menu = menu.item(PopupMenuItem::new(label).icon(icon(script_icon(id))).on_click(
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.icon = id;
                                cx.notify();
                            });
                        },
                    ));
                }
                menu
            });
        let has_preview = !self.preview_url.read(cx).value().trim().is_empty();

        div()
            .id("script-dialog-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(ui::hex(0x000000).opacity(0.5))
            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
            .child(
                v_flex()
                    .id("script-dialog")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(500.))
                    .gap_4()
                    .p_6()
                    .rounded_xl()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .items_start()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(div().text_lg().font_semibold().child("Add Action"))
                                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                                        "Actions are project-scoped commands you can run from the info panel.",
                                    )),
                            )
                            .child(
                                Button::new("script-dialog-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(icon(IconName::X))
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            ),
                    )
                    .child(field(
                        "Name",
                        h_flex().gap_2().child(icon_picker).child(Input::new(&self.name).flex_1()),
                        None,
                        cx,
                    ))
                    .child(field("Command", Input::new(&self.command), None, cx))
                    .child(field(
                        "Preview URL (optional)",
                        Input::new(&self.preview_url),
                        Some("Shown with the action; the native app has no in-app preview yet."),
                        cx,
                    ))
                    .child(toggle(
                        "script-worktree",
                        "Run automatically on worktree creation",
                        self.run_on_worktree_create,
                        true,
                        cx.listener(|this, checked: &bool, _, cx| {
                            this.run_on_worktree_create = *checked;
                            cx.notify();
                        }),
                        cx,
                    ))
                    .child(toggle(
                        "script-wait",
                        "Wait for it to finish before the agent starts",
                        self.wait_for_finish,
                        self.run_on_worktree_create,
                        cx.listener(|this, checked: &bool, _, cx| {
                            this.wait_for_finish = *checked;
                            cx.notify();
                        }),
                        cx,
                    ))
                    .child(toggle(
                        "script-preview",
                        "Open preview automatically when this action runs",
                        self.auto_open_preview,
                        has_preview,
                        cx.listener(|this, checked: &bool, _, cx| {
                            this.auto_open_preview = *checked;
                            cx.notify();
                        }),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .children(self.error.map(|error| {
                                div().flex_1().text_sm().text_color(theme.danger).child(error)
                            }))
                            .child(
                                Button::new("script-cancel")
                                    .outline()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                Button::new("script-save")
                                    .primary()
                                    .label("Save action")
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn field(
    label: &'static str,
    control: impl IntoElement,
    hint: Option<&'static str>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(div().text_sm().font_semibold().child(label))
        .child(control)
        .children(hint.map(|hint| div().text_xs().text_color(cx.theme().muted_foreground).child(hint)))
}

fn toggle(
    id: &'static str,
    label: &'static str,
    checked: bool,
    enabled: bool,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .px_3()
        .py_2p5()
        .rounded_lg()
        .bg(cx.theme().secondary)
        .text_sm()
        .when(!enabled, |row| row.opacity(0.5))
        .child(div().flex_1().child(label))
        .child(Switch::new(id).checked(checked && enabled).disabled(!enabled).on_change(on_change))
}

/// Same IDs as the web app's `nextProjectScriptId`: a slug of the name,
/// suffixed `-2`, `-3`… until unused, at most `MAX_SCRIPT_ID_LENGTH` long.
fn next_script_id<'a>(name: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let taken: Vec<&str> = existing.collect();
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut base = slug.trim_matches('-').to_owned();
    base.truncate(MAX_SCRIPT_ID_LENGTH);
    let base = match base.trim_end_matches('-') {
        "" => "script".to_owned(),
        trimmed => trimmed.to_owned(),
    };
    if !taken.contains(&base.as_str()) {
        return base;
    }
    (2..10_000)
        .map(|suffix| {
            let suffix = format!("-{suffix}");
            let keep = MAX_SCRIPT_ID_LENGTH.saturating_sub(suffix.len()).min(base.len());
            format!("{}{suffix}", &base[..keep])
        })
        .find(|candidate| !taken.contains(&candidate.as_str()))
        .unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn script_ids_are_unique_slugs() {
        assert_eq!(next_script_id("Run Tests!", [].into_iter()), "run-tests");
        assert_eq!(next_script_id("  ", [].into_iter()), "script");
        assert_eq!(next_script_id("Test", ["test", "test-2"].into_iter()), "test-3");
        let long = next_script_id("a very long action name that keeps going", [].into_iter());
        assert!(long.len() <= MAX_SCRIPT_ID_LENGTH && !long.ends_with('-'));
    }
}
