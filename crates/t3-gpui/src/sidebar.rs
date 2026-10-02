//! Thread sidebar: search, thread list and connection status.
//!
//! Lives in its own entity so the "Working" loaders' per-frame redraws (see
//! `Window::request_animation_frame`) dirty only this view and its ancestors;
//! the open thread is a cached sibling and is not re-rendered. The
//! filtered/sorted thread list and the project lookup are cached here too, so
//! those redraws re-layout the cards without re-filtering, re-lowercasing or
//! re-sorting `shell.threads` every frame.

use std::collections::{HashMap, HashSet};

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{ProjectShell, SessionStatus, ShellState, ThreadShell, sort_settled_threads};

use crate::backend::Status;
use crate::ui::{self, SIDEBAR_WIDTH, icon};

pub enum SidebarEvent {
    OpenThread(String),
    OpenDraft(String),
    DiscardDraft(String),
    OpenSettings,
    ToggleUsage,
    LoadArchived(String),
    SwitchServer,
    AddProject,
    NewThread,
    ThreadAction(String, t3_client::ThreadAction),
}

/// A new thread being composed, not yet on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarDraft {
    pub id: String,
    pub project_id: String,
    pub text: String,
}

pub struct Sidebar {
    search: Entity<InputState>,
    /// Draft threads, shown above the thread list.
    new_drafts: Vec<SidebarDraft>,
    /// Existing threads whose composer holds unsent text.
    unsent: HashSet<String>,
    /// Provider instances by id, for each card's account badge.
    providers: HashMap<String, t3_client::ServerProvider>,
    shell: ShellState,
    status: Status,
    open_thread_id: Option<String>,
    /// Unarchived, un-settled, idle threads matching the search query:
    /// pinned first, then most recently updated. Recomputed only when
    /// `shell` or the query changes.
    active: Vec<ThreadShell>,
    /// Matching threads with a turn running, most recently updated first.
    /// Shown in their own collapsible shelf above "Settled".
    working: Vec<ThreadShell>,
    working_expanded: bool,
    usage_open: bool,
    /// Unarchived, settled threads matching the search query (see
    /// `ThreadShell::is_settled`), newest-settled first. Collapsed behind
    /// the "Settled (N)" divider until `settled_expanded` or the user is
    /// searching.
    settled: Vec<ThreadShell>,
    settled_expanded: bool,
    archive_mode: bool,
    archive_request: Option<String>,
    archived: Vec<ThreadShell>,
    archived_projects: HashMap<String, ProjectShell>,
    archive_error: bool,
    restoring: Vec<String>,
    rename: Entity<InputState>,
    renaming: Option<String>,
    rename_pending: bool,
    /// Project lookup by id, rebuilt alongside `active`/`settled` so cards
    /// don't linear-scan `shell.projects` on every render.
    projects: HashMap<String, ProjectShell>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let rename = cx.new(|cx| InputState::new(window, cx).placeholder("Thread title"));
        let mut subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.recompute(cx);
            }
        })];

        subscriptions.push(cx.subscribe(&rename, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_rename(cx);
            }
            cx.notify();
        }));
        Self {
            rename,
            renaming: None,
            rename_pending: false,
            archive_mode: false,
            archive_request: None,
            archived: Vec::new(),
            archived_projects: HashMap::new(),
            archive_error: false,
            restoring: Vec::new(),
            search,
            new_drafts: Vec::new(),
            unsent: HashSet::new(),
            providers: HashMap::new(),
            shell: ShellState::default(),
            status: Status::Connecting(String::new()),
            open_thread_id: None,
            active: Vec::new(),
            working: Vec::new(),
            working_expanded: true,
            usage_open: false,
            settled: Vec::new(),
            settled_expanded: false,
            projects: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_shell(&mut self, shell: ShellState, cx: &mut Context<Self>) {
        self.archived.retain(|archived| {
            !shell
                .threads
                .iter()
                .any(|thread| thread.id == archived.id && thread.archived_at.is_none())
        });
        self.shell = shell;
        self.recompute(cx);
    }

    pub fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        if !matches!(status, Status::Connected(_)) {
            self.archive_request = None;
            self.archived.clear();
            self.archived_projects.clear();
            self.restoring.clear();
            self.rename_pending = false;
        }
        self.status = status;
        if self.archive_mode && matches!(self.status, Status::Connected(_)) {
            self.load_archived(cx);
        }
        cx.notify();
    }

    pub fn reset_environment(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        self.rename_pending = false;
        self.archive_mode = false;
        self.archive_request = None;
        self.archive_error = false;
        self.archived.clear();
        self.archived_projects.clear();
        self.restoring.clear();
        self.open_thread_id = None;
        cx.notify();
    }

    pub fn set_providers(
        &mut self,
        providers: Vec<t3_client::ServerProvider>,
        cx: &mut Context<Self>,
    ) {
        self.providers =
            providers.into_iter().map(|provider| (provider.instance_id.clone(), provider)).collect();
        cx.notify();
    }

    pub fn set_drafts(
        &mut self,
        new_drafts: Vec<SidebarDraft>,
        unsent: HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        if self.new_drafts != new_drafts || self.unsent != unsent {
            self.new_drafts = new_drafts;
            self.unsent = unsent;
            cx.notify();
        }
    }

    pub fn set_usage_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.usage_open = open;
        cx.notify();
    }

    pub fn set_open_thread(&mut self, thread_id: Option<String>, cx: &mut Context<Self>) {
        self.open_thread_id = thread_id;
        cx.notify();
    }

    fn load_archived(&mut self, cx: &mut Context<Self>) {
        let id = t3_client::new_id();
        self.archive_request = Some(id.clone());
        self.archive_error = false;
        cx.emit(SidebarEvent::LoadArchived(id));
        cx.notify();
    }

    pub fn set_archived(
        &mut self,
        request_id: &str,
        snapshot: Option<t3_client::ShellSnapshot>,
        cx: &mut Context<Self>,
    ) {
        if self.archive_request.as_deref() != Some(request_id) {
            return;
        }
        self.archive_request = None;
        self.archive_error = snapshot.is_none();
        if let Some(snapshot) = snapshot {
            self.archived_projects =
                snapshot.projects.into_iter().map(|p| (p.id.clone(), p)).collect();
            self.projects.extend(self.archived_projects.clone());
            self.archived = snapshot
                .threads
                .into_iter()
                .filter(|t| {
                    t.archived_at.is_some()
                        && !(self.shell.sequence >= snapshot.snapshot_sequence
                            && self
                                .shell
                                .thread(&t.id)
                                .is_some_and(|current| current.archived_at.is_none()))
                })
                .collect();
            self.archived.sort_by(|a, b| b.archived_at.cmp(&a.archived_at));
        }
        cx.notify();
    }

    fn submit_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename_pending || !matches!(self.status, Status::Connected(_)) {
            return;
        }
        let Some(id) = self.renaming.clone() else {
            return;
        };
        let title = self.rename.read(cx).value().trim().to_owned();
        if title.is_empty() {
            return;
        }
        self.rename_pending = true;
        cx.emit(SidebarEvent::ThreadAction(id, t3_client::ThreadAction::Rename(title)));
        cx.notify();
    }

    pub fn action_finished(
        &mut self,
        id: &str,
        action: &t3_client::ThreadAction,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        match action {
            t3_client::ThreadAction::Rename(_) if self.renaming.as_deref() == Some(id) => {
                self.rename_pending = false;
                if success {
                    self.renaming = None;
                }
            }
            t3_client::ThreadAction::Unarchive => {
                self.restoring.retain(|pending| pending != id);
                if success {
                    self.archived.retain(|thread| thread.id != id);
                }
            }
            t3_client::ThreadAction::Archive if success && self.archive_mode => {
                self.load_archived(cx)
            }
            _ => {}
        }
        cx.notify();
    }

    /// Rebuilds `active`, `working`, `settled` and `projects` from `shell` and the
    /// search query. Search matches across both: a settled thread that
    /// matches still needs to be findable, it's just collapsed by default
    /// (see `render`, which expands the shelf while searching).
    fn recompute(&mut self, cx: &mut Context<Self>) {
        self.projects = self.archived_projects.clone();
        self.projects.extend(self.shell.projects.iter().map(|p| (p.id.clone(), p.clone())));

        let query = self.search.read(cx).value().trim().to_lowercase();
        let project_title = |id: &str| self.projects.get(id).map(|p| p.title.as_str());
        let matching: Vec<&ThreadShell> = self
            .shell
            .threads
            .iter()
            .filter(|t| t.archived_at.is_none())
            .filter(|t| {
                query.is_empty()
                    || t.title.to_lowercase().contains(&query)
                    || project_title(&t.project_id)
                        .is_some_and(|title| title.to_lowercase().contains(&query))
            })
            .collect();

        let is_working = |t: &ThreadShell| matches!(thread_badge(t), Some(Badge::Working));
        let (mut working, mut active): (Vec<ThreadShell>, Vec<ThreadShell>) = matching
            .iter()
            .filter(|t| !t.is_settled())
            .map(|t| (*t).clone())
            .partition(|t| is_working(t));
        working.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        active.sort_by(|a, b| {
            b.pinned_at
                .is_some()
                .cmp(&a.pinned_at.is_some())
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });

        let mut settled: Vec<ThreadShell> =
            matching.iter().filter(|t| t.is_settled()).map(|t| (*t).clone()).collect();
        sort_settled_threads(&mut settled);

        self.active = active;
        self.working = working;
        self.settled = settled;
        cx.notify();
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let draft_cards: Vec<_> = if self.archive_mode {
            Vec::new()
        } else {
            self.new_drafts
                .iter()
                .filter(|draft| {
                    query.is_empty()
                        || draft.text.to_lowercase().contains(&query)
                        || self
                            .projects
                            .get(&draft.project_id)
                            .is_some_and(|p| p.title.to_lowercase().contains(&query))
                })
                .enumerate()
                .map(|(ix, draft)| self.render_draft_card(ix, draft, cx).into_any_element())
                .collect()
        };
        let mut active_cards: Vec<_> = self
            .active
            .iter()
            .enumerate()
            .map(|(ix, thread)| {
                let active = self.open_thread_id.as_deref() == Some(thread.id.as_str());
                self.render_thread_card("active-thread", ix, thread, active, cx).into_any_element()
            })
            .collect();
        // Searching surfaces collapsed matches too, rather than making the
        // user expand a shelf first to find what they typed for.
        let searching = !self.search.read(cx).value().trim().is_empty();
        let working_expanded = self.working_expanded || searching;
        let working_cards: Vec<_> = if working_expanded && !self.archive_mode {
            self.working
                .iter()
                .enumerate()
                .map(|(ix, thread)| {
                    let active = self.open_thread_id.as_deref() == Some(thread.id.as_str());
                    self.render_thread_card("working-thread", ix, thread, active, cx)
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let settled_expanded = self.settled_expanded || searching;
        let mut settled_cards: Vec<_> = if settled_expanded {
            self.settled
                .iter()
                .enumerate()
                .map(|(ix, thread)| {
                    let active = self.open_thread_id.as_deref() == Some(thread.id.as_str());
                    self.render_thread_card("settled-thread", ix, thread, active, cx)
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        if self.archive_mode {
            let query = self.search.read(cx).value().trim().to_lowercase();
            active_cards = self
                .archived
                .iter()
                .filter(|t| {
                    query.is_empty()
                        || t.title.to_lowercase().contains(&query)
                        || self
                            .projects
                            .get(&t.project_id)
                            .is_some_and(|p| p.title.to_lowercase().contains(&query))
                })
                .enumerate()
                .map(|(ix, t)| {
                    self.render_thread_card("archived-thread", ix, t, false, cx).into_any_element()
                })
                .collect();
            settled_cards.clear();
        }
        let empty = active_cards.is_empty()
            && draft_cards.is_empty()
            && (self.archive_mode || (self.settled.is_empty() && self.working.is_empty()));
        let working_divider = (!self.archive_mode && !self.working.is_empty()).then(|| {
            self.render_shelf_divider(Shelf::Working, working_expanded, cx).into_any_element()
        });
        let divider = (!self.archive_mode && !self.settled.is_empty()).then(|| {
            self.render_shelf_divider(Shelf::Settled, settled_expanded, cx).into_any_element()
        });
        // The shelves sit at the bottom of the sidebar; open, each takes a
        // capped share of the window's height and scrolls on its own.
        let working_max_h = window.viewport_size().height * 0.3;
        let settled_max_h = window.viewport_size().height * 0.4;

        let theme = cx.theme();
        let (label, color) = match &self.status {
            Status::NeedsPairing => ("Not paired".to_owned(), theme.muted_foreground),
            Status::Connecting(_) => ("Connecting…".to_owned(), theme.warning),
            Status::Connected(server) => (server.clone(), theme.success),
            Status::Reconnecting { reason, .. } => {
                (format!("Reconnecting: {reason}"), theme.danger)
            }
        };

        v_flex()
            .w(SIDEBAR_WIDTH)
            .flex_shrink_0()
            .h_full()
            .min_h_0()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .pt_1()
                    .pb_2()
                    .child(div().flex_1().min_w_0().child(
                        Input::new(&self.search).small().appearance(false).cleanable(true).prefix(
                            icon(IconName::Search).small().text_color(theme.muted_foreground),
                        ),
                    ))
                    .child(
                        Button::new("archive-toggle")
                            .ghost()
                            .small()
                            .when(self.archive_mode, |button| button.primary())
                            .icon(icon(IconName::Archive))
                            .tooltip(if self.archive_mode {
                                "Show active threads"
                            } else {
                                "Show archived threads"
                            })
                            .disabled(!matches!(self.status, Status::Connected(_)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.archive_mode = !this.archive_mode;
                                if this.archive_mode {
                                    this.load_archived(cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("add-project")
                            .ghost()
                            .small()
                            .icon(icon(IconName::FolderPlus))
                            .tooltip("Add project")
                            .disabled(!matches!(self.status, Status::Connected(_)))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SidebarEvent::AddProject);
                            })),
                    )
                    .child(
                        Button::new("new-thread")
                            .ghost()
                            .small()
                            .icon(icon(IconName::SquarePen))
                            .tooltip("New thread")
                            .disabled(!matches!(self.status, Status::Connected(_)))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SidebarEvent::NewThread);
                            })),
                    ),
            )
            .when(self.archive_mode, |sidebar| {
                sidebar.child(
                    h_flex()
                        .px_3()
                        .pb_2()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .text_xs()
                                .font_medium()
                                .text_color(theme.muted_foreground)
                                .child(icon(IconName::Archive).xsmall())
                                .child(format!("Archived · {}", self.archived.len())),
                        )
                        .child(
                            Button::new("archive-refresh")
                                .ghost()
                                .xsmall()
                                .icon(icon(IconName::RefreshCw))
                                .tooltip("Refresh archived threads")
                                .disabled(
                                    self.archive_request.is_some()
                                        || !matches!(self.status, Status::Connected(_)),
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.load_archived(cx))),
                        ),
                )
            })
            .when(self.renaming.is_some(), |sidebar| {
                sidebar.child(
                    v_flex()
                        .px_3()
                        .pb_2()
                        .gap_2()
                        .child(
                            Input::new(&self.rename)
                                .small()
                                .disabled(self.rename_pending)
                                .aria_label("Thread title"),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("rename-save")
                                        .small()
                                        .label("Save")
                                        .disabled(
                                            self.rename_pending
                                                || self.rename.read(cx).value().trim().is_empty()
                                                || !matches!(self.status, Status::Connected(_)),
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.submit_rename(cx)),
                                        ),
                                )
                                .child(
                                    Button::new("rename-cancel")
                                        .ghost()
                                        .small()
                                        .label("Cancel")
                                        .disabled(self.rename_pending)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.renaming = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
            .child(
                div().id("thread-list").flex_1().min_h_0().px_2().overflow_y_scrollbar().child(
                    v_flex()
                        .gap_0p5()
                        .pb_2()
                        .children(draft_cards)
                        .children(active_cards)
                        .when(empty, |list| {
                            list.child(
                                div()
                                    .px_3()
                                    .py_4()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(if self.archive_mode {
                                        if self.archive_request.is_some() {
                                            "Loading archived threads…"
                                        } else if self.archive_error {
                                            "Could not load archive. Try Refresh."
                                        } else {
                                            "No archived threads match"
                                        }
                                    } else if self.shell.threads.is_empty() {
                                        "No threads yet"
                                    } else {
                                        "No matching threads"
                                    }),
                            )
                        }),
                ),
            )
            .when_some(working_divider, |sidebar, divider| {
                sidebar.child(
                    v_flex()
                        .flex_none()
                        .px_2()
                        .pt_1()
                        .border_t_1()
                        .border_color(theme.sidebar_border)
                        .child(divider)
                        .when(!working_cards.is_empty(), |shelf| {
                            shelf.child(
                                div()
                                    .id("working-list")
                                    .max_h(working_max_h)
                                    .overflow_y_scrollbar()
                                    .child(
                                        v_flex().gap_0p5().pb_2().children(working_cards),
                                    ),
                            )
                        }),
                )
            })
            .when_some(divider, |sidebar, divider| {
                sidebar.child(
                    v_flex()
                        .flex_none()
                        .px_2()
                        .pt_1()
                        .border_t_1()
                        .border_color(theme.sidebar_border)
                        .child(divider)
                        .when(!settled_cards.is_empty(), |shelf| {
                            shelf.child(
                                div()
                                    .id("settled-list")
                                    .max_h(settled_max_h)
                                    .overflow_y_scrollbar()
                                    .child(
                                        v_flex().gap_0p5().pb_2().children(settled_cards),
                                    ),
                            )
                        }),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_2()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(div().size_2().flex_shrink_0().rounded_full().bg(color))
                            .child(div().min_w_0().truncate().child(label)),
                    )
                    .when(self.status != Status::NeedsPairing, |footer| {
                        footer.child(
                            Button::new("switch-server")
                                .ghost()
                                .small()
                                .icon(icon(IconName::Plug))
                                .tooltip("Switch server")
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(SidebarEvent::SwitchServer);
                                })),
                        )
                    })
                    .child(
                        Button::new("sidebar-settings")
                            .ghost()
                            .small()
                            .icon(icon(IconName::Settings))
                            .tooltip("Settings")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SidebarEvent::OpenSettings);
                            })),
                    )
                    .child(
                        Button::new("sidebar-usage")
                            .ghost()
                            .small()
                            .when(self.usage_open, |button| button.primary())
                            .icon(icon(IconName::ChartNoAxesColumn))
                            .label("Usage")
                            .accessibility_label("Usage: cost, tokens and subscription limits")
                            .tooltip("Usage")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SidebarEvent::ToggleUsage);
                            })),
                    ),
            )
    }
}

