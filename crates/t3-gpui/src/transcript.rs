//! The message transcript: a scrollable column of user/assistant/thinking rows.
//!
//! Split out of `ThreadView` into its own entity so that `ThreadView`'s
//! continuously-animating children (the composer's working indicator) don't
//! dirty the transcript on every frame. `Window::mark_view_dirty` (in
//! `gpui-pre`'s `src/window.rs`) walks UP from a notified view to mark its
//! ancestors dirty; it never reaches down into descendants. `ThreadView`
//! embeds this view as a cached child (`AnyView::cached`, see `thread_view.rs`
//! and `gpui-pre`'s `src/view.rs`), so a cached, non-dirty `Transcript`
//! reuses last frame's paint even while `ThreadView` itself redraws every
//! frame because of the composer's `request_animation_frame` loader.
//! `Transcript` therefore must never host anything that animates
//! continuously; it should only redraw when it notifies itself, i.e. on a new
//! stream item or a scroll tick.

use std::rc::Rc;

use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Size, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{MessageRole, Session, ThreadState, ThreadStreamItem};

use crate::ui::{self, CONTENT_WIDTH};

pub struct Transcript {
    state: ThreadState,
    /// Snapshot of the transcript handed to the virtualized row renderer.
    rows: Rc<Vec<Row>>,
    scroller: Entity<MessageScrollerState>,
}

impl Transcript {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // No `cx.observe(&scroller, ...)` here: GPUI's `list` element already
        // calls `cx.notify(current_view)` on every scroll tick from inside
        // its own scroll handler (`gpui-pre`'s `elements/list.rs`), where
        // `current_view` is whichever entity is currently being painted —
        // this view, since `MessageScroller` is embedded directly rather
        // than as its own entity. Observing the scroller's state as well
        // just schedules a second, deferred `cx.notify()` for the same
        // scroll tick, doubling this view's re-render rate while scrolling.
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        Self { state: ThreadState::default(), rows: Rc::default(), scroller }
    }

    /// The session carried by the last applied thread detail, if any. Used
    /// by `ThreadView` as the fallback source for `is_working` and the
    /// composer footer once the shell-stream's optimistic copy is caught up.
    pub fn session(&self) -> Option<&Session> {
        self.state.thread.as_ref().and_then(|t| t.session.as_ref())
    }

    /// The branch carried by the last applied thread detail, if any.
    pub fn branch(&self) -> Option<&str> {
        self.state.thread.as_ref().and_then(|t| t.branch.as_deref())
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

        let old_len = self.rows.len();
        let messages = self.state.thread.as_ref().map(|t| t.messages.as_slice()).unwrap_or(&[]);
        let new_len = messages.len();

        // Mutate the row vector in place instead of deep-cloning it (every
        // streamed token used to `.clone()` the whole `Vec<Message>`,
        // including every untouched message's `String`). `Rc::make_mut`
        // clones only if a render pass still holds a reference to the
        // previous vector; by the time a stream event lands here the last
        // rendered element tree has already been painted and dropped, so in
        // the steady state this mutates the existing allocation instead.
        //
        // Rows whose id, streaming flag, and text length all still match are
        // left untouched so their `SharedString` keeps the same allocation
        // across frames (see `Row::from`), which is what lets `TextView`
        // skip reparsing their Markdown.
        let rows = Rc::make_mut(&mut self.rows);
        let mut changed = None;
        for (index, message) in messages.iter().enumerate() {
            let unchanged = rows.get(index).is_some_and(|row| {
                row.id == message.id
                    && row.streaming == message.streaming
                    && row.text.len() == message.text.len()
            });
            if unchanged {
                continue;
            }
            changed.get_or_insert(index);
            match rows.get_mut(index) {
                Some(slot) => *slot = Row::from(message),
                None => rows.push(Row::from(message)),
            }
        }
        rows.truncate(new_len);

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
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if self.state.thread.is_none() {
            return centered(
                h_flex()
                    .gap_2()
                    .child(ui::loader("loading-thread", Size::Small))
                    .child("Loading thread"),
                cx,
            )
            .into_any_element();
        }
        if self.rows.is_empty() {
            return centered("No messages yet. Say something to start a turn.", cx)
                .into_any_element();
        }

        let rows = self.rows.clone();
        MessageScroller::new("transcript", self.scroller.clone(), move |index, _, cx| {
            match rows.get(index) {
                Some(message) => column(render_message(message, cx))
                    .when(index == 0, |row| row.pt_6())
                    .into_any_element(),
                None => div().into_any_element(),
            }
        })
        .with_bottom_fade(theme.background)
        .size_full()
        .into_any_element()
    }
}

/// Centers content in the full space this view is given. `ThreadView`
/// embeds `Transcript` as a cached view styled to fill the remaining column
/// (`flex_1`/`min_h_0`), so `Transcript`'s own root is laid out with definite
/// bounds and can simply fill them, mirroring how `ThreadView` fills the
/// bounds `T3App` gives it (see `app.rs`).
fn centered(content: impl IntoElement, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(content)
}

/// Centers a transcript row in the content column.
fn column(content: impl IntoElement) -> Div {
    div()
        .flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(div().w_full().max_w(CONTENT_WIDTH).px_1().py_2().child(content))
}

fn render_message(message: &Row, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let id = SharedString::from(format!("message-{}", message.id));

    match message.role {
        MessageRole::User => h_flex()
            .justify_end()
            .child(
                div()
                    .max_w(relative(0.8))
                    .px_4()
                    .py_2p5()
                    .rounded_xl()
                    .bg(theme.secondary_hover)
                    .text_sm()
                    .child(message.text.clone()),
            )
            .into_any_element(),
        MessageRole::Assistant => v_flex()
            .gap_2()
            .text_sm()
            .child(TextView::markdown(id, message.text.clone()))
            .into_any_element(),
        MessageRole::Reasoning => v_flex()
            .gap_1()
            .pl_3()
            .border_l_2()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(h_flex().gap_2().text_xs().font_medium().child("Thinking"))
            .child(message.text.trim().to_owned())
            .into_any_element(),
        MessageRole::System | MessageRole::Unknown => div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(message.text.clone())
            .into_any_element(),
    }
}

/// A transcript row: a [`t3_client::Message`] with its text pre-converted to
/// a [`SharedString`].
///
/// `TextView::markdown` (via `gpui-base`'s `TextViewState::set_element_text`)
/// recognizes unchanged text by comparing the incoming string's allocation
/// pointer to the one it parsed last frame, not its content — and the
/// virtualized list re-invokes every visible row's renderer on every frame it
/// repaints (`gpui-pre`'s `elements/list.rs`), not only when the row's data
/// changes. `Message::text` is a plain `String`, so converting it with
/// `.clone().into()` inside the row renderer would hand `TextView` a fresh
/// allocation every frame and force it to reparse the Markdown of every
/// visible message on every frame, scrolling or not. Converting once here and
/// reusing the same `SharedString` (via `Clone`, an `Arc` bump) across
/// frames — as long as `apply` finds the row unchanged — keeps that pointer
/// stable.
#[derive(Clone)]
struct Row {
    id: String,
    role: MessageRole,
    text: SharedString,
    streaming: bool,
}

impl From<&t3_client::Message> for Row {
    fn from(message: &t3_client::Message) -> Self {
        Self {
            id: message.id.clone(),
            role: message.role,
            text: SharedString::from(message.text.clone()),
            streaming: message.streaming,
        }
    }
}
