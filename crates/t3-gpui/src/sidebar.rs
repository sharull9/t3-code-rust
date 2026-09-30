//! Thread sidebar: search, thread list and connection status.
//!
//! Lives in its own entity so the "Working" loader's per-frame redraws (see
//! `Window::request_animation_frame`) dirty only this view and its ancestors;
//! the open thread is a cached sibling and is not re-rendered. The
//! filtered/sorted thread list and the project lookup are cached here too, so
//! those redraws re-layout the cards without re-filtering, re-lowercasing or
//! re-sorting `shell.threads` every frame.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{ProjectShell, SessionStatus, ShellState, ThreadShell, sort_settled_threads};

use crate::backend::Status;
use crate::ui::{self, SIDEBAR_WIDTH, icon};

pub enum SidebarEvent {
    OpenThread(String),
    SwitchServer,
    AddProject,
    NewThread,
}

pub struct Sidebar {
    search: Entity<InputState>,
    shell: ShellState,
    status: Status,
    open_thread_id: Option<String>,
    /// Unarchived, un-settled threads matching the search query: pinned
    /// first, then most recently updated. Recomputed only when `shell` or
    /// the query changes.
    active: Vec<ThreadShell>,
    /// Unarchived, settled threads matching the search query (see
    /// `ThreadShell::is_settled`), newest-settled first. Collapsed behind
    /// the "Settled (N)" divider until `settled_expanded` or the user is
    /// searching.
    settled: Vec<ThreadShell>,
    settled_expanded: bool,
    /// Project lookup by id, rebuilt alongside `active`/`settled` so cards
    /// don't linear-scan `shell.projects` on every render.
    projects: HashMap<String, ProjectShell>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.recompute(cx);
            }
        })];

        Self {
            search,
            shell: ShellState::default(),
            status: Status::Connecting(String::new()),
            open_thread_id: None,
            active: Vec::new(),
            settled: Vec::new(),
            settled_expanded: false,
            projects: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_shell(&mut self, shell: ShellState, cx: &mut Context<Self>) {
        self.shell = shell;
        self.recompute(cx);
    }

    pub fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }

    pub fn set_open_thread(&mut self, thread_id: Option<String>, cx: &mut Context<Self>) {
        self.open_thread_id = thread_id;
        cx.notify();
    }

    /// Rebuilds `active`, `settled` and `projects` from `shell` and the
    /// search query. Search matches across both: a settled thread that
    /// matches still needs to be findable, it's just collapsed by default
    /// (see `render`, which expands the shelf while searching).
    fn recompute(&mut self, cx: &mut Context<Self>) {
        self.projects = self.shell.projects.iter().map(|p| (p.id.clone(), p.clone())).collect();

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

        let mut active: Vec<ThreadShell> =
            matching.iter().filter(|t| !t.is_settled()).map(|t| (*t).clone()).collect();
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
        self.settled = settled;
        cx.notify();
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_cards: Vec<_> = self
            .active
            .iter()
            .enumerate()
            .map(|(ix, thread)| {
                let active = self.open_thread_id.as_deref() == Some(thread.id.as_str());
                self.render_thread_card("active-thread", ix, thread, active, cx).into_any_element()
            })
            .collect();
        // Searching surfaces settled matches too, rather than making the
        // user expand the shelf first to find what they typed for.
        let searching = !self.search.read(cx).value().trim().is_empty();
        let settled_expanded = self.settled_expanded || searching;
        let settled_cards: Vec<_> = if settled_expanded {
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
        let empty = active_cards.is_empty() && self.settled.is_empty();
        let divider = (!self.settled.is_empty())
            .then(|| self.render_settled_divider(settled_expanded, cx).into_any_element());

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
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .gap_2()
                    .px_3()
                    .pt_1()
                    .pb_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .small()
                                .appearance(false)
                                .cleanable(true)
                                .prefix(
                                    icon(IconName::Search).small().text_color(theme.muted_foreground),
                                ),
                        ),
                    )
                    .child(
                        Button::new("add-project")
                            .ghost()
                            .small()
                            .icon(icon(IconName::FolderPlus))
                            .tooltip("Add project")
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
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SidebarEvent::NewThread);
                            })),
                    ),
            )
            .child(
                div()
                    .id("thread-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .pb_2()
                            .children(active_cards)
                            .children(divider)
                            .children(settled_cards)
                            .when(empty, |list| {
                                list.child(
                                    div()
                                        .px_3()
                                        .py_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(if self.shell.threads.is_empty() {
                                            "No threads yet"
                                        } else {
                                            "No matching threads"
                                        }),
                                )
                            }),
                    ),
            )
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
                    }),
            )
    }
}

impl Sidebar {
    /// The collapsed "Settled (N)" divider: label, a thin rule, and a
    /// chevron that flips to expand. Expanding reveals the full settled
    /// list in place, same as the T3 desktop app's shelf.
    fn render_settled_divider(&self, expanded: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let label = if expanded { "Settled".to_owned() } else { format!("Settled ({})", self.settled.len()) };

        h_flex()
            .id("settled-divider")
            .gap_2()
            .items_center()
            .px_3()
            .py_1p5()
            .cursor_pointer()
            .text_xs()
            .text_color(theme.muted_foreground)
            .on_click(cx.listener(|this, _, _, cx| {
                this.settled_expanded = !this.settled_expanded;
                cx.notify();
            }))
            .child(label)
            .child(div().flex_1().h(px(1.)).bg(theme.sidebar_border))
            .child(icon(if expanded { IconName::ChevronUp } else { IconName::ChevronDown }).xsmall())
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
        // Distinct from `list` so the loader's id never collides with the
        // card's own id (both would otherwise share `(list, ix)`).
        let working_key: &'static str =
            if list == "active-thread" { "active-working" } else { "settled-working" };

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
                .child(badge.label())
                .into_any_element(),
            None => div()
                .text_color(theme.muted_foreground)
                .children(ui::relative_time(&thread.updated_at))
                .into_any_element(),
        };

        v_flex()
            .id((list, ix))
            .gap_1()
            .px_3()
            .py_2()
            .rounded_lg()
            .cursor_pointer()
            .when(active, |card| card.bg(theme.sidebar_accent))
            .when(!active, |card| card.hover(|style| style.bg(theme.list_hover)))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(SidebarEvent::OpenThread(thread_id.clone()));
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
                    .child(trailing),
            )
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .when(active, |title| title.font_semibold())
                    .when(!active, |title| title.text_color(theme.foreground.opacity(0.85)))
                    .child(thread.title.clone()),
            )
            .children(thread.branch.clone().map(|branch| {
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(icon(IconName::GitBranch).xsmall())
                    .child(div().min_w_0().truncate().child(branch))
            }))
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
