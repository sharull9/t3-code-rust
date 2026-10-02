//! Remote directory chooser for projects hosted by the connected server.
//!
//! Paths are passed to `filesystem.browse`; this picker never inspects the
//! desktop machine's filesystem. A folder can only be selected after the
//! server has returned it as the verified `parent_path` of a browse result.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{WorkspaceBrowseEntry, WorkspaceBrowseResult};

const CONTEXT: &str = "DirectoryPicker";
static NEXT_BROWSE_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub enum DirectoryPickerEvent {
    Browse { id: u64, partial_path: String },
    Select { path: String, title: String },
    Cancel,
}

pub struct DirectoryPicker {
    focus_handle: FocusHandle,
    path: Entity<InputState>,
    open: bool,
    active_request_id: Option<u64>,
    active_partial_path: Option<String>,
    loading: bool,
    parent_path: Option<String>,
    entries: Vec<WorkspaceBrowseEntry>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DirectoryPickerEvent> for DirectoryPicker {}

impl DirectoryPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("Enter a server path"));
        let subscriptions =
            cx.subscribe_in(&path, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.browse(window, cx);
                }
                InputEvent::Change => this.path_changed(cx),
                _ => {}
            });
        Self {
            focus_handle: cx.focus_handle(),
            path,
            open: false,
            active_request_id: None,
            active_partial_path: None,
            loading: false,
            parent_path: None,
            entries: Vec::new(),
            error: None,
            _subscriptions: vec![subscriptions],
        }
    }

    pub fn open(
        &mut self,
        partial_path: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.open = true;
        self.parent_path = None;
        self.entries.clear();
        self.error = None;
        let partial_path = partial_path.into();
        let partial_path = if partial_path.trim().is_empty() {
            "~".to_owned()
        } else {
            partial_path.trim().to_owned()
        };
        self.path.update(cx, |state, cx| {
            state.set_value(partial_path, window, cx);
            state.focus(window, cx);
        });
        self.issue_browse(cx)
    }

    /// Close without emitting `Cancel`; callers use this after selection or
    /// when another app-level navigation replaces the modal.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.active_request_id = None;
        self.active_partial_path = None;
        self.loading = false;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn browse(&mut self, _: &mut Window, cx: &mut Context<Self>) -> u64 {
        self.issue_browse(cx)
    }

    pub fn go_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent_path) = self.parent_path.as_deref() else {
            return;
        };
        let up = remote_parent(parent_path);
        self.path.update(cx, |state, cx| state.set_value(up, window, cx));
        self.issue_browse(cx);
    }

    pub fn browse_child(&mut self, child_path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.path.update(cx, |state, cx| state.set_value(child_path, window, cx));
        self.issue_browse(cx);
    }

    /// Apply a server response only while its request is the latest request
    /// for the currently open dialog. A closed or reopened picker rejects it.
    pub fn apply_result(
        &mut self,
        id: u64,
        result: Result<WorkspaceBrowseResult, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let current_path = self.path.read(cx).value().trim().to_owned();
        if !result_matches(
            self.open,
            self.active_request_id,
            id,
            self.active_partial_path.as_deref(),
            &current_path,
        ) {
            if self.open && self.active_request_id == Some(id) {
                self.active_request_id = None;
                self.active_partial_path = None;
                self.loading = false;
                self.parent_path = None;
                self.entries.clear();
                cx.notify();
            }
            return false;
        }
        self.active_request_id = None;
        self.active_partial_path = None;
        self.loading = false;
        match result {
            Ok(result) => {
                self.path.update(cx, |state, cx| {
                    state.set_value(result.parent_path.clone(), window, cx)
                });
                self.parent_path = Some(result.parent_path);
                self.entries = result.entries;
                self.error = None;
            }
            Err(error) => {
                self.parent_path = None;
                self.entries.clear();
                self.error = Some(error);
            }
        }
        cx.notify();
        true
    }

    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.parent_path.clone() else {
            return;
        };
        let title = directory_title(&path);
        self.open = false;
        self.active_request_id = None;
        self.active_partial_path = None;
        self.loading = false;
        cx.emit(DirectoryPickerEvent::Select { path, title });
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.close(cx);
        cx.emit(DirectoryPickerEvent::Cancel);
    }

    fn issue_browse(&mut self, cx: &mut Context<Self>) -> u64 {
        let partial_path = self.path.read(cx).value().trim().to_owned();
        if partial_path.is_empty() {
            self.active_request_id = None;
            self.active_partial_path = None;
            self.loading = false;
            self.parent_path = None;
            self.entries.clear();
            self.error = Some("Enter a server path before browsing.".into());
            cx.notify();
            return 0;
        }
        let id = NEXT_BROWSE_REQUEST_ID.fetch_add(1, Ordering::Relaxed).max(1);
        self.active_request_id = Some(id);
        self.active_partial_path = Some(partial_path.clone());
        self.loading = true;
        self.parent_path = None;
        self.entries.clear();
        self.error = None;
        cx.emit(DirectoryPickerEvent::Browse { id, partial_path });
        cx.notify();
        id
    }

    fn path_changed(&mut self, cx: &mut Context<Self>) {
        let current_path = self.path.read(cx).value().trim().to_owned();
        if self.active_partial_path.as_deref() == Some(current_path.as_str())
            || self.parent_path.as_deref() == Some(current_path.as_str())
        {
            cx.notify();
            return;
        }
        self.active_request_id = None;
        self.active_partial_path = None;
        self.loading = false;
        self.parent_path = None;
        self.entries.clear();
        self.error = None;
        cx.notify();
    }

    fn render_row(
        &self,
        index: usize,
        entry: &WorkspaceBrowseEntry,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let path = entry.full_path.clone();
        h_flex()
            .id(("remote-directory-row", index))
            .gap_2()
            .px_2()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(theme.list_hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.browse_child(&path, window, cx);
            }))
            .child(div().text_sm().child("▸"))
            .child(div().flex_1().min_w_0().text_sm().truncate().child(entry.name.clone()))
            .child(div().text_xs().text_color(theme.muted_foreground).child("Open"))
    }
}

