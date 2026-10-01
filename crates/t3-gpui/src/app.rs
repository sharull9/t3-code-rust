//! Window root: title bar, thread sidebar, connection status and the open thread.

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use t3_client::{ProjectShell, ShellState, ThreadShell};

use crate::backend::{Backend, Command, Event, Status};
use crate::project_picker::{ProjectPicker, ProjectPickerEvent};
use crate::sidebar::{Sidebar, SidebarEvent};
use crate::thread_view::{ThreadView, ThreadViewEvent};
use crate::ui::{self, SIDEBAR_WIDTH, icon};
use crate::user_input::UserInputPanel;

/// Compatibility fallback when neither the project, current thread nor server
/// config supplies a model. Prefer server-advertised models above this value.
fn fallback_model_selection() -> serde_json::Value {
    json!({ "instanceId": "claudeAgent", "model": "claude-fable-5-1" })
}

pub struct T3App {
    backend: Backend,
    status: Status,
    error: Option<SharedString>,
    shell: ShellState,
    thread: Option<Entity<ThreadView>>,
    pairing_link: Entity<InputState>,
    sidebar: Entity<Sidebar>,
    sidebar_open: bool,
    project_picker: Entity<ProjectPicker>,
    /// A thread just dispatched via `thread.create`, opened as soon as its
    /// shell entry streams in (see `handle_event`'s `Event::Shell` arm).
    pending_new_thread_id: Option<String>,
    /// Pairing screen opened by hand while already paired, to switch servers.
    switching_server: bool,
    providers: Vec<t3_client::ServerProvider>,
    drafts: HashMap<String, String>,
    question_panels: HashMap<String, Entity<UserInputPanel>>,
    sending: HashSet<String>,
    _thread_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl T3App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (backend, events) = Backend::spawn();
        Self::listen(events, window, cx);

