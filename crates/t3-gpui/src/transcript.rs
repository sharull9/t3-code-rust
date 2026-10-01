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
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
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
        self.state.thread.as_ref().map(|thread| t3_client::pending::approvals(&thread.activities)).unwrap_or_default()
    }

    pub fn user_inputs(&self) -> Option<Vec<t3_client::pending::PendingUserInput>> {
        self.state.thread.as_ref().map(|thread| t3_client::pending::user_inputs(&thread.activities))
    }

    fn is_working(&self) -> bool {
        self.session().is_some_and(Session::is_working)
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.state = ThreadState::default();
        self.rows = Rc::default();
        self.working_since = None;
        self.scroller
            .update(cx, |scroller, cx| scroller.reset(0, cx));
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
        let entity = cx.entity();
        let working_since = self.working_since;
        MessageScroller::new(
            "transcript",
            self.scroller.clone(),
            move |index, _, cx| match rows.get(index) {
                Some(row) => column(render_row(
                    row,
                    index,
                    &expanded,
                    &entity,
                    working_since,
                    cx,
                ))
                .when(index == 0, |row| row.pt_2())
                .into_any_element(),
                None => div().into_any_element(),
            },
        )
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
    div().flex().w_full().justify_center().px_6().child(
        div()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .px_1()
            .py_2()
            .child(content),
    )
}

fn render_row(
    row: &Row,
    index: usize,
    expanded: &HashSet<String>,
    entity: &Entity<Transcript>,
    working_since: Option<Instant>,
    cx: &App,
) -> AnyElement {
    match row {
        Row::Message(message) => render_message(message, cx).into_any_element(),
        Row::Thought(thought) => {
            let is_expanded = expanded.contains(&thought.id);
            render_thought(thought, is_expanded, index, entity, cx).into_any_element()
        }
        Row::Activities(group) => {
            let is_expanded = expanded.contains(&group.key);
            render_activity_group(group, is_expanded, index, entity, cx).into_any_element()
        }
        Row::Working => render_working(working_since, cx).into_any_element(),
    }
}

fn render_message(message: &MessageRow, cx: &App) -> impl IntoElement {
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
                    .bg(cx.theme().secondary_hover)
                    .text_sm()
                    .child(message.text.clone()),
            )
            .into_any_element(),
        MessageRole::Assistant => v_flex()
            .gap_2()
            .text_sm()
            .child(TextView::markdown(id, message.text.clone()))
            .into_any_element(),
        MessageRole::System | MessageRole::Unknown | MessageRole::Reasoning => div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(message.text.clone())
            .into_any_element(),
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
    let chevron = if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let label = if thought.streaming {
        "Thinking"
    } else {
        "Thought"
    };

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
        .when(expanded, |column| {
            column.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(thought.text.clone()),
            )
        })
}

/// A collapsed group of consecutive tool-call activities, like the T3 web
/// app's ">_ Ran N commands" row. A lone activity shows its own summary
/// instead of a count; either way the header expands to the per-item list.
fn render_activity_group(
    group: &ActivityGroupRow,
    expanded: bool,
    index: usize,
    entity: &Entity<Transcript>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let key = group.key.clone();
    let entity = entity.clone();
    let chevron = if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let icon_name = match group.items.as_slice() {
        [single] if single.tone != ActivityTone::Tool => activity_tone_icon(single.tone),
        _ => IconName::SquareTerminal,
    };

    let header = h_flex()
        .id(("activity-toggle", index))
        .gap_2()
        .items_center()
        .cursor_pointer()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(ui::icon(icon_name).xsmall())
        .child(summarize_group(group))
        .child(ui::icon(chevron).xsmall())
        .on_click(move |_, _, cx| {
            entity.update(cx, |transcript, cx| transcript.toggle_row(&key, index, cx));
        });

    v_flex().gap_1p5().child(header).when(expanded, |column| {
        column.child(
            v_flex()
                .gap_1()
                .pl_6()
                .children(group.items.iter().map(|item| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(item.summary.clone())
                })),
        )
    })
}