impl Render for DirectoryPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| self.render_row(index, entry, cx).into_any_element())
            .collect();
        let theme = cx.theme();
        let current_path = self.path.read(cx).value().trim().to_owned();
        let path_is_blank = current_path.is_empty();
        let can_choose =
            self.parent_path.as_deref() == Some(current_path.as_str()) && !self.loading;
        let can_go_up = self.parent_path.as_deref().is_some_and(|path| remote_parent(path) != path);

        div()
            .id("directory-picker-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_center()
            .bg(rgb(0x000000).opacity(0.5))
            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
            .child(
                v_flex()
                    .id("directory-picker-panel")
                    .test_support()
                    .key_context(CONTEXT)
                    .track_focus(&self.focus_handle)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(580.))
                    .h(px(440.))
                    .max_h(px(480.))
                    .rounded_xl()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .shadow_lg()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .child(div().text_base().font_semibold().child("Choose a folder"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("Browse directories on the connected server."),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                Input::new(&self.path)
                                    .appearance(false)
                                    .cleanable(false)
                                    .flex_1()
                                    .min_w_0(),
                            )
                            .child(
                                Button::new("directory-picker-browse")
                                    .small()
                                    .label(if self.loading { "Browsing…" } else { "Browse" })
                                    .disabled(self.loading || path_is_blank)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .child(
                                Button::new("directory-picker-up")
                                    .ghost()
                                    .small()
                                    .label("Up")
                                    .disabled(!can_go_up || self.loading)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.go_up(window, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(self.parent_path.clone().unwrap_or_default()),
                            ),
                    )
                    .child(
                        div()
                            .id("directory-picker-list")
                            .flex_1()
                            .min_h(px(160.))
                            .overflow_y_scrollbar()
                            .px_2()
                            .py_1()
                            .child(v_flex().gap_0p5().children(rows).when(
                                self.entries.is_empty() && !self.loading && self.error.is_none(),
                                |list| {
                                    list.child(
                                        div()
                                            .px_2()
                                            .py_4()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child("No child directories"),
                                    )
                                },
                            ))
                            .when(self.loading, |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_4()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child("Loading directories…"),
                                )
                            })
                            .when(self.error.is_some(), |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_4()
                                        .text_sm()
                                        .text_color(theme.danger)
                                        .child(self.error.clone().unwrap_or_default()),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(
                                Button::new("directory-picker-cancel")
                                    .ghost()
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                Button::new("directory-picker-choose")
                                    .small()
                                    .label("Choose folder")
                                    .disabled(!can_choose)
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Parent for a server-reported directory, with POSIX and Windows roots
/// handled without consulting the client's platform or filesystem.
pub fn remote_parent(path: &str) -> String {
    let trimmed = trim_remote_separators(path);
    if is_remote_root(trimmed) {
        return trimmed.to_owned();
    }
    let split = trimmed.rfind(['/', '\\']);
    match split {
        Some(0) => trimmed[..1].to_owned(),
        Some(2) if trimmed.as_bytes().get(1) == Some(&b':') => trimmed[..3].to_owned(),
        Some(index) => trimmed[..index].to_owned(),
        None if trimmed == "." || trimmed == ".." => ".".into(),
        None => ".".into(),
    }
}

fn trim_remote_separators(path: &str) -> &str {
    let mut end = path.len();
    while end > 0 && (path.as_bytes()[end - 1] == b'/' || path.as_bytes()[end - 1] == b'\\') {
        if end == 1 || (end == 3 && path.as_bytes().get(1) == Some(&b':')) {
            break;
        }
        end -= 1;
    }
    &path[..end]
}

fn is_remote_root(path: &str) -> bool {
    path == "/"
        || path == "\\"
        || (path.len() == 3 && path.as_bytes()[1] == b':' && path.ends_with(['/', '\\']))
}

fn directory_title(path: &str) -> String {
    let trimmed = trim_remote_separators(path);
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .filter(|part| *part != ":")
        .map_or_else(|| path.to_owned(), ToOwned::to_owned)
}

fn result_matches(
    open: bool,
    active_request_id: Option<u64>,
    id: u64,
    requested_path: Option<&str>,
    current_path: &str,
) -> bool {
    open && active_request_id == Some(id) && requested_path == Some(current_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use std::{cell::RefCell, rc::Rc};

    fn picker(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DirectoryPicker>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(760.), px(580.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| DirectoryPicker::new(window, cx)),
            )
            .unwrap()
        })
    }

    #[test]
    fn remote_parent_handles_posix_and_windows_paths() {
        assert_eq!(remote_parent("/work/project/src"), "/work/project");
        assert_eq!(remote_parent("/work"), "/");
        assert_eq!(remote_parent("/"), "/");
        assert_eq!(remote_parent("C:\\work\\project"), "C:\\work");
        assert_eq!(remote_parent("C:\\work"), "C:\\");
        assert_eq!(remote_parent("C:\\"), "C:\\");
    }

    #[test]
    fn selected_title_comes_from_server_directory_path() {
        assert_eq!(directory_title("/work/project/"), "project");
        assert_eq!(directory_title("C:\\work\\project"), "project");
        assert_eq!(directory_title("/"), "/");
    }

    #[test]
    fn late_browse_results_are_only_valid_for_open_matching_request() {
        assert!(result_matches(true, Some(7), 7, Some("/work"), "/work"));
        assert!(!result_matches(false, Some(7), 7, Some("/work"), "/work"));
        assert!(!result_matches(true, Some(8), 7, Some("/work"), "/work"));
        assert!(!result_matches(true, None, 7, Some("/work"), "/work"));
        assert!(!result_matches(true, Some(7), 7, Some("/work"), "/elsewhere"));
    }

    #[gpui_kit::test]
    fn blank_open_browses_server_default_and_blank_manual_path_is_rejected(
        cx: &mut TestAppContext,
    ) {
        let (handle, picker) = picker(cx);
        let browse_paths = Rc::new(RefCell::new(Vec::new()));
        let capture = browse_paths.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&picker, move |_, event: &DirectoryPickerEvent, _| {
                if let DirectoryPickerEvent::Browse { partial_path, .. } = event {
                    capture.borrow_mut().push(partial_path.clone());
                }
            })
        });

        cx.update_window(handle, |_, window, cx| {
            let request_id = picker.update(cx, |picker, cx| picker.open("  ", window, cx));
            assert_ne!(request_id, 0);
            assert_eq!(picker.read(cx).active_partial_path.as_deref(), Some("~"));

            picker.update(cx, |picker, cx| {
                picker.path.update(cx, |state, cx| state.set_value("  ", window, cx));
                assert_eq!(picker.browse(window, cx), 0);
            });
            assert!(picker.read(cx).active_request_id.is_none());
        })
        .unwrap();

        assert_eq!(&*browse_paths.borrow(), &["~"]);
    }

    #[gpui_kit::test]
    fn server_verified_folder_is_selected_and_old_dialog_results_are_ignored(
        cx: &mut TestAppContext,
    ) {
        let (handle, picker) = picker(cx);
        let events = Rc::new(RefCell::new(Vec::new()));
        let capture = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&picker, move |_, event: &DirectoryPickerEvent, _| {
                capture.borrow_mut().push(match event {
                    DirectoryPickerEvent::Browse { id, partial_path } => {
                        DirectoryPickerEvent::Browse { id: *id, partial_path: partial_path.clone() }
                    }
                    DirectoryPickerEvent::Select { path, title } => {
                        DirectoryPickerEvent::Select { path: path.clone(), title: title.clone() }
                    }
                    DirectoryPickerEvent::Cancel => DirectoryPickerEvent::Cancel,
                });
            })
        });

        cx.update_window(handle, |_, window, cx| {
            let old_id = picker.update(cx, |picker, cx| picker.open("/remote/old", window, cx));
            let current_id =
                picker.update(cx, |picker, cx| picker.open("/remote/project", window, cx));
            assert_ne!(old_id, current_id);
            assert!(!picker.update(cx, |picker, cx| {
                picker.apply_result(
                    old_id,
                    Ok(WorkspaceBrowseResult {
                        parent_path: "/remote/old".into(),
                        entries: Vec::new(),
                    }),
                    window,
                    cx,
                )
            }));
            assert!(picker.update(cx, |picker, cx| {
                picker.apply_result(
                    current_id,
                    Ok(WorkspaceBrowseResult {
                        parent_path: "/remote/project".into(),
                        entries: vec![WorkspaceBrowseEntry {
                            name: "src".into(),
                            full_path: "/remote/project/src".into(),
                        }],
                    }),
                    window,
                    cx,
                )
            }));
            window.render_frame(cx);
            window.click("directory-picker-choose", cx);
            assert!(!picker.read(cx).is_open());

            let closed_id =
                picker.update(cx, |picker, cx| picker.open("/remote/closed", window, cx));
            picker.update(cx, |picker, cx| picker.close(cx));
            let reopened_id =
                picker.update(cx, |picker, cx| picker.open("/remote/reopened", window, cx));
            assert_ne!(closed_id, reopened_id);
            assert!(!picker.update(cx, |picker, cx| {
                picker.apply_result(
                    closed_id,
                    Ok(WorkspaceBrowseResult {
                        parent_path: "/remote/closed".into(),
                        entries: Vec::new(),
                    }),
                    window,
                    cx,
                )
            }));
            assert!(picker.update(cx, |picker, cx| {
                picker.apply_result(
                    reopened_id,
                    Ok(WorkspaceBrowseResult {
                        parent_path: "/remote/reopened".into(),
                        entries: Vec::new(),
                    }),
                    window,
                    cx,
                )
            }));
        })
        .unwrap();

        assert!(events.borrow().iter().any(|event| matches!(
            event,
            DirectoryPickerEvent::Select { path, title }
                if path == "/remote/project" && title == "project"
        )));
    }
}