        let pairing_link =
            cx.new(|cx| InputState::new(window, cx).placeholder("Pairing link or token"));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        let project_picker = cx.new(|cx| ProjectPicker::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&pairing_link, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.pair(window, cx);
                }
            }),
            cx.subscribe_in(&sidebar, window, |this, _, event: &SidebarEvent, window, cx| {
                match event {
                    SidebarEvent::OpenThread(thread_id) => {
                        this.open_thread(thread_id.clone(), window, cx)
                    }
                    SidebarEvent::SwitchServer => {
                        this.switching_server = true;
                        this.pairing_link.update(cx, |state, cx| state.focus(window, cx));
                        cx.notify();
                    }
                    SidebarEvent::AddProject => this.add_project(window, cx),
                    SidebarEvent::NewThread => {
                        let projects = this.shell.projects.clone();
                        this.project_picker
                            .update(cx, |picker, cx| picker.open(projects, window, cx));
                    }
                    SidebarEvent::ThreadAction(thread_id, action) => {
                        this.backend.send(Command::ThreadAction {
                            thread_id: thread_id.clone(),
                            action: action.clone(),
                        });
                    }
                }
            }),
            cx.subscribe_in(
                &project_picker,
                window,
                |this, _, event: &ProjectPickerEvent, window, cx| match event {
                    ProjectPickerEvent::Select(project) => {
                        this.create_thread_in(project.clone(), window, cx)
                    }
                    ProjectPickerEvent::Cancel => {}
                },
            ),
        ];

        Self {
            backend,
            status: Status::Connecting(String::new()),
            error: None,
            shell: ShellState::default(),
            thread: None,
            pairing_link,
            sidebar,
            sidebar_open: true,
            project_picker,
            pending_new_thread_id: None,
            switching_server: false,
            providers: Vec::new(),
            drafts: HashMap::new(),
            question_panels: HashMap::new(),
            sending: HashSet::new(),
            _thread_subscription: None,
            _subscriptions: subscriptions,
        }
    }

    /// Bridges backend events onto the window: spawned *with* the window
    /// (rather than `cx.spawn`) so `handle_event` can open a thread as soon
    /// as it streams in, which needs one to create the `ThreadView`.
    fn listen(mut events: UnboundedReceiver<Event>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |app, window, cx| app.handle_event(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Status(status) => {
                let connected = matches!(status, Status::Connected(_));
                if connected {
                    self.error = None;
                    // Fresh subscriptions resend snapshots; drop stale state.
                    self.shell = ShellState::default();
                    let shell = self.shell.clone();
                    self.sidebar.update(cx, |sidebar, cx| sidebar.set_shell(shell, cx));
                }
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| {
                        if connected {
                            view.reset(cx);
                        }
                        view.set_connected(connected, cx);
                    });
                }
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_status(status.clone(), cx));
                self.status = status;
            }
            Event::Shell(item) => {
                self.shell.apply(item);
                if self.thread.as_ref().is_some_and(|thread| {
                    self.shell
                        .thread(thread.read(cx).thread_id())
                        .is_none_or(|thread| thread.archived_at.is_some())
                }) && self.shell.synchronized
                {
                    if let Some(thread) = &self.thread {
                        let view = thread.read(cx);
                        self.drafts.insert(view.thread_id().to_owned(), view.draft(cx));
                    }
                    self.thread = None;
                    self.backend.send(Command::CloseThread);
                    self._thread_subscription = None;
                    self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(None, cx));
                }
                self.sync_thread_shell(cx);
                let shell = self.shell.clone();
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_shell(shell, cx));
                // The thread this session's `thread.create` was waiting on
                // has streamed in: open it now that the sidebar has it too.
                if self
                    .pending_new_thread_id
                    .as_deref()
                    .is_some_and(|id| self.shell.thread(id).is_some())
                {
                    let thread_id = self.pending_new_thread_id.take().unwrap();
                    self.open_thread(thread_id, window, cx);
                }
            }
            Event::Thread { thread_id, item } => {
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| {
                        // A late item from the previously open thread is stale.
                        if view.thread_id() == thread_id {
                            view.apply(item, window, cx);
                        }
                    });
                }
            }
            Event::Error(message) => self.error = Some(message.into()),
            Event::Config(config) => {
                self.providers = config.providers;
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| view.set_providers(self.providers.clone(), cx));
                }
            }
            Event::ThreadActionFinished { thread_id, action, success } => {
                if let Some(panel) = self.question_panels.get(&thread_id) {
                    panel.update(cx, |panel, cx| panel.response_finished(&action, success, cx));
                }
                if let Some(thread) = &self.thread {
                    if thread.read(cx).thread_id() == thread_id {
                        thread.update(cx, |view, cx| view.update_finished(&action, success, cx));
                    }
                }
            }
            Event::SendFinished { thread_id, text, success } => {
                self.sending.remove(&thread_id);
                if success && self.drafts.get(&thread_id).is_some_and(|draft| draft.trim() == text)
                {
                    self.drafts.remove(&thread_id);
                }
                if let Some(thread) = &self.thread {
                    if thread.read(cx).thread_id() == thread_id {
                        thread
                            .update(cx, |view, cx| view.send_finished(&text, success, window, cx));
                    }
                }
            }
        }
        cx.notify();
    }

    /// Prompts for a folder with the native picker and dispatches
    /// `project.create` for it. The folder's name is the project title, same
    /// default as the web app's "Add project" flow.
    fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Project".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.pop() else { return };
            let title = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "New project".into());
            let workspace_root = path.to_string_lossy().into_owned();
            let _ = this.update(cx, |app, _| {
                app.backend.send(Command::CreateProject {
                    id: t3_client::new_id(),
                    title,
                    workspace_root,
                });
            });
        })
        .detach();
    }

    /// Dispatches `thread.create` for `project`, defaults matching
    /// `ChatView.tsx`'s new-thread flow (see `t3_client::Connection::create_thread`).
    /// The thread opens once its shell entry streams back in.
    fn create_thread_in(
        &mut self,
        project: ProjectShell,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let thread_id = t3_client::new_id();
        let model_selection = project
            .default_model_selection
            .or_else(|| {
                self.open_thread_shell(cx).and_then(|thread| thread.model_selection.clone())
            })
            .or_else(|| {
                self.providers.iter().find_map(|provider| {
                    if !provider.enabled
                        || !provider.installed
                        || provider.availability.as_deref() == Some("unavailable")
                    {
                        return None;
                    }
                    let model = provider
                        .models
                        .iter()
                        .find(|model| model.is_default)
                        .or_else(|| provider.models.first())?;
                    Some(json!({ "instanceId": provider.instance_id, "model": model.id }))
                })
            })
            .unwrap_or_else(fallback_model_selection);
        self.pending_new_thread_id = Some(thread_id.clone());
        self.backend.send(Command::CreateThread {
            id: thread_id,
            project_id: project.id,
            title: "New thread".into(),
            model_selection,
        });
        cx.notify();
    }

    /// Hands the open thread its shell entry: working state and modes.
    fn sync_thread_shell(&self, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else { return };
        let shell = self.shell.thread(thread.read(cx).thread_id()).cloned();
        thread.update(cx, |view, cx| view.set_shell(shell, cx));
    }

    fn open_thread(&mut self, thread_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.thread.as_ref().is_some_and(|t| t.read(cx).thread_id() == thread_id) {
            return;
        }
        if let Some(thread) = &self.thread {
            let view = thread.read(cx);
            self.drafts.insert(view.thread_id().to_owned(), view.draft(cx));
        }
        let connected = matches!(self.status, Status::Connected(_));
        let panel = self
            .question_panels
            .entry(thread_id.clone())
            .or_insert_with(|| cx.new(UserInputPanel::new))
            .clone();
        let view = cx.new(|cx| {
            let mut view = ThreadView::new(thread_id.clone(), panel, window, cx);
            view.set_connected(connected, cx);
            view.set_providers(self.providers.clone(), cx);
            view.restore_draft(
                self.drafts.get(&thread_id).map_or("", String::as_str),
                self.sending.contains(&thread_id),
                window,
                cx,
            );
            view
        });
        self._thread_subscription = Some(cx.subscribe(&view, Self::on_thread_event));
        self.thread = Some(view);
        self.sync_thread_shell(cx);
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(Some(thread_id.clone()), cx));
        self.backend.send(Command::OpenThread(thread_id));
        cx.notify();
    }

    fn on_thread_event(
        &mut self,
        view: Entity<ThreadView>,
        event: &ThreadViewEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.shell.thread(view.read(cx).thread_id()).cloned() else {
            self.error = Some("This thread is no longer available.".into());
            return cx.notify();
        };
        match event {
            ThreadViewEvent::Send(text) => {
                self.sending.insert(thread.id.clone());
                self.backend.send(Command::SendMessage { thread, text: text.clone() });
            }
            ThreadViewEvent::Update(action) => self
                .backend
                .send(Command::ThreadAction { thread_id: thread.id, action: action.clone() }),
            ThreadViewEvent::Stop => {
                let turn_id = thread.session.as_ref().and_then(|s| s.active_turn_id.clone());
                self.backend.send(Command::Interrupt { thread_id: thread.id, turn_id });
            }
        }
    }

    fn pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let link = self.pairing_link.read(cx).value().trim().to_owned();
        if link.is_empty() {
            return;
        }
        self.pairing_link.update(cx, |state, cx| state.set_value("", window, cx));
        self.error = None;
        self.switching_server = false;
        self.thread = None;
        self._thread_subscription = None;
        self.drafts.clear();
        self.question_panels.clear();
        self.sending.clear();
        self.providers.clear();
        self.pending_new_thread_id = None;
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(None, cx));
        self.backend.send(Command::Pair(link));
        cx.notify();
    }

    fn open_thread_shell(&self, cx: &App) -> Option<&ThreadShell> {
        self.shell.thread(self.thread.as_ref()?.read(cx).thread_id())
    }
}