impl Sidebar {
    /// A shelf's divider, such as the collapsed "Settled (N)": label, a thin
    /// rule, and a chevron that flips to expand. Expanding reveals the full
    /// list in place, same as the T3 desktop app's shelf.
    fn render_shelf_divider(
        &self,
        shelf: Shelf,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (id, name, count) = match shelf {
            Shelf::Working => ("working-divider", "Working", self.working.len()),
            Shelf::Settled => ("settled-divider", "Settled", self.settled.len()),
        };
        let label = if expanded { name.to_owned() } else { format!("{name} ({count})") };

        h_flex()
            .id(id)
            .test_support()
            .gap_2()
            .items_center()
            .px_3()
            .py_1p5()
            .cursor_pointer()
            .text_xs()
            .text_color(theme.muted_foreground)
            .on_click(cx.listener(move |this, _, _, cx| {
                match shelf {
                    Shelf::Working => this.working_expanded = !this.working_expanded,
                    Shelf::Settled => this.settled_expanded = !this.settled_expanded,
                }
                cx.notify();
            }))
            .child(label)
            .child(div().flex_1().h(px(1.)).bg(theme.sidebar_border))
            .child(
                icon(if expanded { IconName::ChevronUp } else { IconName::ChevronDown }).xsmall(),
            )
    }

