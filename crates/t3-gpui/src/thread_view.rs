//! One open thread: transcript, working state and composer.
//!
//! The view owns the thread projection and keeps `MessageScrollerState`'s row
//! count aligned with it. Sending and stopping are emitted as events; the app
//! turns them into backend commands.

use std::rc::Rc;

use gpui_kit::component::bubble::Bubble;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::message::{Message, MessageAlignment, MessageContent, MessageHeader};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{MessageRole, ThreadDetail, ThreadState, ThreadStreamItem};

pub enum ThreadViewEvent {
    Send(String),
    Stop,
}

pub struct ThreadView {
    thread_id: String,
    state: ThreadState,
    /// Snapshot of the transcript handed to the virtualized row renderer.
    rows: Rc<Vec<t3_client::Message>>,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<TextareaState>,
    /// From the shell stream, which can report a turn before detail catches up.
    shell_working: bool,
    connected: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

impl ThreadView {
    pub fn new(thread_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder("Message the agent  (Shift+Enter for a new line)")
        });
        let subscriptions = vec![
            cx.observe(&scroller, |_, _, cx| cx.notify()),
            cx.subscribe_in(&composer, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.submit(window, cx);
                }
            }),
        ];
        composer.update(cx, |state, cx| state.focus(window, cx));

        Self {
            thread_id,
            state: ThreadState::default(),
            rows: Rc::default(),
            scroller,
            composer,
            shell_working: false,
            connected: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn set_shell_working(&mut self, working: bool, cx: &mut Context<Self>) {
        if self.shell_working != working {
            self.shell_working = working;
            cx.notify();
        }
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.state = ThreadState::default();
        self.rows = Rc::default();
        self.scroller.update(cx, |scroller, cx| scroller.reset(0, cx));
        cx.notify();
    }

    pub fn apply(&mut self, item: ThreadStreamItem, cx: &mut Context<Self>) {
        let replaced = matches!(item, ThreadStreamItem::Snapshot { .. });
        self.state.apply(item);

        let previous = std::mem::take(&mut self.rows);
        let current = self.state.thread.as_ref().map(|t| t.messages.clone()).unwrap_or_default();
        let changed = first_changed_row(&previous, &current);
        let (old_len, new_len) = (previous.len(), current.len());
        self.rows = Rc::new(current);

        self.scroller.update(cx, |scroller, cx| {
            if replaced || new_len < old_len {
                scroller.reset(new_len, cx);
                return;
            }
            if new_len > old_len {
                scroller.append(new_len - old_len, cx);
            }
            if let Some(first) = changed.filter(|&index| index < old_len) {
                scroller.remeasure_items(first..old_len, cx);
            }
        });
        cx.notify();
    }

    fn detail(&self) -> Option<&ThreadDetail> {
        self.state.thread.as_ref()
    }

    fn is_working(&self) -> bool {
        self.shell_working
            || self
                .detail()
                .and_then(|t| t.session.as_ref())
                .is_some_and(|s| s.is_working())
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
        let working = self.is_working();
        let detail = self.detail();
        let title = detail.map(|t| t.title.clone()).unwrap_or_default();
        let branch = detail.and_then(|t| t.branch.clone());
        let session_error = detail
            .and_then(|t| t.session.as_ref())
            .and_then(|s| s.last_error.clone());

        let header = h_flex()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().min_w_0().truncate().font_semibold().child(title))
            .when(working, |row| {
                row.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(Spinner::new().small())
                        .child("Working"),
                )
            })
            .children(branch.map(|branch| {
                div().text_xs().text_color(theme.muted_foreground).child(branch)
            }));

        let transcript = if detail.is_none() {
            centered(h_flex().gap_2().child(Spinner::new()).child("Loading thread"), cx)
                .into_any_element()
        } else if self.rows.is_empty() {
            centered("No messages yet. Say something to start a turn.", cx).into_any_element()
        } else {
            let rows = self.rows.clone();
            MessageScroller::new("transcript", self.scroller.clone(), move |index, _, cx| {
                match rows.get(index) {
                    Some(message) => render_message(message, cx).into_any_element(),
                    None => div().into_any_element(),
                }
            })
            .with_bottom_fade(theme.background)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };

        let action = if working {
            Button::new("stop")
                .danger()
                .icon(IconName::CircleX)
                .label("Stop")
                .disabled(!self.connected)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadViewEvent::Stop)))
        } else {
            Button::new("send")
                .primary()
                .icon(IconName::ArrowUp)
                .label("Send")
                .disabled(!self.connected)
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
        };

        v_flex()
            .size_full()
            .min_h_0()
            .child(header)
            .child(transcript)
            .children(session_error.map(|error| {
                div().px_4().py_2().text_sm().text_color(theme.danger).child(error)
            }))
            .child(
                h_flex()
                    .items_end()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().flex_1().min_w_0().child(Textarea::new(&self.composer)))
                    .child(action),
            )
    }
}

fn render_message(message: &t3_client::Message, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let id = SharedString::from(format!("message-{}", message.id));
    let streaming = message.streaming.then(|| Spinner::new().xsmall());

    match message.role {
        MessageRole::User => Message::new()
            .id(id)
            .alignment(MessageAlignment::End)
            .content(MessageContent::new().bubble(Bubble::new().child(message.text.clone())))
            .into_any_element(),
        MessageRole::Assistant => Message::new()
            .id(id.clone())
            .header(MessageHeader::new().child("Agent").children(streaming))
            .content(MessageContent::new().child(TextView::markdown(id, message.text.clone())))
            .into_any_element(),
        MessageRole::Reasoning => Message::new()
            .id(id)
            .header(MessageHeader::new().child("Thinking").children(streaming))
            .content(
                MessageContent::new()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(message.text.clone()),
            )
            .into_any_element(),
        MessageRole::System | MessageRole::Unknown => Message::new()
            .id(id)
            .content(
                MessageContent::new()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(message.text.clone()),
            )
            .into_any_element(),
    }
}

fn centered(content: impl IntoElement, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(content)
}

/// Index of the first row whose rendered content differs, if any.
fn first_changed_row(old: &[t3_client::Message], new: &[t3_client::Message]) -> Option<usize> {
    old.iter().zip(new).position(|(a, b)| {
        a.id != b.id || a.streaming != b.streaming || a.text.len() != b.text.len()
    })
}
