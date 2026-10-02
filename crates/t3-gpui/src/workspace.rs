//! Remote workspace browser and change viewer.

use std::collections::HashMap;

use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{
    WorkspaceDiffPreview, WorkspaceDirectory, WorkspaceGitStatus, WorkspaceRefs, WorkspaceRequest,
    WorkspaceResponse, WorkspaceTerminalEvent,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceScope {
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceTab {
    #[default]
    Files,
    Changes,
    Terminal,
}

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    Request { request_id: u64, scope: WorkspaceScope, request: WorkspaceRequest },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RequestSlot {
    Directory,
    File,
    Status,
    Refs,
    Diff,
    Terminal,
    TerminalWrite,
    TerminalClose,
    BranchSwitch,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    scope_epoch: u64,
    slot: RequestSlot,
}

fn response_is_current(
    pending: Option<&PendingRequest>,
    current_scope_epoch: u64,
    scope_matches: bool,
    latest_request_id: Option<u64>,
    request_id: u64,
) -> bool {
    pending.is_some_and(|pending| {
        pending.scope_epoch == current_scope_epoch
            && scope_matches
            && latest_request_id == Some(request_id)
    })
}

pub struct WorkspacePanel {
    scope: WorkspaceScope,
    scope_epoch: u64,
    next_request_id: u64,
    pending: HashMap<u64, PendingRequest>,
    latest: HashMap<RequestSlot, u64>,
    tab: WorkspaceTab,
    directory_path: String,
    directory: Option<WorkspaceDirectory>,
    selected_file: Option<String>,
    file_contents: Option<String>,
    file_truncated: bool,
    status: Option<WorkspaceGitStatus>,
    refs: Option<WorkspaceRefs>,
    diff: Option<WorkspaceDiffPreview>,
    selected_change: Option<String>,
    terminal_history: String,
    terminal_id: String,
    terminal_status: String,
    terminal_input: Entity<InputState>,
    terminal_open: bool,
    terminal_wanted: bool,
    connected: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspaceEvent> for WorkspacePanel {}

impl WorkspacePanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let terminal_input = cx.new(|cx| InputState::new(window, cx).placeholder("Run command…"));
        let subscriptions = vec![cx.subscribe_in(
            &terminal_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    this.send_terminal_input(window, cx);
                }
            },
        )];
        Self {
            scope: WorkspaceScope::default(),
            scope_epoch: 0,
            next_request_id: 1,
            pending: HashMap::new(),
            latest: HashMap::new(),
            tab: WorkspaceTab::Files,
            directory_path: String::new(),
            directory: None,
            selected_file: None,
            file_contents: None,
            file_truncated: false,
            status: None,
            refs: None,
            diff: None,
            selected_change: None,
            terminal_history: String::new(),
            terminal_id: "term-1".into(),
            terminal_status: "closed".into(),
            terminal_input,
            terminal_open: false,
            terminal_wanted: false,
            connected: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Selects the remote project/thread context and starts the initial directory/status reads.
    pub fn set_scope(&mut self, scope: WorkspaceScope, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.scope_epoch = self.scope_epoch.wrapping_add(1);
        self.pending.clear();
        self.latest.clear();
        self.directory_path.clear();
        self.directory = None;
        self.selected_file = None;
        self.file_contents = None;
        self.file_truncated = false;
        self.status = None;
        self.refs = None;
        self.diff = None;
        self.selected_change = None;
        self.terminal_history.clear();
        self.terminal_open = false;
        self.terminal_wanted = false;
        self.error = None;
        if let Some(cwd) = self.scope.cwd.clone() {
            self.request(
                WorkspaceRequest::ListDirectory { cwd: cwd.clone(), directory_path: None },
                RequestSlot::Directory,
                cx,
            );
            self.request(WorkspaceRequest::GitStatus { cwd: cwd.clone() }, RequestSlot::Status, cx);
            self.request(WorkspaceRequest::ListRefs { cwd }, RequestSlot::Refs, cx);
        }
        cx.notify();
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected == connected {
            return;
        }
        self.connected = connected;
        if connected {
            self.refresh_directory(cx);
            if let Some(cwd) = self.scope.cwd.clone() {
                self.request(
                    WorkspaceRequest::GitStatus { cwd: cwd.clone() },
                    RequestSlot::Status,
                    cx,
                );
                self.request(WorkspaceRequest::ListRefs { cwd }, RequestSlot::Refs, cx);
            }
            if self.terminal_wanted && self.tab == WorkspaceTab::Terminal {
                self.open_terminal(cx);
            }
        } else {
            self.pending.clear();
            self.latest.clear();
            self.terminal_open = false;
        }
        cx.notify();
    }

    pub fn select_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.error = None;
        match tab {
            WorkspaceTab::Files if self.directory.is_none() => self.refresh_directory(cx),
            WorkspaceTab::Changes => self.refresh_changes(cx),
            WorkspaceTab::Terminal => self.open_terminal(cx),
            _ => {}
        }
        cx.notify();
    }

    pub fn apply_result(
        &mut self,
        request_id: u64,
        scope: &WorkspaceScope,
        result: Result<WorkspaceResponse, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending.remove(&request_id) else {
            return;
        };
        if !response_is_current(
            Some(&pending),
            self.scope_epoch,
            scope == &self.scope,
            self.latest.get(&pending.slot).copied(),
            request_id,
        ) {
            return;
        }
        match result {
            Ok(WorkspaceResponse::Directory(directory))
                if pending.slot == RequestSlot::Directory =>
            {
                self.directory = Some(directory);
            }
            Ok(WorkspaceResponse::File(file)) if pending.slot == RequestSlot::File => {
                self.selected_file = Some(file.relative_path);
                self.file_contents = Some(file.contents);
                self.file_truncated = file.truncated;
                self.tab = WorkspaceTab::Files;
            }
            Ok(WorkspaceResponse::GitStatus(status)) if pending.slot == RequestSlot::Status => {
                self.status = Some(status)
            }
            Ok(WorkspaceResponse::Refs(refs)) if pending.slot == RequestSlot::Refs => {
                self.refs = Some(refs)
            }
            Ok(WorkspaceResponse::DiffPreview(diff)) if pending.slot == RequestSlot::Diff => {
                self.diff = Some(diff);
                self.tab = WorkspaceTab::Changes;
            }
            Ok(WorkspaceResponse::Terminal(terminal)) if pending.slot == RequestSlot::Terminal => {
                self.terminal_id = terminal.terminal_id;
                self.terminal_status = terminal.status;
                self.set_terminal_history(&terminal.history);
                self.terminal_open = true;
                self.tab = WorkspaceTab::Terminal;
            }
            Ok(WorkspaceResponse::Ack) if pending.slot == RequestSlot::TerminalClose => {
                self.terminal_open = false;
                self.terminal_wanted = false;
                self.terminal_history.clear();
            }
            Ok(WorkspaceResponse::Ack) if pending.slot == RequestSlot::BranchSwitch => {
                self.latest.remove(&RequestSlot::File);
                self.pending.retain(|_, pending| pending.slot != RequestSlot::File);
                self.selected_file = None;
                self.file_contents = None;
                self.file_truncated = false;
                self.directory_path.clear();
                self.refresh_directory(cx);
                self.refresh_changes(cx);
            }
            Ok(WorkspaceResponse::Ack) => {}
            Ok(_) => {
                self.error = Some("The server returned an unexpected workspace response.".into())
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn refresh_directory(&mut self, cx: &mut Context<Self>) {
        if let Some(cwd) = self.scope.cwd.clone() {
            self.request(
                WorkspaceRequest::ListDirectory {
                    cwd,
                    directory_path: (!self.directory_path.is_empty())
                        .then(|| self.directory_path.clone()),
                },
                RequestSlot::Directory,
                cx,
            );
        }
    }

    fn refresh_changes(&mut self, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd.clone() else {
            return;
        };
        self.selected_change = None;
        self.request(WorkspaceRequest::GitStatus { cwd: cwd.clone() }, RequestSlot::Status, cx);
        self.request(WorkspaceRequest::ListRefs { cwd: cwd.clone() }, RequestSlot::Refs, cx);
        self.request(WorkspaceRequest::DiffPreview { cwd }, RequestSlot::Diff, cx);
    }

    fn switch_ref(&mut self, ref_name: String, cx: &mut Context<Self>) {
        let Some(cwd) = self.scope.cwd.clone() else {
            return;
        };
        self.request(WorkspaceRequest::SwitchRef { cwd, ref_name }, RequestSlot::BranchSwitch, cx);
    }

    fn open_terminal(&mut self, cx: &mut Context<Self>) {
        self.terminal_wanted = true;
        if self.terminal_open {
            return;
        }
        let (Some(cwd), Some(thread_id)) = (self.scope.cwd.clone(), self.scope.thread_id.clone())
        else {
            self.error = Some("Open a thread to use its remote terminal.".into());
            return;
        };
        self.request(
            WorkspaceRequest::OpenTerminal {
                cwd,
                thread_id,
                terminal_id: self.terminal_id.clone(),
            },
            RequestSlot::Terminal,
            cx,
        );
    }

    fn restart_terminal(&mut self, cx: &mut Context<Self>) {
        let (Some(cwd), Some(thread_id)) = (self.scope.cwd.clone(), self.scope.thread_id.clone())
        else {
            self.error = Some("Open a thread to use its remote terminal.".into());
            return;
        };
        self.terminal_wanted = true;
        self.request(
            WorkspaceRequest::RestartTerminal {
                cwd,
                thread_id,
                terminal_id: self.terminal_id.clone(),
            },
            RequestSlot::Terminal,
            cx,
        );
    }

    pub fn apply_terminal_event(
        &mut self,
        scope: &WorkspaceScope,
        event: WorkspaceTerminalEvent,
        cx: &mut Context<Self>,
    ) {
        // The backend drops the attach subscription on close/thread switch.
        // Events already queued in the UI channel can still arrive afterward.
        if scope != &self.scope || !self.connected || !self.terminal_open {
            return;
        }
        match event {
            WorkspaceTerminalEvent::Snapshot { snapshot }
            | WorkspaceTerminalEvent::Restarted { snapshot } => {
                if snapshot.thread_id != self.scope.thread_id.as_deref().unwrap_or_default()
                    || snapshot.terminal_id != self.terminal_id
                {
                    return;
                }
                self.set_terminal_history(&snapshot.history);
                self.terminal_status = snapshot.status;
                self.terminal_open = true;
            }
            WorkspaceTerminalEvent::Output { thread_id, terminal_id, data } => {
                if self.matches_terminal(&thread_id, &terminal_id) {
                    self.terminal_status = "running".into();
                    self.append_terminal(&data);
                }
            }
            WorkspaceTerminalEvent::Exited { thread_id, terminal_id, exit_code, exit_signal } => {
                if self.matches_terminal(&thread_id, &terminal_id) {
                    self.terminal_status = "exited".into();
                    self.append_terminal(&format!(
                        "\r\n[process exited: code={exit_code:?}, signal={exit_signal:?}]\r\n"
                    ));
                }
            }
            WorkspaceTerminalEvent::Closed { thread_id, terminal_id } => {
                if self.matches_terminal(&thread_id, &terminal_id) {
                    self.terminal_open = false;
                    self.terminal_wanted = false;
                    self.terminal_status = "closed".into();
                }
            }
            WorkspaceTerminalEvent::Error { thread_id, terminal_id, message } => {
                if self.matches_terminal(&thread_id, &terminal_id) {
                    self.terminal_status = "error".into();
                    self.error = Some(message);
                }
            }
            WorkspaceTerminalEvent::Cleared { thread_id, terminal_id } => {
                if self.matches_terminal(&thread_id, &terminal_id) {
                    self.terminal_history.clear();
                }
            }
            WorkspaceTerminalEvent::Activity { .. } => {}
        }
        cx.notify();
    }

    fn matches_terminal(&self, thread_id: &str, terminal_id: &str) -> bool {
        self.scope.thread_id.as_deref() == Some(thread_id) && self.terminal_id == terminal_id
    }

    fn append_terminal(&mut self, data: &str) {
        const MAX_TERMINAL_CHARS: usize = 250_000;
        self.terminal_history.push_str(data);
        if self.terminal_history.len() > MAX_TERMINAL_CHARS {
            let mut start = self.terminal_history.len() - MAX_TERMINAL_CHARS;
            while !self.terminal_history.is_char_boundary(start) {
                start += 1;
            }
            self.terminal_history.drain(..start);
        }
    }

    fn set_terminal_history(&mut self, history: &str) {
        self.terminal_history.clear();
        self.append_terminal(history);
    }

    fn send_terminal_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terminal_open {
            return;
        }
        let data = self.terminal_input.read(cx).value().to_string();
        if data.is_empty() {
            return;
        }
        if data.len() + 1 > 65_536 {
            self.error = Some("Terminal input exceeds the 64 KiB server limit.".into());
            cx.notify();
            return;
        }
        let Some(thread_id) = self.scope.thread_id.clone() else {
            return;
        };
        if self.scope.cwd.is_none() {
            return;
        }
        self.terminal_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.request(
            WorkspaceRequest::WriteTerminal {
                thread_id,
                terminal_id: self.terminal_id.clone(),
                data: format!("{data}\n"),
            },
            RequestSlot::TerminalWrite,
            cx,
        );
    }

    fn close_terminal(&mut self, cx: &mut Context<Self>) {
        self.terminal_wanted = false;
        let Some(thread_id) = self.scope.thread_id.clone() else {
            return;
        };
        self.request(
            WorkspaceRequest::CloseTerminal {
                thread_id,
                terminal_id: Some(self.terminal_id.clone()),
            },
            RequestSlot::TerminalClose,
            cx,
        );
    }

    fn open_path(&mut self, path: String, is_directory: bool, cx: &mut Context<Self>) {
        if is_directory {
            self.directory_path = path;
            self.directory = None;
            self.file_contents = None;
            self.file_truncated = false;
            self.refresh_directory(cx);
        } else if let Some(cwd) = self.scope.cwd.clone() {
            self.selected_file = Some(path.clone());
            self.file_contents = None;
            self.file_truncated = false;
            self.request(
                WorkspaceRequest::ReadFile { cwd, relative_path: path },
                RequestSlot::File,
                cx,
            );
        }
    }

    fn request(&mut self, request: WorkspaceRequest, slot: RequestSlot, cx: &mut Context<Self>) {
        let Some(_) = self.scope.cwd.as_ref().filter(|_| self.connected) else {
            return;
        };
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.pending.insert(request_id, PendingRequest { scope_epoch: self.scope_epoch, slot });
        self.latest.insert(slot, request_id);
        self.error = None;
        cx.emit(WorkspaceEvent::Request { request_id, scope: self.scope.clone(), request });
    }

    fn is_loading(&self, slot: RequestSlot) -> bool {
        self.latest.get(&slot).is_some_and(|request_id| self.pending.contains_key(request_id))
    }
}

impl Render for WorkspacePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = [WorkspaceTab::Files, WorkspaceTab::Changes, WorkspaceTab::Terminal]
            .into_iter()
            .map(|tab| {
                let label = match tab {
                    WorkspaceTab::Files => "Files",
                    WorkspaceTab::Changes => "Changes",
                    WorkspaceTab::Terminal => "Terminal",
                };
                Button::new(label)
                    .small()
                    .when(self.tab == tab, |button| button.primary())
                    .disabled(!self.connected)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_tab(tab, cx)))
            });
        let mut body = v_flex().gap_2().flex_1();
        match self.tab {
            WorkspaceTab::Files => {
                let mut rows = Vec::new();
                if !self.directory_path.is_empty() {
                    rows.push(
                        Button::new("workspace-parent")
                            .small()
                            .ghost()
                            .label("..  Parent directory")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.directory_path = parent_path(&this.directory_path);
                                this.directory = None;
                                this.refresh_directory(cx);
                            }))
                            .into_any_element(),
                    );
                }
                if let Some(directory) = &self.directory {
                    for entry in &directory.entries {
                        let path = entry.path.clone();
                        let is_dir = entry.kind == "directory";
                        let title = format!(
                            "{}{}",
                            if is_dir { "▸  " } else { "    " },
                            entry.path.rsplit('/').next().unwrap_or(&entry.path)
                        );
                        rows.push(
                            Button::new(SharedString::from(format!("workspace-entry-{path}")))
                                .small()
                                .ghost()
                                .label(title)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_path(path.clone(), is_dir, cx)
                                }))
                                .into_any_element(),
                        );
                    }
                    if directory.truncated {
                        rows.push(
                            div()
                                .text_sm()
                                .child("Directory listing truncated.")
                                .into_any_element(),
                        );
                    }
                } else if self.scope.cwd.is_none() {
                    rows.push(
                        div()
                            .text_sm()
                            .child("Select a project to browse its files.")
                            .into_any_element(),
                    );
                } else if self.is_loading(RequestSlot::Directory) {
                    rows.push(div().text_sm().child("Loading files…").into_any_element());
                }
                if let Some(path) = &self.selected_file {
                    rows.push(
                        div()
                            .mt_3()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(path.clone())
                            .into_any_element(),
                    );
                    if let Some(contents) = &self.file_contents {
                        if self.file_truncated {
                            rows.push(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("File preview truncated by the server.")
                                    .into_any_element(),
                            );
                        }
                        rows.push(
                            div()
                                .mt_1()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().secondary)
                                .text_sm()
                                .font_family("monospace")
                                .child(contents.clone())
                                .into_any_element(),
                        );
                    }
                }
                body = body.children(rows);
            }
            WorkspaceTab::Changes => {
                let mut rows = Vec::new();
                if let Some(refs) = &self.refs {
                    if refs.is_repo {
                        rows.push(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Local branches")
                                .into_any_element(),
                        );
                        for reference in refs.refs.iter().filter(|reference| !reference.is_remote) {
                            let name = reference.name.clone();
                            rows.push(
                                Button::new(SharedString::from(format!("workspace-ref-{name}")))
                                    .small()
                                    .ghost()
                                    .label(format!(
                                        "{} {}",
                                        if reference.current { "●" } else { "○" },
                                        name
                                    ))
                                    .disabled(!self.connected || reference.current)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.switch_ref(name.clone(), cx)
                                    }))
                                    .into_any_element(),
                            );
                        }
                    }
                }
                if let Some(status) = &self.status {
                    if !status.is_repo {
                        rows.push(
                            div()
                                .text_sm()
                                .child("This workspace is not a Git repository.")
                                .into_any_element(),
                        );
                    } else {
                        rows.push(
                            div()
                                .text_sm()
                                .child(format!(
                                    "{}  +{}  −{}",
                                    status.ref_name.as_deref().unwrap_or("(detached)"),
                                    status.working_tree.insertions,
                                    status.working_tree.deletions
                                ))
                                .into_any_element(),
                        );
                        for file in &status.working_tree.files {
                            let path = file.path.clone();
                            rows.push(
                                Button::new(SharedString::from(format!("workspace-change-{path}")))
                                    .small()
                                    .ghost()
                                    .label(format!(
                                        "{}   +{} −{}",
                                        file.path, file.insertions, file.deletions
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let Some(cwd) = this.scope.cwd.clone() else {
                                            return;
                                        };
                                        this.selected_change = Some(path.clone());
                                        this.request(
                                            WorkspaceRequest::DiffFile { cwd, path: path.clone() },
                                            RequestSlot::Diff,
                                            cx,
                                        );
                                    }))
                                    .into_any_element(),
                            );
                        }
                    }
                } else {
                    rows.push(
                        div()
                            .text_sm()
                            .child(if self.is_loading(RequestSlot::Status) {
                                "Loading changes…"
                            } else {
                                "Changes will appear here."
                            })
                            .into_any_element(),
                    );
                }
                if let Some(diff) = &self.diff {
                    if let Some(path) = &self.selected_change {
                        rows.push(
                            div()
                                .mt_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(path.clone())
                                .into_any_element(),
                        );
                    }
                    for source in &diff.sources {
                        rows.push(
                            div()
                                .mt_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(source.title.clone())
                                .into_any_element(),
                        );
                        rows.push(
                            div()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().secondary)
                                .text_xs()
                                .font_family("monospace")
                                .child(source.diff.clone())
                                .into_any_element(),
                        );
                    }
                }
                body = body.children(rows);
            }
            WorkspaceTab::Terminal => {
                body = body.child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Remote terminal · {}", self.terminal_id)),
                        )
                        .child(if matches!(self.terminal_status.as_str(), "exited" | "error") {
                            Button::new("terminal-restart")
                                .small()
                                .ghost()
                                .disabled(!self.connected)
                                .label("Restart")
                                .on_click(cx.listener(|this, _, _, cx| this.restart_terminal(cx)))
                        } else if self.terminal_open {
                            Button::new("terminal-close")
                                .small()
                                .ghost()
                                .disabled(!self.connected)
                                .label("Close")
                                .on_click(cx.listener(|this, _, _, cx| this.close_terminal(cx)))
                        } else {
                            Button::new("terminal-open")
                                .small()
                                .ghost()
                                .disabled(!self.connected)
                                .label("Open terminal")
                                .on_click(cx.listener(|this, _, _, cx| this.open_terminal(cx)))
                        }),
                );
                if self.terminal_open && self.terminal_status == "running" {
                    body = body
                        .child(Input::new(&self.terminal_input).w_full().disabled(!self.connected));
                }
                if self.terminal_open && self.terminal_status != "running" {
                    body = body.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Terminal status: {}", self.terminal_status)),
                    );
                }
                body = body.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .text_sm()
                        .font_family("monospace")
                        .child(self.terminal_history.clone()),
                );
            }
        }
        if let Some(error) = &self.error {
            body = body.child(div().text_sm().child(error.clone()));
        }
        v_flex()
            .size_full()
            .gap_2()
            .p_3()
            .child(h_flex().gap_1().children(tabs))
            .child(body.overflow_y_scrollbar())
    }
}

