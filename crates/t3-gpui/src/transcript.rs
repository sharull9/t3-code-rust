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
//! stream item, a row being expanded/collapsed, a scroll tick, or the slow
//! (1s) "Working for..." ticker below.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use t3_client::attachments::UploadedAttachment;
use t3_client::{
    Activity, ActivityTone, Message, MessageRole, Session, ThreadState, ThreadStreamItem,
};

use crate::ui::{self, CONTENT_WIDTH};

pub struct Transcript {
    state: ThreadState,
    /// Row keys the reader has expanded: a "Thought" message's id, or a tool
    /// group's key (its first activity's id). Collapsed (absent) by default,
    /// like the T3 web app.
    expanded: HashSet<String>,
    /// Tool details expand independently inside their activity group.
    expanded_tools: HashSet<String>,
    /// Snapshot of the transcript handed to the virtualized row renderer.
    rows: Rc<Vec<Row>>,
    scroller: Entity<MessageScrollerState>,
    /// Wall-clock start of the current unbroken working streak, for the
    /// "Working for Xm Ys" row. `None` while idle. Reading it never triggers
    /// a redraw by itself; `render` picks up the latest value whenever
    /// something else (a stream item, a toggle, or the slow ticker spawned in
    /// `new`) notifies this view.
    working_since: Option<Instant>,
}

pub enum TranscriptEvent {
    OpenAttachment(UploadedAttachment),
}

impl EventEmitter<TranscriptEvent> for Transcript {}

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

        // A "Working for..." row reads elapsed time at render time from
        // `working_since`, so it only needs a slow, coarse-grained nudge to
        // stay roughly live -- one `cx.notify()` a second, never a frame
        // loop. This is the one exception to "never animate" in the module
        // doc: it only repaints the single text row, not every frame, and
        // only while a turn is actually running.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |this, cx| {
                        if this.working_since.is_some() {
                            cx.notify()
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();

        Self {
            state: ThreadState::default(),
            expanded: HashSet::new(),
            expanded_tools: HashSet::new(),
            rows: Rc::default(),
            scroller,
            working_since: None,
        }
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

    pub fn approvals(&self) -> Vec<t3_client::pending::PendingApproval> {
        self.state
            .thread
            .as_ref()
            .map(|thread| t3_client::pending::approvals(&thread.activities))
            .unwrap_or_default()
    }

    pub fn user_inputs(&self) -> Option<Vec<t3_client::pending::PendingUserInput>> {
        self.state.thread.as_ref().map(|thread| t3_client::pending::user_inputs(&thread.activities))
    }

    fn is_working(&self) -> bool {
        self.session().is_some_and(Session::is_working)
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    /// Whether the loaded thread has any messages yet.
    pub fn has_messages(&self) -> bool {
        self.state.thread.as_ref().is_some_and(|thread| !thread.messages.is_empty())
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.state = ThreadState::default();
        self.rows = Rc::default();
        self.expanded.clear();
        self.expanded_tools.clear();
        self.working_since = None;
        self.scroller.update(cx, |scroller, cx| scroller.reset(0, cx));
        cx.notify();
    }

    pub fn apply(&mut self, item: ThreadStreamItem, cx: &mut Context<Self>) {
        let replaced = matches!(item, ThreadStreamItem::Snapshot { .. });
        self.state.apply(item);

        let is_working = self.is_working();
        match (self.working_since.is_some(), is_working) {
            (false, true) => self.working_since = Some(Instant::now()),
            (true, false) => self.working_since = None,
            _ => {}
        }

        self.rebuild(replaced, cx);
    }

    /// Recomputes `rows` from the current state and diffs it against the
    /// previous snapshot to tell the scroller what changed. Shared by
    /// `apply` (new stream data) and `toggle_row` (an expand/collapse that
    /// changes nothing but one row's own height).
    fn rebuild(&mut self, replaced: bool, cx: &mut Context<Self>) {
        let old_len = self.rows.len();
        let thread = self.state.thread.as_ref();
        let messages = thread.map(|t| t.messages.as_slice()).unwrap_or(&[]);
        let activities = thread.map(|t| t.activities.as_slice()).unwrap_or(&[]);

        // Mutate the row vector in place instead of deep-cloning it (every
        // streamed token used to `.clone()` the whole `Vec<Message>`,
        // including every untouched message's `String`). `Rc::make_mut`
        // clones only if a render pass still holds a reference to the
        // previous vector; by the time a stream event lands here the last
        // rendered element tree has already been painted and dropped, so in
        // the steady state this mutates the existing allocation instead.
        let old_rows = self.rows.clone();
        let (mut new_rows, first_changed) = build_rows(messages, activities, &old_rows);
        if self.working_since.is_some() {
            new_rows.push(Row::Working);
        }
        let new_len = new_rows.len();

        *Rc::make_mut(&mut self.rows) = new_rows;

        self.scroller.update(cx, |scroller, cx| {
            if replaced || new_len < old_len {
                scroller.reset(new_len, cx);
                return;
            }
            if new_len > old_len {
                scroller.append(new_len - old_len, cx);
            }
            if let Some(first) = first_changed.filter(|&index| index < old_len) {
                scroller.remeasure_items(first..old_len, cx);
            }
        });
        cx.notify();
    }

    /// Flips a row's expand state and asks the scroller to remeasure only
    /// that one row -- expanding a "Thought" or tool group changes how tall
    /// its own row paints, not how many rows there are.
    fn toggle_row(&mut self, key: &str, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        self.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(index..index + 1, cx);
        });
        cx.notify();
    }

    fn toggle_tool(&mut self, key: &str, index: usize, cx: &mut Context<Self>) {
        if !self.expanded_tools.remove(key) {
            self.expanded_tools.insert(key.to_owned());
        }
        self.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(index..index + 1, cx);
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
        let expanded = self.expanded.clone();
        let expanded_tools = self.expanded_tools.clone();
        let entity = cx.entity();
        let working_since = self.working_since;
        MessageScroller::new("transcript", self.scroller.clone(), move |index, _, cx| {
            match rows.get(index) {
                Some(row) => column(render_row(
                    row,
                    index,
                    &expanded,
                    &expanded_tools,
                    &entity,
                    working_since,
                    cx,
                ))
                .when(index == 0, |row| row.pt_2())
                .into_any_element(),
                None => div().into_any_element(),
            }
        })
        .with_row_style(StyleRefinement::default().pb(px(6.)))
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
    h_flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(v_flex().w_full().max_w(CONTENT_WIDTH).px_1().py_1().child(content))
}

