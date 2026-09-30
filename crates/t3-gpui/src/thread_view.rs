//! One open thread: transcript, working state and composer.
//!
//! The view owns connection/shell state and the composer; the transcript
//! itself (rows, scroll position) lives in its own entity, [`Transcript`] —
//! see `transcript.rs` for why. Sending and stopping are emitted as events;
//! the app turns them into backend commands.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, Icon, Size, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{Session, ThreadShell, ThreadStreamItem};

use crate::transcript::Transcript;
use crate::ui::{self, CONTENT_WIDTH};

pub enum ThreadViewEvent {
    Send(String),
    Stop,
}

pub struct ThreadView {
    thread_id: String,
    transcript: Entity<Transcript>,
    composer: Entity<TextareaState>,
    /// The thread's shell entry. It carries the modes, and can report a turn
    /// before detail catches up.
    shell: Option<ThreadShell>,
    connected: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

impl ThreadView {
    pub fn new(thread_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let transcript = cx.new(Transcript::new);
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .submit_on_enter(true)
                .placeholder("Ask anything  (Shift+Enter for a new line)")
        });
        let subscriptions = vec![
            cx.subscribe_in(&composer, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.submit(window, cx);
                }
            }),
        ];
        composer.update(cx, |state, cx| state.focus(window, cx));

        Self {
            thread_id,
            transcript,
            composer,
            shell: None,
            connected: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Shell-stream updates arrive frequently while an agent works, but most
    /// carry no change to anything this view renders (e.g. a bare
    /// `updated_at` bump). `ThreadShell` doesn't derive `PartialEq` because
    /// it also carries fields this view never reads, so compare only the
    /// fields `render` and `is_working` actually use before notifying.
    pub fn set_shell(&mut self, shell: Option<ThreadShell>, cx: &mut Context<Self>) {
        if !shell_render_state_eq(self.shell.as_ref(), shell.as_ref()) {
            cx.notify();
        }
        self.shell = shell;
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.transcript.update(cx, |transcript, cx| transcript.reset(cx));
    }

    pub fn apply(&mut self, item: ThreadStreamItem, cx: &mut Context<Self>) {
        self.transcript.update(cx, |transcript, cx| transcript.apply(item, cx));
    }

    fn is_working(&self, cx: &App) -> bool {
        let shell_session = self.shell.as_ref().and_then(|t| t.session.as_ref());
        let detail_session = self.transcript.read(cx).session();
        shell_session.is_some_and(|s| s.is_working())
            || detail_session.is_some_and(|s| s.is_working())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_owned();
        if text.is_empty() || !self.connected {
            return;
        }
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        cx.emit(ThreadViewEvent::Send(text));
    }
}

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let working = self.is_working(cx);
        let transcript_state = self.transcript.read(cx);
        let shell = self.shell.as_ref();
        let branch = transcript_state
            .branch()
            .map(str::to_owned)
            .or_else(|| shell.and_then(|t| t.branch.clone()));
        let session = transcript_state.session().or_else(|| shell.and_then(|t| t.session.as_ref()));
        let session_error = session.and_then(|s| s.last_error.clone());
        let provider = ui::provider_label(session.and_then(|s| s.provider_name.as_deref()));
        let (runtime_label, runtime_icon) =
            ui::runtime_mode(shell.map_or("", |t| t.runtime_mode.as_str()));
        let (mode_label, mode_icon) =
            ui::interaction_mode(shell.map_or("", |t| t.interaction_mode.as_str()));

        // `Transcript` is embedded cached and styled to fill the remaining
        // column; it repaints only when it notifies itself (new stream
        // items, scrolling), never because this view redraws for an
        // unrelated reason such as the composer's loader animation below
        // (see `transcript.rs`).
        let transcript =
            self.transcript.clone().cached(StyleRefinement::default().flex_1().min_h_0());

        let action = div()
            .id(if working { "stop" } else { "send" })
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size_8()
            .rounded_full()
            .text_color(theme.primary_foreground)
            .map(|button| {
                if working {
                    button
                        .bg(theme.danger)
                        .child(Icon::new(IconName::Square).xsmall())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.connected {
                                cx.emit(ThreadViewEvent::Stop);
                            }
                        }))
                } else {
                    button
                        .bg(theme.primary)
                        .hover(|style| style.bg(theme.primary_hover))
                        .child(Icon::new(IconName::ArrowUp).small())
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
                }
            })
            .when(!self.connected, |button| button.opacity(0.4));

        let composer = v_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .child(Textarea::new(&self.composer).appearance(false)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .child(chip(IconName::Bot, provider, cx))
                    .child(separator(cx))
                    .child(chip(runtime_icon, runtime_label, cx))
                    .child(separator(cx))
                    .child(chip(mode_icon, mode_label, cx))
                    .child(div().flex_1())
                    .when(working, |row| {
                        row.child(div().mr_1().child(ui::loader("composer-working", Size::Small)))
                    })
                    .child(action),
            );

        let footer = h_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .gap_1()
            .px_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::FolderClosed).xsmall())
            .child("Local checkout")
            .child(div().flex_1())
            .children(branch.map(|branch| {
                h_flex().gap_1().child(Icon::new(IconName::GitBranch).xsmall()).child(branch)
            }));

        v_flex()
            .size_full()
            .min_h_0()
            .child(transcript)
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .px_6()
                    .pt_2()
                    .pb_3()
                    .children(session_error.map(|error| {
                        div()
                            .w_full()
                            .max_w(CONTENT_WIDTH)
                            .px_1()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(error)
                    }))
                    .child(composer)
                    .child(footer),
            )
    }
}

/// A read-only setting in the composer footer.
fn chip(icon: IconName, label: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .px_2()
        .py_1()
        .text_xs()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).xsmall())
        .child(label.into())
}

fn separator(cx: &App) -> impl IntoElement {
    div().w_px().h_4().bg(cx.theme().border)
}

/// Compares the [`ThreadShell`] fields this view actually renders
/// (`render`'s branch/provider/runtime/mode/session-error chips and
/// `is_working`'s working spinner), so `set_shell` can skip `cx.notify()`
/// when a shell-stream update changes nothing on screen.
fn shell_render_state_eq(a: Option<&ThreadShell>, b: Option<&ThreadShell>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.runtime_mode == b.runtime_mode
                && a.interaction_mode == b.interaction_mode
                && a.branch == b.branch
                && session_render_state_eq(a.session.as_ref(), b.session.as_ref())
        }
        _ => false,
    }
}

/// The [`Session`] half of [`shell_render_state_eq`].
fn session_render_state_eq(a: Option<&Session>, b: Option<&Session>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.is_working() == b.is_working()
                && a.provider_name == b.provider_name
                && a.last_error == b.last_error
        }
        _ => false,
    }
}