fn parent_path(path: &str) -> String {
    path.rsplit_once('/').map(|(parent, _)| parent.to_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    #[gpui_kit::test]
    fn workspace_responses_from_old_context_are_ignored(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, panel) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| WorkspacePanel::new(window, cx)),
            )
            .unwrap()
        });
        let emitted = Rc::new(RefCell::new(Vec::<WorkspaceEvent>::new()));
        let capture = emitted.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&panel, move |_, event: &WorkspaceEvent, _| {
                capture.borrow_mut().push(event.clone());
            })
        });

        let old_scope = WorkspaceScope {
            project_id: Some("p1".into()),
            thread_id: Some("t1".into()),
            cwd: Some("/old".into()),
        };
        let current_scope = WorkspaceScope {
            project_id: Some("p2".into()),
            thread_id: Some("t2".into()),
            cwd: Some("/current".into()),
        };
        cx.update_window(handle, |_, window, cx| {
            panel.update(cx, |panel, cx| panel.set_connected(true, cx));
            panel.update(cx, |panel, cx| panel.set_scope(old_scope.clone(), cx));
            let old_id = *panel.read(cx).latest.get(&RequestSlot::Directory).unwrap();
            panel.update(cx, |panel, cx| panel.set_scope(current_scope.clone(), cx));
            let current_id = *panel.read(cx).latest.get(&RequestSlot::Directory).unwrap();

            let response: WorkspaceResponse = WorkspaceResponse::Directory(WorkspaceDirectory {
                entries: vec![t3_client::WorkspaceEntry {
                    path: "current.txt".into(),
                    kind: "file".into(),
                    ignored: false,
                }],
                truncated: false,
            });
            panel.update(cx, |panel, cx| {
                panel.apply_result(old_id, &old_scope, Ok(response.clone()), cx)
            });
            assert!(panel.read(cx).directory.is_none());

            panel.update(cx, |panel, cx| {
                panel.apply_result(current_id, &current_scope, Ok(response), cx)
            });
            assert_eq!(panel.read(cx).directory.as_ref().unwrap().entries[0].path, "current.txt");

            panel.update(cx, |panel, cx| panel.open_path("current.txt".into(), false, cx));
            let file_request_id = *panel.read(cx).latest.get(&RequestSlot::File).unwrap();
            panel.update(cx, |panel, cx| {
                panel.apply_result(
                    file_request_id,
                    &current_scope,
                    Ok(WorkspaceResponse::File(t3_client::WorkspaceFile {
                        relative_path: "current.txt".into(),
                        contents: "read-only contents".into(),
                        byte_length: 19,
                        truncated: false,
                    })),
                    cx,
                )
            });
            assert_eq!(panel.read(cx).file_contents.as_deref(), Some("read-only contents"));

            panel.update(cx, |panel, cx| {
                panel.terminal_open = true;
                panel.terminal_input.update(cx, |input, cx| input.set_value("echo hi", window, cx));
                panel.send_terminal_input(window, cx);
            });
            let terminal_request_id =
                *panel.read(cx).latest.get(&RequestSlot::TerminalWrite).unwrap();
            panel.update(cx, |panel, cx| {
                panel.apply_result(
                    terminal_request_id,
                    &current_scope,
                    Ok(WorkspaceResponse::Ack),
                    cx,
                );
                panel.apply_terminal_event(
                    &current_scope,
                    WorkspaceTerminalEvent::Output {
                        thread_id: "t2".into(),
                        terminal_id: "term-1".into(),
                        data: "echo hi\r\n".into(),
                    },
                    cx,
                );
            });
            assert!(panel.read(cx).terminal_history.contains("echo hi"));

            panel.update(cx, |panel, cx| {
                panel.select_tab(WorkspaceTab::Terminal, cx);
                panel.set_connected(false, cx);
                assert!(!panel.terminal_open);
                assert!(panel.terminal_wanted);
                panel.set_connected(true, cx);
            });
        })
        .unwrap();
        let requests = emitted.borrow();
        let directory_requests: Vec<_> = requests
            .iter()
            .filter_map(|event| match event {
                WorkspaceEvent::Request {
                    request_id,
                    scope,
                    request: WorkspaceRequest::ListDirectory { .. },
                } => Some((*request_id, scope.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(directory_requests.len(), 3);
        assert_ne!(directory_requests[0].0, directory_requests[1].0);
        assert_ne!(directory_requests[1].0, directory_requests[2].0);
        assert_eq!(directory_requests[0].1, old_scope);
        assert_eq!(directory_requests[1].1, current_scope);
        assert_eq!(directory_requests[2].1, current_scope);
        assert!(requests.iter().any(|event| matches!(
            event,
            WorkspaceEvent::Request {
                request: WorkspaceRequest::WriteTerminal { data, .. },
                ..
            } if data == "echo hi\n"
        )));
        assert!(requests.iter().any(|event| matches!(
            event,
            WorkspaceEvent::Request { request: WorkspaceRequest::OpenTerminal { .. }, .. }
        )));
    }

    #[::core::prelude::v1::test]
    fn response_guard_rejects_old_scope_and_superseded_request() {
        let pending = PendingRequest { scope_epoch: 1, slot: RequestSlot::Directory };
        assert!(!response_is_current(Some(&pending), 2, true, Some(7), 7));
        assert!(!response_is_current(Some(&pending), 1, true, Some(8), 7));
        assert!(!response_is_current(Some(&pending), 1, false, Some(7), 7));
        assert!(response_is_current(Some(&pending), 1, true, Some(7), 7));
    }

    #[test]
    fn terminal_event_contract_decodes_snapshot_and_output() {
        let snapshot: WorkspaceTerminalEvent = serde_json::from_value(json!({
            "type": "snapshot",
            "snapshot": { "threadId": "thread-1", "terminalId": "term-1", "cwd": "/workspace", "status": "running", "history": "ready", "label": "shell" }
        })).unwrap();
        assert!(matches!(snapshot, WorkspaceTerminalEvent::Snapshot { .. }));

        let output: WorkspaceTerminalEvent = serde_json::from_value(json!({
            "type": "output", "threadId": "thread-1", "terminalId": "term-1", "sequence": 2, "data": "hello"
        })).unwrap();
        assert!(matches!(output, WorkspaceTerminalEvent::Output { data, .. } if data == "hello"));
    }
}