fn render_row(
    row: &Row,
    index: usize,
    expanded: &HashSet<String>,
    expanded_tools: &HashSet<String>,
    entity: &Entity<Transcript>,
    working_since: Option<Instant>,
    cx: &App,
) -> AnyElement {
    match row {
        Row::Message(message) => render_message(message, entity, cx).into_any_element(),
        Row::Thought(thought) => {
            let is_expanded = expanded.contains(&thought.id);
            render_thought(thought, is_expanded, index, entity, cx).into_any_element()
        }
        Row::Activities(group) => {
            let is_expanded = expanded.contains(&group.key);
            render_activity_group(group, is_expanded, expanded_tools, index, entity, cx)
                .into_any_element()
        }
        Row::Working => render_working(working_since, cx).into_any_element(),
    }
}

fn render_message(message: &MessageRow, entity: &Entity<Transcript>, cx: &App) -> impl IntoElement {
    let id = SharedString::from(format!("message-{}", message.id));
    let copy = copy_button(message);
    let attachments = attachment_chips(&message.attachments, &message.id, entity, cx);

    match message.role {
        MessageRole::User => h_flex()
            .group(MESSAGE_GROUP)
            .justify_end()
            .child(
                v_flex()
                    .max_w(relative(0.8))
                    .items_end()
                    .gap_1()
                    .child(
                        div()
                            .px_4()
                            .py_2p5()
                            .rounded_xl()
                            .bg(cx.theme().secondary_hover)
                            .text_sm()
                            .child(message.text.clone()),
                    )
                    .when(!message.attachments.is_empty(), |column| column.child(attachments))
                    .child(h_flex().justify_end().child(copy)),
            )
            .into_any_element(),
        MessageRole::Assistant => v_flex()
            .group(MESSAGE_GROUP)
            .gap_1()
            .text_sm()
            .child(TextView::markdown(id, message.text.clone()))
            .when(!message.attachments.is_empty(), |column| column.child(attachments))
            .child(h_flex().justify_end().child(copy))
            .into_any_element(),
        MessageRole::System | MessageRole::Unknown | MessageRole::Reasoning => v_flex()
            .group(MESSAGE_GROUP)
            .gap_1()
            .child(
                div().text_xs().text_color(cx.theme().muted_foreground).child(message.text.clone()),
            )
            .when(!message.attachments.is_empty(), |column| column.child(attachments))
            .child(h_flex().justify_end().child(copy))
            .into_any_element(),
    }
}

