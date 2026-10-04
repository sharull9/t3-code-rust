//! The info card beside the open thread: where it runs (project folder or
//! worktree, editor, project actions) and its version control (branch,
//! commit and push, changes). Mirrors the web app's workspace popover.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{
    GitAction, GitActionOutcome, ProjectScript, WorkspaceGitStatus, WorkspaceRefs,
    WorkspaceRequest, WorkspaceResponse,
};

use crate::prefs::Prefs;
use crate::thread_view::DraftBranch;
use crate::ui::icon;

pub const INFO_PANEL_WIDTH: Pixels = px(310.);

/// What the card describes. Rebuilt by `T3App` from the open thread or draft.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoScope {
    pub project_id: Option<String>,
    pub project_title: String,
    pub project_root: Option<String>,
    pub scripts: Vec<ProjectScript>,
    /// The server thread; `None` for a draft, which has no terminal yet.
    pub thread_id: Option<String>,
    /// `Some` for a draft: whether it will start in a new worktree.
    pub draft_new_worktree: Option<bool>,
    pub worktree_path: Option<String>,
    /// Changes when a turn settles, so the git status is read again.
    pub turn_marker: Option<String>,
}

impl InfoScope {
    /// Where git and editor commands run: the thread's worktree, else the
    /// project checkout.
    pub fn cwd(&self) -> Option<&str> {
        self.worktree_path.as_deref().or(self.project_root.as_deref())
    }
}

#[derive(Debug, Clone)]
pub enum InfoPanelEvent {
    Request { request_id: u64, request: WorkspaceRequest },
    /// Show the working-tree diff in the workspace panel.
    ShowChanges,
    /// Run a project action's command in the thread terminal.
    RunScript(String),
    SetDraftWorktree(bool),
    AddScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    Status,
    Refs,
    Switch,
    Editor,
    Git,
    Scripts,
}

