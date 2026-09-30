//! Window root: connection status, thread navigation and the open thread.

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{SessionStatus, ShellState, ThreadShell};

use crate::backend::{Backend, Command, Event, Status};
use crate::thread_view::{ThreadView, ThreadViewEvent};

pub struct T3App {
    backend: Backend,
    status: Status,
    error: Option<SharedString>,
    shell: ShellState,
    thread: Option<Entity<ThreadView>>,
    pairing_link: Entity<InputState>,
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
        let subscriptions = vec![cx.subscribe_in(
            &pairing_link,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.pair(window, cx);
                }
            },
        )];

        Self {
            backend,
            status: Status::Connecting(String::new()),
            error: None,
            shell: ShellState::default(),
            thread: None,
            pairing_link,
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
                }
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| {
                        if connected {
                            view.reset(cx);
                        }
                        view.set_connected(connected, cx);
                    });
                }
                self.status = status;
            }
            Event::Shell(item) => {
                self.shell.apply(item);
                self.sync_thread_working(cx);
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

    fn sync_thread_working(&self, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else { return };
        let working = self
            .shell
            .thread(thread.read(cx).thread_id())
            .and_then(|t| t.session.as_ref())
            .is_some_and(|s| s.is_working());
        thread.update(cx, |view, cx| view.set_shell_working(working, cx));
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
        self.sync_thread_working(cx);
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
}

impl Render for T3App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if self.status == Status::NeedsPairing || self.switching_server {
            self.render_pairing(window, cx).into_any_element()
        } else if let Some(thread) = &self.thread {
            thread.clone().into_any_element()
        } else {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Select a thread")
                .into_any_element()
        };

        h_flex()
            .items_stretch()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(self.error.clone().map(|error| {
                        div().p_2().child(Alert::error("backend-error", error))
                    }))
                    .child(main),
            )
    }
}

impl T3App {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open_id = self.thread.as_ref().map(|t| t.read(cx).thread_id().to_owned());

        let groups = self.shell.projects.iter().map(|project| {
            let items = self.shell.project_threads(&project.id).into_iter().map(|thread| {
                let thread_id = thread.id.clone();
                let badge = thread_badge(thread);
                SidebarMenuItem::new(thread.title.clone())
                    .active(open_id.as_deref() == Some(thread.id.as_str()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_thread(thread_id.clone(), window, cx)
                    }))
                    .when_some(badge, |item, badge| {
                        item.suffix(move |_, cx| {
                            div().text_xs().text_color(badge.color(cx)).child(badge.label())
                        })
                    })
            });
            SidebarGroup::new(project.title.clone()).child(SidebarMenu::new().children(items))
        });

        let (label, color) = match &self.status {
            Status::NeedsPairing => ("Not paired".to_owned(), cx.theme().muted_foreground),
            Status::Connecting(_) => ("Connecting…".to_owned(), cx.theme().warning),
            Status::Connected(server) => (server.clone(), cx.theme().success),
            Status::Reconnecting { reason, .. } => {
                (format!("Reconnecting: {reason}"), cx.theme().danger)
            }
        };

        Sidebar::new("threads")
            .header(SidebarHeader::new().child(div().font_semibold().child("T3 Code")))
            .children(groups)
            .footer(
                SidebarFooter::new().child(
                    v_flex()
                        .gap_1()
                        .w_full()
                        .min_w_0()
                        .child(
                            h_flex()
                                .gap_2()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(div().size_2().flex_shrink_0().rounded_full().bg(color))
                                .child(div().min_w_0().truncate().child(label)),
                        )
                        .when(self.status != Status::NeedsPairing, |footer| {
                            footer.child(
                                Button::new("switch-server")
                                    .ghost()
                                    .xsmall()
                                    .label("Switch server")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.switching_server = true;
                                        this.pairing_link
                                            .update(cx, |state, cx| state.focus(window, cx));
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
            )
    }

    fn render_pairing(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_1().items_center().justify_center().child(
            v_flex()
                .gap_3()
                .w(px(520.))
                .child(div().text_xl().font_semibold().child("Connect to a T3 server"))
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                    "Paste the pairing link from `npx t3 serve`, or the token from \
                     `npx t3 auth pairing create`. A token alone connects to \
                     http://localhost:3773; for another server enter `<server url> <token>`.",
                ))
                .child(Input::new(&self.pairing_link))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("pair")
                                .primary()
                                .label("Connect")
                                .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                        )
                        .when(self.switching_server, |row| {
                            row.child(Button::new("cancel-pair").ghost().label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.switching_server = false;
                                    cx.notify();
                                }),
                            ))
                        }),
                ),
        )
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
            Badge::NeedsInput => "needs you",
            Badge::Working => "working",
            Badge::Failed => "error",
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