fn render_working(working_since: Option<Instant>, cx: &App) -> impl IntoElement {
    let label = match working_since {
        Some(started) => format!("Working for {}", format_elapsed(started.elapsed())),
        None => "Working".to_owned(),
    };
    h_flex()
        .gap_2()
        .items_center()
        .text_sm()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
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
        many => format!("Ran {} commands", many.len()),
    }
}

/// "1m 57s" above a minute, "57s" below it.
fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
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
}

#[derive(Clone)]
struct ThoughtRow {
    id: String,
    text: SharedString,
    streaming: bool,
}

#[derive(Clone)]
struct ActivityItemRow {
    summary: SharedString,
    tone: ActivityTone,
}

#[derive(Clone)]
struct ActivityGroupRow {
    /// The first activity's id in this group; identifies the row across
    /// rebuilds so expand state (keyed on this) survives new stream items.
    key: String,
    items: Vec<ActivityItemRow>,
}

/// Interleaves `messages` and `activities` by `created_at` and groups
/// consecutive tool-tone activities into one row, mirroring
/// `apps/web/src/components/chat/MessagesTimeline.logic.ts`'s activity
/// grouping (simplified: this client has no per-tool-call payload, so groups
/// summarize by count rather than by tool kind).
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
            Entry::Act(index) if activities[index].tone == ActivityTone::Tool => {
                pending.push(index);
            }
            _ => {
                flush_tool_group(
                    &mut pending,
                    &mut rows,
                    activities,
                    old_rows,
                    &mut first_changed,
                );
                let position = rows.len();
                let (row, reused) = match entry {
                    Entry::Act(index) => single_activity_row(index, activities, old_rows, position),
                    Entry::Msg(index) if messages[index].role == MessageRole::Reasoning => {
                        thought_row(index, messages, old_rows, position)
                    }
                    Entry::Msg(index) => message_row(index, messages, old_rows, position),
                };
                if !reused && position < old_rows.len() {
                    first_changed.get_or_insert(position);
                }
                rows.push(row);
            }
        }
    }
    flush_tool_group(
        &mut pending,
        &mut rows,
        activities,
        old_rows,
        &mut first_changed,
    );

    (rows, first_changed)
}