fn copy_button(message: &MessageRow) -> impl IntoElement {
    copy_text_button(&message.id, message.text.to_string())
}

/// Copies on click and briefly shows a check. Hidden until the pointer is
/// over its row (the `MESSAGE_GROUP` hover group), so a long transcript
/// isn't striped with copy icons.
fn copy_text_button(message_id: &str, text: String) -> impl IntoElement {
    div().opacity(0.).group_hover(MESSAGE_GROUP, |style| style.opacity(1.)).child(
        Clipboard::new(SharedString::from(format!("copy-message-{message_id}")))
            .value(text)
            .tooltip("Copy message")
            .small(),
    )
}

/// Hover group for a transcript row, revealing its copy control.
const MESSAGE_GROUP: &str = "transcript-message";

fn attachment_chips(
    attachments: &[UploadedAttachment],
    message_id: &str,
    entity: &Entity<Transcript>,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let entity = entity.clone();
    h_flex().flex_wrap().gap_1().children(attachments.iter().map(|attachment| {
        let value = attachment.clone();
        let entity = entity.clone();
        let attachment_id = attachment.id.clone();
        let label = format!("{} · {}", attachment.name, format_size(attachment.size_bytes));
        Button::new(format!("open-attachment-{message_id}-{attachment_id}"))
            .ghost()
            .small()
            .label(label)
            .tooltip(format!("Open {}", attachment.name))
            .text_color(theme.muted_foreground)
            .on_click(move |_, _, cx| {
                entity.update(cx, |_, cx| {
                    cx.emit(TranscriptEvent::OpenAttachment(value.clone()));
                });
            })
    }))
}

fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KB", bytes as f64 / KIB as f64)
    } else if bytes < KIB * KIB * KIB {
        format!("{:.1} MB", bytes as f64 / (KIB * KIB) as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / (KIB * KIB * KIB) as f64)
    }
}

/// A collapsible "Thought" row: a brain icon and chevron header, the
/// reasoning text only when expanded. Collapsed by default, like the T3 web
/// app's trace view.
fn render_thought(
    thought: &ThoughtRow,
    expanded: bool,
    index: usize,
    entity: &Entity<Transcript>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let key = thought.id.clone();
    let entity = entity.clone();
    let chevron = if expanded { IconName::ChevronDown } else { IconName::ChevronRight };
    let label = if thought.streaming { "Thinking" } else { "Thought" };
    let attachments = attachment_chips(&thought.attachments, &thought.id, &entity, cx);

    let header = h_flex()
        .id(("thought-toggle", index))
        .gap_2()
        .items_center()
        .cursor_pointer()
        .text_xs()
        .font_medium()
        .text_color(theme.muted_foreground)
        .child(ui::icon(IconName::Brain).xsmall())
        .child(label)
        .child(ui::icon(chevron).xsmall())
        .on_click(move |_, _, cx| {
            entity.update(cx, |transcript, cx| transcript.toggle_row(&key, index, cx));
        });

    v_flex()
        .gap_1p5()
        .pl_3()
        .border_l_2()
        .border_color(theme.border)
        .child(header)
        .when(!thought.attachments.is_empty(), |column| column.child(attachments))
        .when(expanded, |column| {
            column.child(
                v_flex()
                    .group(MESSAGE_GROUP)
                    .gap_1()
                    .child(
                        div().text_sm().text_color(theme.muted_foreground).child(
                            TextView::markdown(
                                SharedString::from(format!("thought-{}", thought.id)),
                                tidy_reasoning(&thought.text),
                            ),
                        ),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .child(copy_text_button(&thought.id, thought.text.to_string())),
                    ),
            )
        })
}

/// Reasoning streams arrive with padding: leading/trailing whitespace and
/// runs of blank lines between summary sections. Collapse them so an
/// expanded thought reads as tight paragraphs instead of large gaps.
fn tidy_reasoning(text: &str) -> SharedString {
    let mut tidy = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.trim().lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        tidy.push_str(line);
        tidy.push('\n');
    }
    tidy.truncate(tidy.trim_end().len());
    tidy.into()
}