    /// A draft thread's card: pencil, project, and the first line of its
    /// unsent text; clicking reopens it and the cross discards it.
    fn render_draft_card(
        &self,
        ix: usize,
        draft: &SidebarDraft,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let active = self.open_thread_id.as_deref() == Some(draft.id.as_str());
        let project = self.projects.get(&draft.project_id);
        let first_line = draft.text.lines().map(str::trim).find(|line| !line.is_empty());
        let open_id = draft.id.clone();
        let discard_id = draft.id.clone();

        v_flex()
            .id(("draft-thread", ix))
            .test_support()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_dashed()
            .border_color(theme.primary.opacity(if active { 0.45 } else { 0.25 }))
            .cursor_pointer()
            .when(active, |card| card.bg(theme.sidebar_accent))
            .when(!active, |card| card.hover(|style| style.bg(theme.list_hover)))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(SidebarEvent::OpenDraft(open_id.clone()));
            }))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(icon(IconName::SquarePen).xsmall().text_color(theme.primary))
                    .when_some(project, |row, project| {
                        row.child(ui::project_tag(&project.id, &project.title)).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(project.title.clone()),
                        )
                    })
                    .when(project.is_none(), |row| row.child(div().flex_1()))
                    .child(div().text_color(theme.primary).child("Draft"))
                    .child(
                        Button::new(SharedString::from(format!("discard-draft-{}", draft.id)))
                            .ghost()
                            .xsmall()
                            .icon(icon(IconName::X))
                            .tooltip("Discard draft")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                cx.emit(SidebarEvent::DiscardDraft(discard_id.clone()));
                            })),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .when(first_line.is_none(), |title| {
                        title.italic().text_color(theme.muted_foreground)
                    })
                    .when(first_line.is_some(), |title| {
                        title.text_color(theme.foreground.opacity(0.85))
                    })
                    .child(first_line.unwrap_or("New thread").to_owned()),
            )
    }

    fn render_thread_card(
        &self,
        list: &'static str,
        ix: usize,
        thread: &ThreadShell,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let project = self.projects.get(&thread.project_id);
        let thread_id = thread.id.clone();
        let menu_view = cx.entity().downgrade();
        let menu_thread_id = thread.id.clone();
        let pinned = thread.pinned_at.is_some();
        let settled = thread.is_settled();
        let connected = matches!(self.status, Status::Connected(_));
        let archived = thread.archived_at.is_some();
        let title = thread.title.clone();
        let menu = Button::new(SharedString::from(format!("thread-menu-{}", thread.id)))
            .ghost()
            .xsmall()
            .icon(icon(IconName::Ellipsis))
            .tooltip("Thread actions")
            .disabled(!connected || self.restoring.contains(&thread.id))
            .on_click(|_, _, cx| cx.stop_propagation())
            .dropdown_menu(move |mut menu, _, _| {
                let view = menu_view.clone();
                let id = menu_thread_id.clone();
                let title = title.clone();
                menu = menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        if this.rename_pending {
                            return;
                        }
                        this.renaming = Some(id.clone());
                        this.rename.update(cx, |input, cx| {
                            input.set_value(title.clone(), window, cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    });
                }));
                for (label, action) in [
                    (if pinned { "Unpin" } else { "Pin" }, t3_client::ThreadAction::Pin(!pinned)),
                    (
                        if settled { "Move to active" } else { "Settle" },
                        t3_client::ThreadAction::Settle(!settled),
                    ),
                    ("Archive", t3_client::ThreadAction::Archive),
                ] {
                    let view = menu_view.clone();
                    let id = menu_thread_id.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let _ = view.update(cx, |_, cx| {
                            cx.emit(SidebarEvent::ThreadAction(id.clone(), action.clone()))
                        });
                    }));
                }
                menu
            });
        // Distinct from `list` so the loader's id never collides with the
        // card's own id (both would otherwise share `(list, ix)`).
        let working_key: &'static str = match list {
            "active-thread" => "active-working",
            "working-thread" => "working-working",
            _ => "settled-working",
        };

        let trailing = match thread_badge(thread) {
            Some(badge) => h_flex()
                .gap_1()
                .text_color(badge.color(cx))
                .child(match badge {
                    Badge::Working => {
                        ui::loader((working_key, ix), Size::XSmall).into_any_element()
                    }
                    _ => icon(IconName::CircleAlert).xsmall().into_any_element(),
                })
                .child(match badge {
                    Badge::Working => working_since(thread)
                        .and_then(ui::elapsed)
                        .map_or_else(|| "Working".to_owned(), |time| format!("Working {time}")),
                    _ => badge.label().to_owned(),
                })
                .into_any_element(),
            None => div()
                .text_color(theme.muted_foreground)
                .children(ui::relative_time(&thread.updated_at))
                .into_any_element(),
        };
        let unsent = self.unsent.contains(&thread.id);
        // The account the thread runs on: its provider instance's mark.
        let account = thread
            .model_selection
            .as_ref()
            .and_then(|selection| selection["instanceId"].as_str())
            .and_then(|instance| self.providers.get(instance))
            .map(|provider| {
                crate::model_picker::provider_mark(
                    &provider.instance_id,
                    &provider.driver,
                    &crate::model_picker::provider_name(provider),
                    px(16.),
                )
            });

        v_flex()
            .id((list, ix))
            .test_support()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(transparent_black())
            .cursor_pointer()
            .when(active, |card| {
                card.bg(theme.sidebar_accent).border_color(theme.primary.opacity(0.25))
            })
            .when(!active, |card| card.hover(|style| style.bg(theme.list_hover)))
            .on_click(cx.listener(move |_, _, _, cx| {
                if !archived {
                    cx.emit(SidebarEvent::OpenThread(thread_id.clone()));
                }
            }))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .when_some(project, |row, project| {
                        row.child(ui::project_tag(&project.id, &project.title)).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(project.title.clone()),
                        )
                    })
                    .when(project.is_none(), |row| row.child(div().flex_1()))
                    .when(unsent, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("unsent-{}", thread.id)))
                                .child(icon(IconName::PencilLine).xsmall().text_color(theme.primary))
                                .tooltip(|window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new("Unsent draft")
                                        .build(window, cx)
                                }),
                        )
                    })
                    .child(trailing)
                    .when(pinned, |row| {
                        row.child(icon(IconName::Pin).xsmall().text_color(theme.muted_foreground))
                    })
                    .when(!archived, |row| row.child(menu))
                    .when(archived, |row| {
                        let id = thread.id.clone();
                        row.child(
                            Button::new(SharedString::from(format!("restore-{}", id)))
                                .ghost()
                                .xsmall()
                                .label("Restore")
                                .disabled(!connected || self.restoring.contains(&id))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if this.restoring.contains(&id) {
                                        return;
                                    }
                                    this.restoring.push(id.clone());
                                    cx.emit(SidebarEvent::ThreadAction(
                                        id.clone(),
                                        t3_client::ThreadAction::Unarchive,
                                    ));
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .when(active, |title| title.font_semibold())
                    .when(!active, |title| title.text_color(theme.foreground.opacity(0.85)))
                    .child(ui::display_title(&thread.title)),
            )
            .when(thread.branch.is_some() || account.is_some(), |card| {
                card.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .children(thread.branch.clone().map(|branch| {
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(icon(IconName::GitBranch).xsmall())
                                .child(div().min_w_0().truncate().child(branch))
                        }))
                        .when(thread.branch.is_none(), |row| row.child(div().flex_1()))
                        .children(account.map(|mark| div().pr_0p5().child(mark))),
                )
            })
    }
}