fn message_row(
    index: usize,
    messages: &[Message],
    old_rows: &[Row],
    position: usize,
) -> (Row, bool) {
    let message = &messages[index];
    if let Some(Row::Message(old)) = old_rows.get(position)
        && old.id == message.id
        && old.streaming == message.streaming
        && old.text.len() == message.text.len()
    {
        return (Row::Message(old.clone()), true);
    }
    (
        Row::Message(MessageRow {
            id: message.id.clone(),
            role: message.role,
            text: SharedString::from(message.text.clone()),
            streaming: message.streaming,
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
    if let Some(Row::Thought(old)) = old_rows.get(position)
        && old.id == message.id
        && old.streaming == message.streaming
        && old.text.len() == message.text.len()
    {
        return (Row::Thought(old.clone()), true);
    }
    (
        Row::Thought(ThoughtRow {
            id: message.id.clone(),
            text: SharedString::from(message.text.clone()),
            streaming: message.streaming,
        }),
        false,
    )
}

fn single_activity_row(
    index: usize,
    activities: &[Activity],
    old_rows: &[Row],
    position: usize,
) -> (Row, bool) {
    let activity = &activities[index];
    if let Some(Row::Activities(old)) = old_rows.get(position)
        && old.key == activity.id
        && old.items.len() == 1
    {
        return (Row::Activities(old.clone()), true);
    }
    (
        Row::Activities(ActivityGroupRow {
            key: activity.id.clone(),
            items: vec![ActivityItemRow {
                summary: SharedString::from(activity.summary.clone()),
                tone: activity.tone,
            }],
        }),
        false,
    )
}

fn flush_tool_group(
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
    let first_id = activities[pending[0]].id.as_str();
    if let Some(Row::Activities(old)) = old_rows.get(position)
        && old.key == first_id
        && old.items.len() == pending.len()
    {
        rows.push(Row::Activities(old.clone()));
        pending.clear();
        return;
    }
    if position < old_rows.len() {
        first_changed.get_or_insert(position);
    }
    let items = pending
        .iter()
        .map(|&index| {
            let activity = &activities[index];
            ActivityItemRow {
                summary: SharedString::from(activity.summary.clone()),
                tone: activity.tone,
            }
        })
        .collect();
    rows.push(Row::Activities(ActivityGroupRow {
        key: first_id.to_owned(),
        items,
    }));
    pending.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    // Shadows the `gpui::test` macro brought in by `use super::*`.
    use core::prelude::v1::test;

    fn message(id: &str, role: MessageRole, created_at: &str) -> Message {
        Message {
            id: id.to_owned(),
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
    fn interleaves_messages_and_activities_by_time() {
        let messages = vec![
            message("m1", MessageRole::User, "2026-01-01T00:00:00.000Z"),
            message("m2", MessageRole::Assistant, "2026-01-01T00:00:03.000Z"),
        ];
        let activities = vec![activity(
            "a1",
            ActivityTone::Tool,
            "tool.completed",
            "2026-01-01T00:00:01.000Z",
        )];
        let (rows, _) = build_rows(&messages, &activities, &[]);
        assert_eq!(rows.len(), 3);
        assert!(matches!(&rows[0], Row::Message(m) if m.id == "m1"));
        assert!(matches!(&rows[1], Row::Activities(g) if g.key == "a1"));
        assert!(matches!(&rows[2], Row::Message(m) if m.id == "m2"));
    }

    #[test]
    fn groups_consecutive_tool_activities() {
        let activities = vec![
            activity(
                "a1",
                ActivityTone::Tool,
                "tool.started",
                "2026-01-01T00:00:00.000Z",
            ),
            activity(
                "a2",
                ActivityTone::Tool,
                "tool.completed",
                "2026-01-01T00:00:01.000Z",
            ),
            activity(
                "a3",
                ActivityTone::Tool,
                "tool.completed",
                "2026-01-01T00:00:02.000Z",
            ),
        ];
        let (rows, _) = build_rows(&[], &activities, &[]);
        assert_eq!(rows.len(), 1);
        let Row::Activities(group) = &rows[0] else {
            panic!("expected an activity group")
        };
        assert_eq!(group.key, "a1");
        assert_eq!(group.items.len(), 3);
    }

    #[test]
    fn non_tool_activity_breaks_the_group() {
        let activities = vec![
            activity(
                "a1",
                ActivityTone::Tool,
                "tool.started",
                "2026-01-01T00:00:00.000Z",
            ),
            activity(
                "a2",
                ActivityTone::Info,
                "thread.settled",
                "2026-01-01T00:00:01.000Z",
            ),
            activity(
                "a3",
                ActivityTone::Tool,
                "tool.completed",
                "2026-01-01T00:00:02.000Z",
            ),
        ];
        let (rows, _) = build_rows(&[], &activities, &[]);
        assert_eq!(rows.len(), 3);
        assert!(matches!(&rows[0], Row::Activities(g) if g.key == "a1" && g.items.len() == 1));
        assert!(matches!(&rows[1], Row::Activities(g) if g.key == "a2" && g.items.len() == 1));
        assert!(matches!(&rows[2], Row::Activities(g) if g.key == "a3" && g.items.len() == 1));
    }

    #[test]
    fn reasoning_messages_become_thought_rows() {
        let messages = vec![message(
            "m1",
            MessageRole::Reasoning,
            "2026-01-01T00:00:00.000Z",
        )];
        let (rows, _) = build_rows(&messages, &[], &[]);
        assert!(matches!(&rows[0], Row::Thought(t) if t.id == "m1"));
    }

    #[test]
    fn unchanged_rows_reuse_the_previous_shared_string_allocation() {
        let mut messages = vec![message(
            "m1",
            MessageRole::Assistant,
            "2026-01-01T00:00:00.000Z",
        )];
        // Long enough to be heap-allocated: short `SharedString`s are stored
        // inline, so their pointer moves with the row.
        messages[0].text = "A reply long enough to live on the heap. ".repeat(4);
        let (first, _) = build_rows(&messages, &[], &[]);
        let (second, first_changed) = build_rows(&messages, &[], &first);
        let Row::Message(a) = &first[0] else { panic!() };
        let Row::Message(b) = &second[0] else {
            panic!()
        };
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
}