/// A collapsed activity run between messages. Tool calls and context updates
/// share one compact row; expanding it reveals individually expandable items.
fn render_activity_group(
    group: &ActivityGroupRow,
    expanded: bool,
    expanded_tools: &HashSet<String>,
    index: usize,
    entity: &Entity<Transcript>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let key = group.key.clone();
    let entity = entity.clone();
    let header_entity = entity.clone();
    let chevron = if expanded { IconName::ChevronDown } else { IconName::ChevronRight };
    let icon_name = match group.items.as_slice() {
        [single] if single.tone != ActivityTone::Tool => activity_tone_icon(single.tone),
        _ => IconName::SquareTerminal,
    };

    let header = h_flex()
        .id(("activity-toggle", index))
        .test_support()
        .aria_expanded(expanded)
        .gap_2()
        .items_center()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(ui::icon(icon_name).xsmall())
        .child(summarize_group(group))
        .child(ui::icon(chevron).xsmall())
        .on_click(move |_, _, cx| {
            header_entity.update(cx, |transcript, cx| transcript.toggle_row(&key, index, cx));
        });

    v_flex().gap_1p5().child(header).when(expanded, |column| {
        column.child(v_flex().gap_1().pl_6().children(group.items.iter().map(|item| {
            let item_key = item.id.clone();
            let details_id = format!("activity-details-{}", item.id);
            let entity = entity.clone();
            let item_expanded = expanded_tools.contains(&item.id);
            let kind = item.kind.replace(['.', '_', '-'], " ");
            v_flex()
                .id(format!("activity-{}", item.id))
                .gap_1()
                .child(
                    h_flex()
                        .id(format!("activity-toggle-{}", item.id))
                        .test_support()
                        .aria_expanded(item_expanded)
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(ui::icon(activity_tone_icon(item.tone)).xsmall())
                        .child(div().flex_1().child(format!("{kind} · {}", item.summary)))
                        .child(
                            ui::icon(if item_expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall(),
                        )
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            entity.update(cx, |transcript, cx| {
                                transcript.toggle_tool(&item_key, index, cx);
                            });
                        }),
                )
                .when(item_expanded, |detail| {
                    let payload = serde_json::to_string_pretty(&item.payload)
                        .unwrap_or_else(|_| item.payload.to_string());
                    detail.child(
                        div().id(details_id).test_support().max_h(px(180.)).ml_5().child(
                            div()
                                .max_h(px(180.))
                                .overflow_y_scrollbar()
                                .p_2()
                                .rounded_md()
                                .bg(theme.secondary)
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .child(payload),
                        ),
                    )
                })
                .into_any_element()
        })))
    })
}

fn render_working(working_since: Option<Instant>, cx: &App) -> impl IntoElement {
    let label = match working_since {
        Some(started) => format!("Working for {}", format_elapsed(started.elapsed())),
        None => "Working".to_owned(),
    };
    let theme = cx.theme();
    h_flex()
        .gap_2()
        .items_center()
        .text_sm()
        .font_medium()
        .text_color(theme.muted_foreground)
        .child(div().size_1p5().rounded_full().bg(theme.primary))
        .child(label)
}

fn activity_tone_icon(tone: ActivityTone) -> IconName {
    match tone {
        ActivityTone::Error => IconName::CircleAlert,
        ActivityTone::Approval => IconName::CircleCheck,
        ActivityTone::Info | ActivityTone::Tool | ActivityTone::Unknown => IconName::Info,
    }
}

fn summarize_group(group: &ActivityGroupRow) -> String {
    match group.items.as_slice() {
        [] => String::new(),
        [single] => single.summary.to_string(),
        many => {
            let tools = many.iter().filter(|item| item.tone == ActivityTone::Tool).count();
            let updates = many.len() - tools;
            match (tools, updates) {
                (0, n) => format!("{n} status updates"),
                (1, 0) => "Ran 1 tool call".to_owned(),
                (n, 0) => format!("Ran {n} tool calls"),
                (n, m) => format!(
                    "Ran {n} tool call{} · {m} status update{}",
                    if n == 1 { "" } else { "s" },
                    if m == 1 { "" } else { "s" }
                ),
            }
        }
    }
}

/// "1m 57s" above a minute, "57s" below it.
fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes > 0 { format!("{minutes}m {seconds:02}s") } else { format!("{seconds}s") }
}

