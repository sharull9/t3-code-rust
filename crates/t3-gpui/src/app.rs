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
use t3_client::{ShellState, ThreadShell};

use crate::backend::{Backend, Command, Event, Status};
use crate::sidebar::{Sidebar, SidebarEvent};
use crate::thread_view::{ThreadView, ThreadViewEvent};
use crate::ui::{self, SIDEBAR_WIDTH, icon};

pub struct T3App {
    backend: Backend,
    status: Status,
    error: Option<SharedString>,
    shell: ShellState,
    thread: Option<Entity<ThreadView>>,
    pairing_link: Entity<InputState>,
    sidebar: Entity<Sidebar>,
    sidebar_open: bool,
    /// Pairing screen opened by hand while already paired, to switch servers.
    switching_server: bool,
    _thread_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl T3App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (backend, events) = Backend::spawn();
        Self::listen(events, cx);

        let pairing_link = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Pairing link or token")
        });
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &pairing_link,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.pair(window, cx);
                    }
                },
            ),
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
                }
            }),
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
            switching_server: false,
            _thread_subscription: None,
            _subscriptions: subscriptions,
        }
    }

    fn listen(mut events: UnboundedReceiver<Event>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |app, cx| app.handle_event(event, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: Event, cx: &mut Context<Self>) {
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
                self.sync_thread_shell(cx);
                let shell = self.shell.clone();
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_shell(shell, cx));
            }
            Event::Thread { thread_id, item } => {
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| {
                        // A late item from the previously open thread is stale.
                        if view.thread_id() == thread_id {
                            view.apply(item, cx);
                        }
                    });
                }
            }
            Event::Error(message) => self.error = Some(message.into()),
        }
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
        let connected = matches!(self.status, Status::Connected(_));
        let view = cx.new(|cx| {
            let mut view = ThreadView::new(thread_id.clone(), window, cx);
            view.set_connected(connected, cx);
            view
        });
        self._thread_subscription = Some(cx.subscribe(&view, Self::on_thread_event));
        self.thread = Some(view);
        self.sync_thread_shell(cx);
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_open_thread(Some(thread_id.clone()), cx)
        });
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
            ThreadViewEvent::Send(text) => self.backend.send(Command::SendMessage {
                thread,
                text: text.clone(),
            }),
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
                brand.w(SIDEBAR_WIDTH).bg(theme.sidebar).border_r_1().border_color(theme.sidebar_border)
            });

        let shell = self.open_thread_shell(cx);
        let project = shell.and_then(|thread| {
            self.shell.projects.iter().find(|p| p.id == thread.project_id)
        });
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

        TitleBar::new()
            .pl_0()
            .child(h_flex().h_full().min_w_0().child(brand).child(breadcrumb))
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