#[derive(Clone, Copy)]
enum Badge {
    NeedsInput,
    Working,
    Failed,
}

impl Badge {
    fn label(self) -> &'static str {
        match self {
            Badge::NeedsInput => "Needs you",
            Badge::Working => "Working",
            Badge::Failed => "Failed",
        }
    }

    fn color(self, cx: &App) -> Hsla {
        match self {
            Badge::NeedsInput => cx.theme().warning,
            Badge::Working => cx.theme().info,
            Badge::Failed => cx.theme().danger,
        }
    }
}

/// The collapsible lists at the bottom of the sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shelf {
    Working,
    Settled,
}

/// When the running turn started, for the "Working 7m" label.
fn working_since(thread: &ThreadShell) -> Option<&str> {
    let turn = thread.latest_turn.as_ref()?;
    turn.started_at.as_deref().or(turn.requested_at.as_deref())
}

fn thread_badge(thread: &ThreadShell) -> Option<Badge> {
    if thread.has_pending_approvals || thread.has_pending_user_input {
        return Some(Badge::NeedsInput);
    }
    match thread.session.as_ref()?.status {
        SessionStatus::Starting | SessionStatus::Running => Some(Badge::Working),
        SessionStatus::Error => Some(Badge::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    fn thread(id: &str, archived: bool) -> ThreadShell {
        serde_json::from_value(json!({ "id": id, "projectId": "project-1", "title": "Original title", "runtimeMode": "full-access", "archivedAt": if archived { Some("2026-10-01T00:00:00Z") } else { None } })).unwrap()
    }

    fn sidebar(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Sidebar>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(800.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let sidebar = cx.new(|cx| Sidebar::new(window, cx));
                    sidebar.update(cx, |sidebar, cx| {
                        sidebar.set_status(Status::Connected("Test server".into()), cx)
                    });
                    sidebar
                },
            )
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn active_threads_scroll_without_moving_the_settings_footer(cx: &mut TestAppContext) {
        let (handle, sidebar) = sidebar(cx);
        cx.update_window(handle, |_, window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                let mut shell = ShellState::default();
                shell.threads =
                    (0..40).map(|index| thread(&format!("thread-{index:02}"), false)).collect();
                sidebar.set_shell(shell, cx);
            });
            window.render_frame(cx);
            let first = window.find(("active-thread", 0usize)).bounds();
            let footer = window.find("sidebar-settings").bounds();
            window.scroll(
                ("active-thread", 0usize),
                ScrollDelta::Pixels(point(px(0.), px(-240.))),
                cx,
            );
            let scrolled = window.try_find(("active-thread", 0usize));
            assert!(
                scrolled.is_none_or(|row| !row.visible() || row.bounds().origin.y < first.origin.y)
            );
            assert!(window.find("sidebar-settings").visible());
            assert_eq!(window.find("sidebar-settings").bounds(), footer);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn working_threads_sit_in_a_collapsible_shelf_above_settled(cx: &mut TestAppContext) {
        let (handle, sidebar) = sidebar(cx);
        let events = Rc::new(RefCell::new(0));
        let capture = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &SidebarEvent, _| {
                if matches!(event, SidebarEvent::ToggleUsage) {
                    *capture.borrow_mut() += 1;
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                let mut running = thread("running", false);
                running.session = serde_json::from_value(json!({ "status": "running" })).unwrap();
                let mut settled = thread("settled", false);
                settled.settled_override = Some("settled".into());
                let shell = ShellState {
                    threads: vec![thread("idle", false), running, settled],
                    ..Default::default()
                };
                sidebar.set_shell(shell, cx);
                assert_eq!(sidebar.active.len(), 1);
                assert_eq!(sidebar.working.len(), 1);
            });
            window.render_frame(cx);
            assert!(window.find(("working-thread", 0usize)).visible());
            let working = window.find("working-divider").bounds();
            let settled = window.find("settled-divider").bounds();
            assert!(working.origin.y < settled.origin.y);
            assert!(window.find(("active-thread", 0usize)).bounds().origin.y < working.origin.y);

            window.click("working-divider", cx);
            window.render_frame(cx);
            assert!(window.try_find(("working-thread", 0usize)).is_none());

            window.click("sidebar-usage", cx);
        })
        .unwrap();
        assert_eq!(*events.borrow(), 1);
    }

    #[gpui_kit::test]
    fn sidebar_settings_button_emits_open_event(cx: &mut TestAppContext) {
        let (handle, sidebar) = sidebar(cx);
        let events = Rc::new(RefCell::new(Vec::new()));
        let capture = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &SidebarEvent, _| {
                if matches!(event, SidebarEvent::OpenSettings) {
                    capture.borrow_mut().push(());
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("sidebar-settings", cx);
        })
        .unwrap();
        assert_eq!(events.borrow().len(), 1);
    }

    #[gpui_kit::test]
    fn rename_keeps_failed_draft_and_blocks_duplicate_and_offline_submissions(
        cx: &mut TestAppContext,
    ) {
        let (handle, sidebar) = sidebar(cx);
        let actions = Rc::new(RefCell::new(Vec::new()));
        let capture = actions.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &SidebarEvent, _| {
                if let SidebarEvent::ThreadAction(id, action) = event {
                    capture.borrow_mut().push((id.clone(), action.clone()));
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_shell(
                    ShellState {
                        sequence: 1,
                        synchronized: true,
                        threads: vec![thread("thread-1", false)],
                        ..Default::default()
                    },
                    cx,
                );
            });
            window.render_frame(cx);
            window.click("thread-menu-thread-1", cx);
            window.press("down", cx);
            window.press("enter", cx);
            assert_eq!(sidebar.read(cx).renaming.as_deref(), Some("thread-1"));
            sidebar.read(cx).rename.clone().update(cx, |input, cx| input.set_value("", window, cx));
            window.render_frame(cx);
            window.click("rename-save", cx);
            assert!(!sidebar.read(cx).rename_pending);
            sidebar.read(cx).rename.clone().update(cx, |input, cx| input.focus(window, cx));
            window.input("  Updated title  ", cx);
            window.click("rename-save", cx);
            assert!(sidebar.read(cx).rename_pending);
            window.click("rename-save", cx);
        })
        .unwrap();
        assert_eq!(
            *actions.borrow(),
            vec![("thread-1".into(), t3_client::ThreadAction::Rename("Updated title".into()))]
        );
        cx.update_window(handle, |_, window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.action_finished(
                    "thread-1",
                    &t3_client::ThreadAction::Rename("Updated title".into()),
                    false,
                    cx,
                )
            });
            assert_eq!(sidebar.read(cx).rename.read(cx).value().trim(), "Updated title");
            sidebar.update(cx, |sidebar, cx| sidebar.set_status(Status::NeedsPairing, cx));
            window.render_frame(cx);
            window.click("rename-save", cx);
            assert!(!sidebar.read(cx).rename_pending);
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_status(Status::Connected("Test server".into()), cx)
            });
            window.render_frame(cx);
            window.click("rename-save", cx);
            sidebar.update(cx, |sidebar, cx| {
                sidebar.action_finished(
                    "thread-1",
                    &t3_client::ThreadAction::Rename("Updated title".into()),
                    true,
                    cx,
                )
            });
            assert!(sidebar.read(cx).renaming.is_none());
        })
        .unwrap();
        assert_eq!(actions.borrow().len(), 2);
    }

    #[gpui_kit::test]
    fn archive_browsing_ignores_stale_queries_and_restore_can_retry(cx: &mut TestAppContext) {
        let (handle, sidebar) = sidebar(cx);
        let actions = Rc::new(RefCell::new(Vec::new()));
        let capture = actions.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &SidebarEvent, _| {
                if let SidebarEvent::ThreadAction(_, action) = event {
                    capture.borrow_mut().push(action.clone());
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("archive-toggle", cx);
            let request = sidebar.read(cx).archive_request.clone().unwrap();
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_archived(
                    "stale",
                    Some(t3_client::ShellSnapshot {
                        snapshot_sequence: 1,
                        projects: vec![],
                        threads: vec![thread("wrong", true)],
                    }),
                    cx,
                )
            });
            assert!(sidebar.read(cx).archived.is_empty());
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_archived(
                    &request,
                    Some(t3_client::ShellSnapshot {
                        snapshot_sequence: 1,
                        projects: vec![],
                        threads: vec![thread("archived-1", true)],
                    }),
                    cx,
                )
            });
            window.render_frame(cx);
            window.click("restore-archived-1", cx);
            window.click("restore-archived-1", cx);
            assert_eq!(sidebar.read(cx).restoring.len(), 1);
            sidebar.update(cx, |sidebar, cx| {
                sidebar.action_finished(
                    "archived-1",
                    &t3_client::ThreadAction::Unarchive,
                    false,
                    cx,
                )
            });
            window.render_frame(cx);
            window.click("restore-archived-1", cx);
            sidebar.update(cx, |sidebar, cx| {
                sidebar.action_finished("archived-1", &t3_client::ThreadAction::Unarchive, true, cx)
            });
            assert!(sidebar.read(cx).archived.is_empty());
            assert!(sidebar.read(cx).restoring.is_empty());
            // A query taken before the live restore cannot reintroduce the thread.
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_shell(
                    ShellState {
                        sequence: 3,
                        synchronized: true,
                        threads: vec![thread("archived-1", false)],
                        ..Default::default()
                    },
                    cx,
                );
                sidebar.load_archived(cx);
                let request = sidebar.archive_request.clone().unwrap();
                sidebar.set_archived(
                    &request,
                    Some(t3_client::ShellSnapshot {
                        snapshot_sequence: 2,
                        projects: vec![],
                        threads: vec![thread("archived-1", true)],
                    }),
                    cx,
                );
                assert!(sidebar.archived.is_empty());
            });
        })
        .unwrap();
        assert_eq!(
            *actions.borrow(),
            vec![t3_client::ThreadAction::Unarchive, t3_client::ThreadAction::Unarchive]
        );
    }
}