/// A transcript row.
///
/// `TextView::markdown` (via `gpui-base`'s `TextViewState::set_element_text`)
/// recognizes unchanged text by comparing the incoming string's allocation
/// pointer to the one it parsed last frame, not its content — and the
/// virtualized list re-invokes every visible row's renderer on every frame it
/// repaints (`gpui-pre`'s `elements/list.rs`), not only when the row's data
/// changes. `build_rows` below keeps a row's `SharedString`s from the
/// previous build whenever the underlying message/activity is unchanged (via
/// `Clone`, an `Arc` bump), so that pointer stays stable across frames.
#[derive(Clone)]
enum Row {
    Message(MessageRow),
    Thought(ThoughtRow),
    Activities(ActivityGroupRow),
    /// "Working for Xm Ys": appended after the rest while a turn is running.
    /// Carries no data of its own; `render_row` reads `working_since` fresh
    /// every render instead.
    Working,
}

#[derive(Clone)]
struct MessageRow {
    id: String,
    role: MessageRole,
    text: SharedString,
    streaming: bool,
    attachments: Vec<UploadedAttachment>,
}

#[derive(Clone)]
struct ThoughtRow {
    id: String,
    text: SharedString,
    streaming: bool,
    attachments: Vec<UploadedAttachment>,
}

#[derive(Clone, PartialEq)]
struct ActivityItemRow {
    id: String,
    summary: SharedString,
    tone: ActivityTone,
    kind: String,
    payload: Value,
}

#[derive(Clone, PartialEq)]
struct ActivityGroupRow {
    /// The first activity's id in this group; identifies the row across
    /// rebuilds so expand state (keyed on this) survives new stream items.
    key: String,
    items: Vec<ActivityItemRow>,
}

/// Interleaves `messages` and `activities` by `created_at` and groups
/// every activity in the same message interval into one row, mirroring
/// `apps/web/src/components/chat/MessagesTimeline.logic.ts`'s activity
/// grouping. Status events remain in the same collapsible run as tool calls;
/// reasoning messages still split runs so their timeline order remains clear.
///
/// Reuses a row from `old_rows` at the same position when the underlying
/// data is unchanged, so unaffected `SharedString`s keep their allocation
/// (see the `Row` doc comment). Returns the new rows plus the index of the
/// first row (below `old_rows.len()`) whose content actually changed, for
/// the caller to hand the scroller a remeasure range.
fn build_rows(
    messages: &[Message],
    activities: &[Activity],
    old_rows: &[Row],
) -> (Vec<Row>, Option<usize>) {
    #[derive(Clone, Copy)]
    enum Entry {
        Msg(usize),
        Act(usize),
    }

    fn created_at<'a>(
        entry: Entry,
        messages: &'a [Message],
        activities: &'a [Activity],
    ) -> &'a str {
        match entry {
            Entry::Msg(index) => messages[index].created_at.as_str(),
            Entry::Act(index) => activities[index].created_at.as_str(),
        }
    }

    let mut entries: Vec<Entry> = Vec::with_capacity(messages.len() + activities.len());
    entries.extend((0..messages.len()).map(Entry::Msg));
    entries.extend((0..activities.len()).map(Entry::Act));
    entries.sort_by(|&a, &b| {
        created_at(a, messages, activities).cmp(created_at(b, messages, activities))
    });

    let mut rows = Vec::with_capacity(entries.len());
    let mut first_changed = None;
    let mut pending: Vec<usize> = Vec::new();

    for entry in entries {
        match entry {
            Entry::Act(index) => pending.push(index),
            Entry::Msg(index) => {
                flush_activity_group(
                    &mut pending,
                    &mut rows,
                    activities,
                    old_rows,
                    &mut first_changed,
                );
                let position = rows.len();
                let (row, reused) = match messages[index].role {
                    MessageRole::Reasoning => thought_row(index, messages, old_rows, position),
                    _ => message_row(index, messages, old_rows, position),
                };
                if !reused && position < old_rows.len() {
                    first_changed.get_or_insert(position);
                }
                rows.push(row);
            }
        }
    }
    flush_activity_group(&mut pending, &mut rows, activities, old_rows, &mut first_changed);

    (rows, first_changed)
}

fn message_row(
    index: usize,
    messages: &[Message],
    old_rows: &[Row],
    position: usize,
) -> (Row, bool) {
    let message = &messages[index];
    let previous = if let Some(Row::Message(old)) = old_rows.get(position)
        && old.id == message.id
        && old.streaming == message.streaming
        && old.text.len() == message.text.len()
    {
        Some(old)
    } else {
        None
    };
    if let Some(old) = previous
        && old.attachments == message.attachments
    {
        return (Row::Message(old.clone()), true);
    }
    (
        Row::Message(MessageRow {
            id: message.id.clone(),
            role: message.role,
            text: previous
                .map_or_else(|| SharedString::from(message.text.clone()), |old| old.text.clone()),
            streaming: message.streaming,
            attachments: message.attachments.clone(),
        }),
        false,
    )
}

