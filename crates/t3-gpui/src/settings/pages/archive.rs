//! Archive: the threads archived on the connected server, with a filter box
//! and a Restore action per thread. Nothing here is a setting, so there is
//! nothing to restore to defaults.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::*;
use t3_client::{ShellSnapshot, ThreadAction, ThreadShell};

use crate::settings::row::SettingsGroup;
use crate::settings::search::SearchEntry;
use crate::settings::{Section, SettingsEvent, SettingsPage};

pub const SEARCH: &[SearchEntry] = &[SearchEntry {
    title: "Archived threads",
    description: "Browse archived threads and restore them to the sidebar.",
    keywords: &["unarchive", "restore", "history", "old threads"],
    section: Section::Archive,
}];

/// What the page knows about archived threads. Loaded when the page opens and
/// on refresh; shares no state with the sidebar's own archive shelf.
pub struct State {
    threads: Vec<ThreadShell>,
    /// Project ID to title, from the archive snapshot.
    projects: HashMap<String, String>,
    request: Option<String>,
    loaded: bool,
    failed: bool,
    /// Threads with an unarchive in flight.
    restoring: Vec<String>,
    filter: Entity<InputState>,
}

impl State {
    pub fn new(
        window: &mut Window,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<SettingsPage>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter archived threads"));
        subscriptions.push(cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        }));
        Self {
            threads: Vec::new(),
            projects: HashMap::new(),
            request: None,
            loaded: false,
            failed: false,
            restoring: Vec::new(),
            filter,
        }
    }

    /// Threads whose title or project title contains every word of `query`.
    fn visible(&self, query: &str) -> Vec<&ThreadShell> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.threads
            .iter()
            .filter(|thread| {
                let project = self.projects.get(&thread.project_id).map_or("", String::as_str);
                let haystack = format!("{} {}", thread.title, project).to_lowercase();
                words.iter().all(|word| haystack.contains(word.as_str()))
            })
            .collect()
    }
}

pub fn modified(_: &SettingsPage, _: &App) -> bool {
    false
}

pub fn restore_defaults(_: &mut SettingsPage, _: &mut Context<SettingsPage>) {}

/// Asks the server for the archived threads.
pub fn load(page: &mut SettingsPage, cx: &mut Context<SettingsPage>) {
    if !page.connected {
        return;
    }
    let id = t3_client::new_id();
    page.archive.request = Some(id.clone());
    page.archive.failed = false;
    cx.emit(SettingsEvent::LoadArchived(id));
    cx.notify();
}

/// Asks the server to unarchive `thread_id`.
pub fn unarchive(page: &mut SettingsPage, thread_id: &str, cx: &mut Context<SettingsPage>) {
    if !page.connected || page.archive.restoring.iter().any(|id| id == thread_id) {
        return;
    }
    page.archive.restoring.push(thread_id.to_owned());
    cx.emit(SettingsEvent::ThreadAction {
        thread_id: thread_id.to_owned(),
        action: ThreadAction::Unarchive,
    });
    cx.notify();
}

impl SettingsPage {
    /// The answer to a [`SettingsEvent::LoadArchived`]. Answers to other
    /// requests (the sidebar shares the event stream) are ignored.
    pub fn set_archived(
        &mut self,
        request_id: &str,
        snapshot: Option<ShellSnapshot>,
        cx: &mut Context<Self>,
    ) {
        if self.archive.request.as_deref() != Some(request_id) {
            return;
        }
        self.archive.request = None;
        self.archive.failed = snapshot.is_none();
        if let Some(snapshot) = snapshot {
            self.archive.loaded = true;
            self.archive.projects =
                snapshot.projects.into_iter().map(|project| (project.id, project.title)).collect();
            self.archive.threads =
                snapshot.threads.into_iter().filter(|thread| thread.archived_at.is_some()).collect();
            self.archive.threads.sort_by(|a, b| b.archived_at.cmp(&a.archived_at));
        }
        cx.notify();
    }

    /// The answer to a [`SettingsEvent::ThreadAction`] sent from this page
    /// (other threads' actions are ignored).
    pub fn archive_action_finished(
        &mut self,
        thread_id: &str,
        action: &ThreadAction,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        if !matches!(action, ThreadAction::Unarchive) {
            return;
        }
        let before = self.archive.restoring.len();
        self.archive.restoring.retain(|id| id != thread_id);
        if success {
            self.archive.threads.retain(|thread| thread.id != thread_id);
        }
        if self.archive.restoring.len() != before || success {
            cx.notify();
        }
    }
}

/// The date part of an ISO timestamp.
fn date_label(timestamp: &str) -> &str {
    timestamp.split('T').next().unwrap_or(timestamp)
}