pub struct InfoPanel {
    scope: InfoScope,
    scope_epoch: u64,
    next_request_id: u64,
    pending: HashMap<u64, (u64, Slot)>,
    visible: bool,
    connected: bool,
    status: Option<WorkspaceGitStatus>,
    refs: Option<WorkspaceRefs>,
    available_editors: Vec<String>,
    ref_search: Entity<InputState>,
    refs_open: bool,
    git_running: Option<GitAction>,
    git_result: Option<Result<GitActionOutcome, String>>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InfoPanelEvent> for InfoPanel {}

impl InfoPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ref_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search refs…"));
        let subscriptions =
            vec![cx.subscribe(&ref_search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })];
        Self {
            scope: InfoScope::default(),
            scope_epoch: 0,
            next_request_id: 1,
            pending: HashMap::new(),
            visible: false,
            connected: false,
            status: None,
            refs: None,
            available_editors: Vec::new(),
            ref_search,
            refs_open: false,
            git_running: None,
            git_result: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_scope(&mut self, scope: InfoScope, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        let moved = self.scope.cwd() != scope.cwd();
        let settled = self.scope.turn_marker != scope.turn_marker;
        self.scope = scope;
        if moved {
            self.scope_epoch = self.scope_epoch.wrapping_add(1);
            self.pending.clear();
            self.status = None;
            self.refs = None;
            self.refs_open = false;
            self.git_running = None;
            self.git_result = None;
            self.error = None;
        }
        if moved || settled {
            self.refresh(cx);
        }
        cx.notify();
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.refresh(cx);
        }
        cx.notify();
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected == connected {
            return;
        }
        self.connected = connected;
        if connected {
            self.refresh(cx);
        } else {
            self.pending.clear();
            self.git_running = None;
        }
        cx.notify();
    }

    pub fn set_available_editors(&mut self, editors: Vec<String>, cx: &mut Context<Self>) {
        if self.available_editors != editors {
            self.available_editors = editors;
            cx.notify();
        }
    }

    /// Loads the git status, and with it [`Self::current_branch`], even
    /// while the panel is hidden: a draft's new worktree branches from it.
    pub fn load_branch(&mut self, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd().map(str::to_owned) else {
            return;
        };
        let waiting = |slot: Slot| self.pending.values().any(|(_, pending)| *pending == slot);
        let (status, refs) = (waiting(Slot::Status), waiting(Slot::Refs));
        if self.status.is_none() && !status {
            self.request(WorkspaceRequest::GitStatus { cwd: cwd.clone() }, Slot::Status, cx);
        }
        // The draft's branch picker lists these.
        if self.refs.is_none() && !refs {
            self.request(WorkspaceRequest::ListRefs { cwd }, Slot::Refs, cx);
        }
    }

    /// The checked-out branch, the base for a draft's new worktree.
    pub fn current_branch(&self) -> Option<&str> {
        self.status.as_ref().and_then(|status| status.ref_name.as_deref())
    }

    /// The local branches, for a draft's branch picker.
    pub fn local_branches(&self) -> Vec<DraftBranch> {
        let Some(refs) = &self.refs else {
            return Vec::new();
        };
        refs.refs
            .iter()
            .filter(|reference| !reference.is_remote)
            .map(|reference| DraftBranch {
                name: reference.name.clone(),
                elsewhere: !reference.current && reference.worktree_path.is_some(),
            })
            .collect()
    }

    /// Switches the checkout to `ref_name`, as the branch row does.
    pub fn checkout_ref(&mut self, ref_name: String, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd().map(str::to_owned) else {
            return;
        };
        self.request(WorkspaceRequest::SwitchRef { cwd, ref_name }, Slot::Switch, cx);
        cx.notify();
    }

    /// Writes the project's whole action list.
    pub fn save_scripts(
        &mut self,
        project_id: String,
        scripts: Vec<ProjectScript>,
        cx: &mut Context<Self>,
    ) {
        self.request(WorkspaceRequest::SetProjectScripts { project_id, scripts }, Slot::Scripts, cx);
    }

    /// Re-reads git status and refs, while the card is showing.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        if let Some(cwd) = self.scope.cwd().map(str::to_owned) {
            self.request(WorkspaceRequest::GitStatus { cwd: cwd.clone() }, Slot::Status, cx);
            self.request(WorkspaceRequest::ListRefs { cwd }, Slot::Refs, cx);
        }
    }

    pub fn apply_result(
        &mut self,
        request_id: u64,
        result: Result<WorkspaceResponse, String>,
        cx: &mut Context<Self>,
    ) {
        let Some((epoch, slot)) = self.pending.remove(&request_id) else {
            return;
        };
        // Scripts belong to the project, not the checkout, so they survive a
        // thread switch; everything else is about the old checkout.
        if epoch != self.scope_epoch && slot != Slot::Scripts {
            return;
        }
        match (slot, result) {
            (Slot::Status, Ok(WorkspaceResponse::GitStatus(status))) => self.status = Some(status),
            (Slot::Refs, Ok(WorkspaceResponse::Refs(refs))) => self.refs = Some(refs),
            (Slot::Git, Ok(WorkspaceResponse::GitAction(outcome))) => {
                self.git_running = None;
                self.git_result = Some(Ok(outcome));
                self.refresh(cx);
            }
            (Slot::Git, Err(error)) => {
                self.git_running = None;
                self.git_result = Some(Err(error));
                self.refresh(cx);
            }
            (Slot::Switch, Ok(_)) if self.visible => self.refresh(cx),
            // A draft's branch picker switched it: re-read for the picker.
            (Slot::Switch, Ok(_)) => {
                self.status = None;
                self.refs = None;
                self.load_branch(cx);
            }
            (_, Ok(_)) => {}
            (_, Err(error)) => self.error = Some(error),
        }
        cx.notify();
    }

    fn request(&mut self, request: WorkspaceRequest, slot: Slot, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.pending.insert(request_id, (self.scope_epoch, slot));
        self.error = None;
        cx.emit(InfoPanelEvent::Request { request_id, request });
    }

    fn open_in_editor(&mut self, editor: String, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd().map(str::to_owned) else {
            return;
        };
        if Prefs::global(cx).preferred_editor.as_deref() != Some(&editor) {
            Prefs::update(cx, |prefs| prefs.preferred_editor = Some(editor.clone()));
        }
        self.request(WorkspaceRequest::OpenInEditor { cwd, editor }, Slot::Editor, cx);
        cx.notify();
    }

    fn run_git(&mut self, action: GitAction, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd().map(str::to_owned) else {
            return;
        };
        if self.git_running.is_some() {
            return;
        }
        self.git_running = Some(action);
        self.git_result = None;
        let thread_id = self.scope.thread_id.clone();
        self.request(WorkspaceRequest::RunGitAction { cwd, action, thread_id }, Slot::Git, cx);
        cx.notify();
    }

    fn switch_ref(&mut self, ref_name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_refs_open(false, window, cx);
        let Some(cwd) = self.scope.cwd().map(str::to_owned) else {
            return;
        };
        self.request(WorkspaceRequest::SwitchRef { cwd, ref_name }, Slot::Switch, cx);
    }

    fn set_refs_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.refs_open = open;
        if open {
            self.ref_search.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
            if let Some(cwd) = self.scope.cwd().map(str::to_owned) {
                self.request(WorkspaceRequest::ListRefs { cwd }, Slot::Refs, cx);
            }
        }
        cx.notify();
    }

    /// The saved editor if the server still has it, else its first editor
    /// (a code editor before the file manager).
    fn preferred_editor(&self, cx: &App) -> Option<String> {
        let saved = Prefs::global(cx).preferred_editor.as_ref();
        saved
            .filter(|id| self.available_editors.contains(id))
            .or_else(|| self.available_editors.iter().find(|id| *id != "file-manager"))
            .or_else(|| self.available_editors.first())
            .cloned()
    }
}