fn thought_row(
    index: usize,
    messages: &[Message],
    old_rows: &[Row],
    position: usize,
) -> (Row, bool) {
    let message = &messages[index];
    let previous = if let Some(Row::Thought(old)) = old_rows.get(position)
        && old.id == message.id
        && old.streaming == message.streaming
        && old.text.len() == message.text.len()
    {
        Some(old)
    } else {
        None
    };
    if let Some(old) = previous
        && old.attachments == message.attachments
    {
        return (Row::Thought(old.clone()), true);
    }
    (
        Row::Thought(ThoughtRow {
            id: message.id.clone(),
            text: previous
                .map_or_else(|| SharedString::from(message.text.clone()), |old| old.text.clone()),
            streaming: message.streaming,
            attachments: message.attachments.clone(),
        }),
        false,
    )
}

fn flush_activity_group(
    pending: &mut Vec<usize>,
    rows: &mut Vec<Row>,
    activities: &[Activity],
    old_rows: &[Row],
    first_changed: &mut Option<usize>,
) {
    if pending.is_empty() {
        return;
    }
    let position = rows.len();
    let items: Vec<_> = pending.iter().map(|&index| activity_item(&activities[index])).collect();
    let first_id = activities[pending[0]].id.as_str();
    if let Some(Row::Activities(old)) = old_rows.get(position)
        && old.key == first_id
        && old.items == items
    {
        rows.push(Row::Activities(old.clone()));
        pending.clear();
        return;
    }
    if position < old_rows.len() {
        first_changed.get_or_insert(position);
    }
    rows.push(Row::Activities(ActivityGroupRow { key: first_id.to_owned(), items }));
    pending.clear();
}