impl Render for T3App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if self.status == Status::NeedsPairing || self.switching_server {
            self.render_pairing(window, cx).into_any_element()
        } else if let Some(thread) = &self.thread {
            // Cached: `T3App` re-renders on plenty of events (shell deltas,
            // status changes) that have nothing to do with this thread. Skip
            // re-rendering the transcript unless the thread view notifies
            // itself (see `ThreadView::apply`/`set_shell`/`set_connected`).
            thread.clone().cached(StyleRefinement::default().size_full()).into_any_element()
        } else {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Select a thread")
                .into_any_element()
        };

        v_flex()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .when(self.sidebar_open, |row| row.child(self.sidebar.clone()))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .children(self.error.clone().map(|error| {
                                div().p_2().child(Alert::error("backend-error", error))
                            }))
                            .child(main),
                    ),
            )
            // Painted last so it stacks above the sidebar and main column.
            .child(self.project_picker.clone())
    }
}

impl T3App {
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let toggle = Button::new("toggle-sidebar")
            .ghost()
            .small()
            .icon(icon(IconName::PanelLeft))
            .tooltip(if self.sidebar_open { "Hide sidebar" } else { "Show sidebar" })
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }));
        let brand = h_flex()
            .gap_2()
            .px_2()
            .h_full()
            .flex_shrink_0()
            .child(toggle)
            .child(div().text_sm().font_semibold().child("T3 Code"))
            .when(self.sidebar_open, |brand| {
                brand
                    .w(SIDEBAR_WIDTH)
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.sidebar_border)
            });

        let shell = self.open_thread_shell(cx);
        let project =
            shell.and_then(|thread| self.shell.projects.iter().find(|p| p.id == thread.project_id));
        let breadcrumb = h_flex()
            .gap_2()
            .px_4()
            .min_w_0()
            .text_sm()
            .when_some(project, |row, project| {
                row.child(ui::project_tag(&project.id, &project.title))
                    .child(div().text_color(theme.muted_foreground).child(project.title.clone()))
                    .child(div().text_color(theme.muted_foreground).child("/"))
            })
            .when_some(shell, |row, thread| {
                row.child(div().min_w_0().truncate().font_semibold().child(thread.title.clone()))
            });

        TitleBar::new().pl_0().child(h_flex().h_full().min_w_0().child(brand).child(breadcrumb))
    }

    fn render_pairing(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().flex().flex_1().items_center().justify_center().p_4().child(
            v_flex()
                .gap_4()
                .w(px(520.))
                .p_6()
                .rounded_xl()
                .border_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .child(
                    h_flex()
                        .gap_2()
                        .child(icon(IconName::Plug).text_color(theme.primary))
                        .child(div().text_lg().font_semibold().child("Connect to a T3 server")),
                )
                .child(div().text_sm().text_color(theme.muted_foreground).child(
                    "Paste the pairing link from `npx t3 serve`, or the token from \
                     `npx t3 auth pairing create`. A token alone connects to \
                     http://localhost:3773; for another server enter `<server url> <token>`.",
                ))
                .child(Input::new(&self.pairing_link))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .when(self.switching_server, |row| {
                            row.child(Button::new("cancel-pair").ghost().label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.switching_server = false;
                                    cx.notify();
                                }),
                            ))
                        })
                        .child(
                            Button::new("pair")
                                .primary()
                                .label("Connect")
                                .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                        ),
                ),
        )
    }
}