impl Render for InfoPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = self.render_folder_row(cx).into_any_element();
        let editor = self.render_editor_row(cx).into_any_element();
        let scripts = self.render_scripts(cx);
        let version_control = self.render_version_control(cx);
        let theme = cx.theme();
        v_flex()
            .id("info-panel")
            .w(INFO_PANEL_WIDTH)
            .max_h_full()
            .py_1p5()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .overflow_y_scrollbar()
            // Keep clicks from reaching the transcript underneath.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(section_label("Workspace", cx))
            .child(folder)
            .child(editor)
            .children(scripts)
            .child(
                row("info-add-script", cx)
                    .text_color(theme.muted_foreground)
                    .child(row_icon(IconName::Plus, cx))
                    .child(row_label("Add project script"))
                    .when(self.scope.project_id.is_some() && self.connected, |row| {
                        row.on_click(cx.listener(|_, _, _, cx| cx.emit(InfoPanelEvent::AddScript)))
                    }),
            )
            .child(div().my_1p5().h_px().bg(theme.border))
            .child(section_label("Version Control", cx))
            .child(version_control)
            .children(self.error.clone().map(|error| {
                div().mx_3().mt_1().text_xs().text_color(theme.danger).child(error)
            }))
    }
}

impl InfoPanel {
    fn render_folder_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let in_worktree = self.scope.worktree_path.is_some()
            || self.scope.draft_new_worktree == Some(true);
        let draft = self.scope.draft_new_worktree;
        let view = cx.entity().downgrade();
        let menu = Button::new("info-folder-menu")
            .ghost()
            .xsmall()
            .icon(icon(IconName::ChevronDown))
            .disabled(draft.is_none())
            .tooltip(if draft.is_some() {
                "Where the new thread runs"
            } else {
                "Set when the thread starts"
            })
            .dropdown_menu(move |mut menu, _, _| {
                for (label, worktree, icon_name) in [
                    ("Current checkout", false, IconName::Folder),
                    ("New worktree", true, IconName::FolderGit2),
                ] {
                    let view = view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(icon(icon_name))
                            .checked(draft == Some(worktree))
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |_, cx| {
                                    cx.emit(InfoPanelEvent::SetDraftWorktree(worktree))
                                });
                            }),
                    );
                }
                menu
            });
        split_row("info-folder", cx)
            .child(row_icon(if in_worktree { IconName::FolderGit2 } else { IconName::Folder }, cx))
            .child(row_label(if self.scope.project_title.is_empty() {
                "No project".to_owned()
            } else {
                self.scope.project_title.clone()
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(if in_worktree { "Worktree" } else { "Project folder" }),
            )
            .child(menu)
    }

    fn render_editor_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let preferred = self.preferred_editor(cx);
        let enabled = self.connected && self.scope.cwd().is_some();
        let label = match &preferred {
            Some(id) => format!("Open in {}", editor_label(id)),
            None => "Open in editor".to_owned(),
        };
        let view = cx.entity().downgrade();
        let editors = self.available_editors.clone();
        let menu = Button::new("info-editor-menu")
            .ghost()
            .xsmall()
            .icon(icon(IconName::ChevronDown))
            .disabled(!enabled || editors.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                for id in &editors {
                    let view = view.clone();
                    let editor = id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(editor_label(id))
                            .icon(icon(editor_icon(id)))
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.open_in_editor(editor.clone(), cx)
                                });
                            }),
                    );
                }
                menu
            });
        split_row("info-editor", cx)
            .child(row_icon(preferred.as_deref().map_or(IconName::Code, editor_icon), cx))
            .child(row_label(label))
            .when_some(preferred.filter(|_| enabled), |row, editor| {
                row.on_click(
                    cx.listener(move |this, _, _, cx| this.open_in_editor(editor.clone(), cx)),
                )
            })
            .child(menu)
    }

    fn render_scripts(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let runnable = self.connected && self.scope.thread_id.is_some();
        self.scope
            .scripts
            .iter()
            .map(|script| {
                let command = script.command.clone();
                let tooltip: SharedString = if runnable {
                    script.command.clone().into()
                } else {
                    "Start the thread to run project actions".into()
                };
                row(SharedString::from(format!("info-script-{}", script.id)), cx)
                    .child(row_icon(script_icon(&script.icon), cx))
                    .child(row_label(script.name.clone()))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .when(runnable, |row| {
                        row.on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(InfoPanelEvent::RunScript(command.clone()))
                        }))
                    })
                    .when(!runnable, |row| row.opacity(0.6))
                    .into_any_element()
            })
            .collect()
    }

    fn render_version_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(status) = &self.status else {
            let theme = cx.theme();
            let text = if self.scope.cwd().is_none() {
                "Open a thread to see its branch."
            } else if self.connected {
                "Reading git status…"
            } else {
                "Reconnect to see git status."
            };
            return div()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element();
        };
        if !status.is_repo {
            return div()
                .text_color(cx.theme().muted_foreground)
                .px_3()
                .py_1()
                .text_sm()
                .child("Not a git repository.")
                .into_any_element();
        }
        let tree = &status.working_tree;
        let has_changes = status.has_working_tree_changes || !tree.files.is_empty();
        let can_push = status.ahead_count > 0 || !status.has_upstream;
        let idle = self.connected && self.git_running.is_none();
        let primary = if has_changes { GitAction::CommitPush } else { GitAction::Push };
        let view = cx.entity().downgrade();
        let is_default = status.is_default_ref;
        let git_menu = Button::new("info-git-menu")
            .ghost()
            .xsmall()
            .icon(icon(IconName::ChevronDown))
            .disabled(!idle)
            .dropdown_menu(move |mut menu, _, _| {
                for (label, action, icon_name, enabled) in [
                    ("Commit", GitAction::Commit, IconName::GitCommitHorizontal, has_changes),
                    ("Push", GitAction::Push, IconName::CloudUpload, can_push || has_changes),
                    ("Create PR", GitAction::CreatePr, IconName::Github, !is_default),
                ] {
                    let view = view.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(icon(icon_name))
                            .disabled(!enabled)
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| this.run_git(action, cx));
                            }),
                    );
                }
                menu
            });
        let git_label = match self.git_running {
            Some(GitAction::Commit) => "Committing…",
            Some(GitAction::Push) => "Pushing…",
            Some(GitAction::CreatePr) => "Creating PR…",
            Some(GitAction::CommitPush) => "Committing & pushing…",
            None if has_changes => "Commit & push",
            None => "Push",
        };
        let primary_enabled = idle && (has_changes || can_push);
        let branch_row = self.render_branch_row(status, cx).into_any_element();
        let git_result = self.render_git_result(cx);
        let git_icon = if self.git_running.is_some() {
            div()
                .size_4()
                .flex_shrink_0()
                .child(crate::ui::loader("info-git-loader", gpui_kit::component::Size::XSmall))
                .into_any_element()
        } else {
            row_icon(IconName::CloudUpload, cx).into_any_element()
        };
        let theme = cx.theme();

        v_flex()
            .child(branch_row)
            .child(
                split_row("info-git", cx)
                    .child(git_icon)
                    .child(row_label(git_label))
                    .when(!primary_enabled, |row| row.text_color(theme.muted_foreground))
                    .when(primary_enabled, |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| this.run_git(primary, cx)))
                    })
                    .child(git_menu),
            )
            .children(git_result)
            .child(
                row("info-changes", cx)
                    .child(row_icon(IconName::FileDiff, cx))
                    .child(row_label("Changes"))
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap_1()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .child(
                                div()
                                    .text_color(theme.success)
                                    .child(format!("+{}", tree.insertions)),
                            )
                            .child(
                                div()
                                    .text_color(theme.danger)
                                    .child(format!("-{}", tree.deletions)),
                            ),
                    )
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(InfoPanelEvent::ShowChanges))),
            )
            .into_any_element()
    }

    fn render_branch_row(&self, status: &WorkspaceGitStatus, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let branch = status.ref_name.clone().unwrap_or_else(|| "(detached)".into());
        let trigger = Button::new("info-branch")
            .ghost()
            .mx_1p5()
            .h_8()
            .px_1p5()
            .disabled(!self.connected)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .font_medium()
                    .child(row_icon(IconName::GitBranch, cx))
                    .child(row_label(branch))
                    .when(status.behind_count > 0 || status.ahead_count > 0, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("↑{} ↓{}", status.ahead_count, status.behind_count)),
                        )
                    })
                    .child(
                        div()
                            .px_1()
                            .child(icon(IconName::ChevronDown).xsmall().text_color(theme.muted_foreground)),
                    ),
            );
        let view = cx.entity().downgrade();
        Popover::new("info-branch-popover")
            .anchor(Anchor::TopRight)
            .p_0()
            .overflow_hidden()
            .open(self.refs_open)
            .on_open_change(move |open, window, cx| {
                let _ = view.update(cx, |this, cx| this.set_refs_open(*open, window, cx));
            })
            .trigger(trigger)
            .child(self.render_ref_list(cx))
    }

    fn render_ref_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let query = self.ref_search.read(cx).value().trim().to_lowercase();
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(refs) = &self.refs {
            let matches = refs
                .refs
                .iter()
                .filter(|reference| !reference.is_remote)
                .filter(|reference| query.is_empty() || reference.name.to_lowercase().contains(&query));
            for reference in matches {
                let name = reference.name.clone();
                // A branch checked out in another worktree cannot be switched to here.
                let elsewhere = !reference.current && reference.worktree_path.is_some();
                let tag = if reference.current {
                    Some("current")
                } else if elsewhere {
                    Some("worktree")
                } else {
                    None
                };
                rows.push(
                    h_flex()
                        .id(SharedString::from(format!("info-ref-{name}")))
                        .w_full()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .when(reference.current, |row| row.bg(theme.list_active))
                        .when(!reference.current && !elsewhere, |row| {
                            row.cursor_pointer().hover(|style| style.bg(theme.list_hover))
                        })
                        .when(elsewhere, |row| row.text_color(theme.muted_foreground))
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .children(tag.map(|tag| {
                            div().flex_shrink_0().text_xs().text_color(theme.muted_foreground).child(tag)
                        }))
                        .when(!reference.current && !elsewhere && self.connected, |row| {
                            row.on_click(cx.listener(move |this, _, window, cx| {
                                this.switch_ref(name.clone(), window, cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .px_2()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.refs.is_none() { "Loading refs…" } else { "No matching refs" })
                    .into_any_element(),
            );
        }
        v_flex()
            .w(px(280.))
            .child(
                div()
                    .p_1p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Input::new(&self.ref_search).small().prefix(icon(IconName::Search).xsmall())),
            )
            .child(
                div()
                    .id("info-ref-list")
                    .max_h(px(280.))
                    .p_1()
                    .overflow_y_scrollbar()
                    .child(v_flex().children(rows)),
            )
    }

    fn render_git_result(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let result = self.git_result.as_ref()?;
        let line = match result {
            Ok(outcome) => {
                let url = outcome.pr_url.clone();
                h_flex()
                    .gap_1p5()
                    .text_color(theme.success)
                    .child(icon(IconName::CircleCheck).xsmall().flex_shrink_0())
                    .child(
                        div().flex_1().min_w_0().child(match &outcome.description {
                            Some(description) => format!("{} · {description}", outcome.title),
                            None => outcome.title.clone(),
                        }),
                    )
                    .when_some(url, |line, url| {
                        line.child(
                            Button::new("info-open-pr")
                                .ghost()
                                .xsmall()
                                .icon(icon(IconName::ExternalLink))
                                .tooltip("Open pull request")
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    })
            }
            Err(error) => h_flex()
                .gap_1p5()
                .text_color(theme.danger)
                .child(icon(IconName::CircleAlert).xsmall().flex_shrink_0())
                .child(div().flex_1().min_w_0().child(error.clone())),
        };
        Some(
            line.mx_3()
                .my_0p5()
                .text_xs()
                .child(
                    Button::new("info-dismiss-git")
                        .ghost()
                        .xsmall()
                        .icon(icon(IconName::X))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.git_result = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

fn section_label(text: &'static str, cx: &App) -> impl IntoElement {
    div().px_3().pt_1().pb_1p5().text_xs().text_color(cx.theme().muted_foreground).child(text)
}

/// A full-width clickable row in the card.
fn row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .mx_1p5()
        .h_8()
        .gap_2()
        .px_1p5()
        .rounded_md()
        .text_sm()
        .font_medium()
        .cursor_pointer()
        .hover(|style| style.bg(theme.list_hover))
}

/// A row whose trailing chevron opens a menu of alternatives.
fn split_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    row(id, cx).pr_0p5()
}

fn row_icon(name: IconName, cx: &App) -> impl IntoElement {
    icon(name).small().flex_shrink_0().text_color(cx.theme().muted_foreground)
}

fn row_label(text: impl Into<SharedString>) -> impl IntoElement {
    div().flex_1().min_w_0().truncate().child(text.into())
}

/// Display name for an `EditorId` (see `editor.ts`'s `EDITORS`).
pub fn editor_label(id: &str) -> &str {
    match id {
        "cursor" => "Cursor",
        "trae" => "Trae",
        "kiro" => "Kiro",
        "vscode" => "VS Code",
        "vscode-insiders" => "VS Code Insiders",
        "vscodium" => "VSCodium",
        "zed" => "Zed",
        "antigravity" => "Antigravity",
        "idea" => "IntelliJ IDEA",
        "aqua" => "Aqua",
        "clion" => "CLion",
        "datagrip" => "DataGrip",
        "dataspell" => "DataSpell",
        "goland" => "GoLand",
        "phpstorm" => "PhpStorm",
        "pycharm" => "PyCharm",
        "rider" => "Rider",
        "rubymine" => "RubyMine",
        "rustrover" => "RustRover",
        "webstorm" => "WebStorm",
        "file-manager" if cfg!(target_os = "windows") => "File Explorer",
        "file-manager" if cfg!(target_os = "macos") => "Finder",
        "file-manager" => "Files",
        other => other,
    }
}

fn editor_icon(id: &str) -> IconName {
    if id == "file-manager" { IconName::FolderOpen } else { IconName::Code }
}

/// Icon for a `ProjectScriptIcon`.
pub fn script_icon(name: &str) -> IconName {
    match name {
        "test" => IconName::FlaskConical,
        "lint" => IconName::ListChecks,
        "configure" => IconName::Wrench,
        "build" => IconName::Hammer,
        "debug" => IconName::Bug,
        _ => IconName::Play,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn commands_run_in_the_worktree_before_the_checkout() {
        let mut scope = InfoScope { project_root: Some("/repo".into()), ..Default::default() };
        assert_eq!(scope.cwd(), Some("/repo"));
        scope.worktree_path = Some("/worktrees/repo/t3code-1".into());
        assert_eq!(scope.cwd(), Some("/worktrees/repo/t3code-1"));
    }

    #[::core::prelude::v1::test]
    fn editor_labels_match_the_web_app() {
        assert_eq!(editor_label("vscode"), "VS Code");
        assert_eq!(editor_label("zed"), "Zed");
        assert_eq!(editor_label("unknown-editor"), "unknown-editor");
    }
}