fn activity_item(activity: &Activity) -> ActivityItemRow {
    ActivityItemRow {
        id: activity.id.clone(),
        summary: SharedString::from(activity.summary.clone()),
        tone: activity.tone,
        kind: activity.kind.clone(),
        payload: activity.payload.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use t3_client::attachments::AttachmentKind;
    // Shadows the `gpui::test` macro brought in by `use super::*`.
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;

    fn message(id: &str, role: MessageRole, created_at: &str) -> Message {
        Message {
            id: id.to_owned(),
            attachments: Vec::new(),
            role,
            text: "hi".to_owned(),
            turn_id: None,
            streaming: false,
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
        }
    }

    fn activity(id: &str, tone: ActivityTone, kind: &str, created_at: &str) -> Activity {
        Activity {
            id: id.to_owned(),
            tone,
            kind: kind.to_owned(),
            summary: format!("summary-{id}"),
            payload: serde_json::Value::Null,
            turn_id: None,
            created_at: created_at.to_owned(),
        }
    }

    #[test]
    fn reasoning_text_drops_padding_and_repeated_blank_lines() {
        let text = "\n\n**Planning**  \n\n\n\nRead the file.\n\n\n";
        assert_eq!(tidy_reasoning(text).as_ref(), "**Planning**\n\nRead the file.");
    }

    #[test]
    fn interleaves_messages_and_activities_by_time() {
        let messages = vec![
            message("m1", MessageRole::User, "2026-01-01T00:00:00.000Z"),
            message("m2", MessageRole::Assistant, "2026-01-01T00:00:03.000Z"),
        ];
        let activities =
            vec![activity("a1", ActivityTone::Tool, "tool.completed", "2026-01-01T00:00:01.000Z")];
        let (rows, _) = build_rows(&messages, &activities, &[]);
        assert_eq!(rows.len(), 3);
        assert!(matches!(&rows[0], Row::Message(m) if m.id == "m1"));
        assert!(matches!(&rows[1], Row::Activities(g) if g.key == "a1"));
        assert!(matches!(&rows[2], Row::Message(m) if m.id == "m2"));
    }

    #[test]
    fn groups_consecutive_tool_activities() {
        let activities = vec![
            activity("a1", ActivityTone::Tool, "tool.started", "2026-01-01T00:00:00.000Z"),
            activity("a2", ActivityTone::Tool, "tool.completed", "2026-01-01T00:00:01.000Z"),
            activity("a3", ActivityTone::Tool, "tool.completed", "2026-01-01T00:00:02.000Z"),
        ];
        let (rows, _) = build_rows(&[], &activities, &[]);
        assert_eq!(rows.len(), 1);
        let Row::Activities(group) = &rows[0] else { panic!("expected an activity group") };
        assert_eq!(group.key, "a1");
        assert_eq!(group.items.len(), 3);
    }

    #[test]
    fn context_events_stay_with_tool_calls_between_messages() {
        let messages = vec![
            message("m1", MessageRole::User, "2026-01-01T00:00:00.000Z"),
            message("m2", MessageRole::Assistant, "2026-01-01T00:00:04.000Z"),
        ];
        let mut activities = vec![
            activity("a1", ActivityTone::Tool, "tool.started", "2026-01-01T00:00:01.000Z"),
            activity("a2", ActivityTone::Info, "thread.status", "2026-01-01T00:00:02.000Z"),
            activity("a3", ActivityTone::Tool, "tool.completed", "2026-01-01T00:00:03.000Z"),
        ];
        activities[0].payload = serde_json::json!({"command": "cargo test", "output": "ok"});
        let (rows, _) = build_rows(&messages, &activities, &[]);
        assert_eq!(rows.len(), 3);
        assert!(matches!(&rows[0], Row::Message(m) if m.id == "m1"));
        let Row::Activities(group) = &rows[1] else { panic!("expected an activity run") };
        assert_eq!(
            group.items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            ["a1", "a2", "a3"]
        );
        assert_eq!(group.items[0].payload["output"], "ok");
        assert!(matches!(&rows[2], Row::Message(m) if m.id == "m2"));
        assert_eq!(summarize_group(group), "Ran 2 tool calls · 1 status update");
    }

    #[test]
    fn activity_run_split_keeps_reasoning_messages_in_timeline_order() {
        let messages = vec![message("m1", MessageRole::Reasoning, "2026-01-01T00:00:02.000Z")];
        let activities = vec![
            activity("a1", ActivityTone::Tool, "tool.started", "2026-01-01T00:00:01.000Z"),
            activity("a2", ActivityTone::Info, "thread.status", "2026-01-01T00:00:03.000Z"),
        ];
        let (rows, _) = build_rows(&messages, &activities, &[]);
        assert_eq!(rows.len(), 3);
        assert!(matches!(&rows[0], Row::Activities(group) if group.key == "a1"));
        assert!(matches!(&rows[1], Row::Thought(thought) if thought.id == "m1"));
        assert!(matches!(&rows[2], Row::Activities(group) if group.key == "a2"));
    }

    #[test]
    fn reasoning_messages_become_thought_rows() {
        let messages = vec![message("m1", MessageRole::Reasoning, "2026-01-01T00:00:00.000Z")];
        let (rows, _) = build_rows(&messages, &[], &[]);
        assert!(matches!(&rows[0], Row::Thought(t) if t.id == "m1"));
    }

    #[test]
    fn unchanged_rows_reuse_the_previous_shared_string_allocation() {
        let mut messages = vec![message("m1", MessageRole::Assistant, "2026-01-01T00:00:00.000Z")];
        // Long enough to be heap-allocated: short `SharedString`s are stored
        // inline, so their pointer moves with the row.
        messages[0].text = "A reply long enough to live on the heap. ".repeat(4);
        let (first, _) = build_rows(&messages, &[], &[]);
        let (second, first_changed) = build_rows(&messages, &[], &first);
        let Row::Message(a) = &first[0] else { panic!() };
        let Row::Message(b) = &second[0] else { panic!() };
        // `SharedString`'s `PartialEq` compares content, but pointer
        // stability is what unblocks `TextView`'s reparse skip, so compare
        // the underlying allocation directly.
        assert!(a.text.as_ptr() == b.text.as_ptr());
        assert_eq!(first_changed, None);
    }

    #[test]
    fn a_changed_message_is_reported_as_the_first_changed_row() {
        let mut messages = vec![
            message("m1", MessageRole::User, "2026-01-01T00:00:00.000Z"),
            message("m2", MessageRole::Assistant, "2026-01-01T00:00:01.000Z"),
        ];
        let (first, _) = build_rows(&messages, &[], &[]);
        messages[1].text = "hi there".to_owned();
        let (_, first_changed) = build_rows(&messages, &[], &first);
        assert_eq!(first_changed, Some(1));
    }

    #[test]
    fn attachment_updates_refresh_row_metadata_without_replacing_markdown_text() {
        let mut messages = vec![message("m1", MessageRole::Assistant, "2026-01-01T00:00:00.000Z")];
        messages[0].text = "A stable message body long enough to own a shared allocation.".into();
        let (first, _) = build_rows(&messages, &[], &[]);
        let attachment = UploadedAttachment {
            kind: AttachmentKind::File,
            id: "upload-1".into(),
            name: "notes.txt".into(),
            mime_type: "text/plain".into(),
            size_bytes: 512,
        };
        messages[0].attachments.push(attachment.clone());

        let (updated, first_changed) = build_rows(&messages, &[], &first);
        let (Row::Message(old), Row::Message(new)) = (&first[0], &updated[0]) else {
            panic!("expected message rows")
        };
        assert_eq!(first_changed, Some(0));
        assert_eq!(new.attachments, [attachment]);
        assert!(old.text.as_ptr() == new.text.as_ptr());
    }

    #[test]
    fn attachment_size_labels_are_human_readable() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MB");
    }

    #[gpui_kit::test]
    fn individual_tool_details_expand_without_collapsing_the_run(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;

        cx.update(gpui_kit::init);
        let (handle, transcript) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(800.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(Transcript::new),
            )
            .unwrap()
        });
        let snapshot = serde_json::from_value::<t3_client::ThreadDetailSnapshot>(serde_json::json!({
            "snapshotSequence": 0,
            "thread": {
                "id": "thread-1",
                "projectId": "project-1",
                "title": "Tool run",
                "messages": [],
                "activities": [
                    {"id":"tool-1","tone":"tool","kind":"tool.started","summary":"Run tests","createdAt":"2026-01-01T00:00:01.000Z","payload":{"command":"cargo test"}},
                    {"id":"status-1","tone":"info","kind":"thread.status","summary":"Waiting for output","createdAt":"2026-01-01T00:00:02.000Z","payload":{}},
                    {"id":"tool-2","tone":"tool","kind":"tool.completed","summary":"Tests passed","createdAt":"2026-01-01T00:00:03.000Z","payload":{"output":"ok"}}
                ]
            }
        })).unwrap();
        transcript.update(cx, |transcript, cx| {
            transcript.apply(ThreadStreamItem::Snapshot { snapshot }, cx)
        });

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("activity-toggle", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find("activity-toggle-tool-1").is_some());
            assert_eq!(window.find(("activity-toggle", 0usize)).expanded(), Some(true));
            assert!(window.try_find("activity-details-tool-1").is_none());
            window.click("activity-toggle-tool-1", cx);
            window.render_frame(cx);
            assert!(transcript.read(cx).expanded.contains("tool-1"));
            assert!(transcript.read(cx).expanded_tools.contains("tool-1"));
            assert_eq!(window.find("activity-toggle-tool-1").expanded(), Some(true));
            assert_eq!(window.find(("activity-toggle", 0usize)).expanded(), Some(true));
            assert!(window.try_find("activity-details-tool-1").is_some());
            window.click("activity-toggle-tool-1", cx);
            window.render_frame(cx);
            assert!(transcript.read(cx).expanded.contains("tool-1"));
            assert!(!transcript.read(cx).expanded_tools.contains("tool-1"));
            assert_eq!(window.find("activity-toggle-tool-1").expanded(), Some(false));
            assert_eq!(window.find(("activity-toggle", 0usize)).expanded(), Some(true));
            assert!(window.try_find("activity-details-tool-1").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn copy_controls_are_compact_and_message_rows_stay_tightly_spaced(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;

        cx.update(gpui_kit::init);
        let (handle, transcript) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(Transcript::new),
            )
            .unwrap()
        });
        let snapshot = serde_json::from_value::<t3_client::ThreadDetailSnapshot>(serde_json::json!({
            "snapshotSequence": 0,
            "thread": {
                "id": "thread-1",
                "projectId": "project-1",
                "title": "Two replies",
                "messages": [
                    {"id":"message-1","role":"assistant","text":"First reply","streaming":false,"createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"},
                    {"id":"message-2","role":"assistant","text":"Second reply","streaming":false,"createdAt":"2026-01-01T00:00:01.000Z","updatedAt":"2026-01-01T00:00:01.000Z"}
                ],
                "activities": []
            }
        })).unwrap();
        transcript.update(cx, |transcript, cx| {
            transcript.apply(ThreadStreamItem::Snapshot { snapshot }, cx)
        });

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let first = window.find("copy-message-message-1").bounds();
            let second = window.find("copy-message-message-2").bounds();
            assert!(first.size.width <= px(40.));
            assert!(second.size.width <= px(40.));
            assert!(second.origin.y - first.origin.y <= px(100.));
        })
        .unwrap();
    }
}