pub fn render(page: &SettingsPage, cx: &Context<SettingsPage>) -> AnyElement {
    let state = &page.archive;
    let theme = cx.theme();
    let query = state.filter.read(cx).value().to_string();
    let visible = state.visible(&query);
    let loading = state.request.is_some();

    let toolbar = h_flex()
        .gap_2()
        .items_center()
        .child(div().flex_1().min_w_0().child(Input::new(&state.filter).small().cleanable(true)))
        .child(
            Button::new("archive-refresh")
                .ghost()
                .small()
                .icon(Icon::new(IconName::RotateCcw))
                .tooltip("Refresh archived threads")
                .disabled(!page.connected || loading)
                .on_click(cx.listener(|this, _, _, cx| load(this, cx))),
        );

    let note = |text: &'static str| {
        div().px_4().py_6().text_sm().text_color(theme.muted_foreground).child(text)
    };
    let mut group = SettingsGroup::new("Archived threads");
    if visible.is_empty() {
        group = group.child(note(if !page.connected {
            "Connect to a server to browse archived threads."
        } else if loading && !state.loaded {
            "Loading archived threads..."
        } else if state.failed {
            "Archived threads could not be loaded."
        } else if state.threads.is_empty() {
            "No archived threads."
        } else {
            "No archived threads match."
        }));
    }
    for thread in visible {
        let id = thread.id.clone();
        let project = state.projects.get(&thread.project_id).cloned().unwrap_or_default();
        let archived = thread.archived_at.as_deref().map(date_label).unwrap_or_default();
        let detail = if project.is_empty() {
            format!("Archived {archived}")
        } else {
            format!("{project} · Archived {archived}")
        };
        let busy = state.restoring.contains(&id);
        group = group.child(
            h_flex()
                .id(SharedString::from(format!("archive-row-{id}")))
                .gap_4()
                .px_4()
                .py_3()
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().truncate().text_sm().font_medium().child(thread.title.clone()))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("archive-restore-{id}")))
                        .outline()
                        .small()
                        .label(if busy { "Restoring" } else { "Unarchive" })
                        .disabled(!page.connected || busy)
                        .on_click(cx.listener(move |this, _, _, cx| unarchive(this, &id, cx))),
                ),
        );
    }
    v_flex().gap_4().child(toolbar).child(group).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn snapshot() -> ShellSnapshot {
        serde_json::from_str(
            r#"{
            "snapshotSequence": 1,
            "projects": [{ "id": "p1", "title": "Alpha", "workspaceRoot": "/a" }],
            "threads": [
                { "id": "t1", "projectId": "p1", "title": "Fix login", "runtimeMode": "full-access",
                  "archivedAt": "2026-01-02T10:00:00.000Z" },
                { "id": "t2", "projectId": "p1", "title": "Refactor", "runtimeMode": "full-access",
                  "archivedAt": "2026-03-04T10:00:00.000Z" },
                { "id": "t3", "projectId": "p1", "title": "Live", "runtimeMode": "full-access" }
            ]
        }"#,
        )
        .unwrap()
    }

    type Events = Rc<RefCell<Vec<String>>>;

    /// Records the load and thread-action events the page emits.
    fn capture(page: &Entity<SettingsPage>, cx: &mut TestAppContext) -> (Events, Subscription) {
        let events = Events::default();
        let captured = events.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(page, move |_, event: &SettingsEvent, _| match event {
                SettingsEvent::LoadArchived(id) => captured.borrow_mut().push(format!("load:{id}")),
                SettingsEvent::ThreadAction { thread_id, action } => {
                    assert_eq!(*action, ThreadAction::Unarchive);
                    captured.borrow_mut().push(format!("unarchive:{thread_id}"))
                }
                _ => {}
            })
        });
        (events, subscription)
    }

    /// Opens the page, connected and with `snapshot()` loaded.
    fn loaded_page(
        cx: &mut TestAppContext,
    ) -> (AnyWindowHandle, Entity<SettingsPage>, Events, Subscription) {
        let (handle, page) = crate::settings::tests::open_page(cx, size(px(900.), px(900.)));
        let (events, subscription) = capture(&page, cx);
        page.update(cx, |page, cx| {
            page.set_open(true, cx);
            page.set_connected(true, cx);
            page.section = Section::Archive;
            load(page, cx);
        });
        let request = page.read_with(cx, |page, _| page.archive.request.clone().unwrap());
        assert_eq!(events.borrow()[0], format!("load:{request}"));
        page.update(cx, |page, cx| {
            // A stale answer is ignored.
            page.set_archived("other", Some(snapshot()), cx);
            assert!(page.archive.threads.is_empty());
            page.set_archived(&request, Some(snapshot()), cx);
        });
        (handle, page, events, subscription)
    }

    #[gpui_kit::test]
    fn archive_lists_only_archived_threads_newest_first_and_filters(cx: &mut TestAppContext) {
        let (_, page, _, _subscription) = loaded_page(cx);
        page.read_with(cx, |page, _| {
            let titles: Vec<_> = page.archive.visible("").iter().map(|t| t.title.clone()).collect();
            assert_eq!(titles, ["Refactor", "Fix login"]);
            assert_eq!(page.archive.visible("alpha login").len(), 1);
            assert!(page.archive.visible("zzz").is_empty());
        });
    }

    #[gpui_kit::test]
    fn unarchive_emits_the_action_once_and_removes_the_thread(cx: &mut TestAppContext) {
        let (handle, page, events, _subscription) = loaded_page(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("archive-row-t3").is_none());
            window.click("archive-restore-t1", cx);
            window.render_frame(cx);
            // Pressing again while in flight does nothing.
            window.click("archive-restore-t1", cx);
        })
        .unwrap();
        assert_eq!(events.borrow()[1..], ["unarchive:t1"]);
        page.update(cx, |page, cx| {
            page.archive_action_finished("t1", &ThreadAction::Unarchive, true, cx);
            assert_eq!(page.archive.visible("").len(), 1);
            assert!(page.archive.restoring.is_empty());
        });
    }

    #[test]
    fn dates_drop_the_time() {
        assert_eq!(date_label("2026-01-02T10:00:00.000Z"), "2026-01-02");
        assert_eq!(date_label("odd"), "odd");
    }
}
