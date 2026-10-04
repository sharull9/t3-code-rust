//! Window root: title bar, thread sidebar, connection status and the open thread.

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::assets::IconName;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size, StyledExt as _, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use t3_client::{ProjectShell, ShellState, ThreadShell};

use crate::attachments::{AttachmentPanel, AttachmentPanelEvent};
use crate::backend::{Backend, Command, Event, Status};
use crate::directory_picker::{DirectoryPicker, DirectoryPickerEvent};
use crate::drafts::{DraftStore, DraftThread};
use crate::info_panel::{INFO_PANEL_WIDTH, InfoPanel, InfoPanelEvent, InfoScope};
use crate::script_dialog::{ScriptDialog, ScriptDialogEvent};
use crate::project_picker::{ProjectPicker, ProjectPickerEvent};
use crate::settings::{SettingsEvent, SettingsPage};
use crate::sidebar::{Sidebar, SidebarEvent};
use crate::thread_view::{ThreadView, ThreadViewEvent};
use crate::ui::{self, SIDEBAR_WIDTH, icon};
use crate::usage::{UsageEvent, UsageView};
use crate::user_input::UserInputPanel;
use crate::workspace::{WorkspaceEvent, WorkspacePanel, WorkspaceScope, WorkspaceTab};

/// Workspace-result scope that routes `@` file searches back to the composer.
const COMPOSER_FILES_SCOPE: &str = "__composer_files";

/// Compatibility fallback when neither the project, current thread nor server
/// config supplies a model. Prefer server-advertised models above this value.
fn fallback_model_selection() -> serde_json::Value {
    json!({ "instanceId": "claudeAgent", "model": "claude-fable-5-1" })
}

gpui_kit::actions!(
    t3_app,
    [
        NewThread,
        ToggleSidebar,
        FocusComposer,
        ToggleWorkspace,
        ToggleInfo,
        ShowSettings,
        DismissModal,
        UndoThreadAction
    ]
);
pub fn init(cx: &mut App) {
    crate::settings::init(cx);
    // The user can rebind these; see `keymap`.
    crate::keymap::apply(cx);
}
pub struct T3App {
    focus_handle: FocusHandle,
    /// Whether the light palette is currently applied.
    light_applied: bool,
    backend: Backend,
    status: Status,
    error: Option<SharedString>,
    shell: ShellState,
    thread: Option<Entity<ThreadView>>,
    pairing_link: Entity<InputState>,
    sidebar: Entity<Sidebar>,
    sidebar_open: bool,
    project_picker: Entity<ProjectPicker>,
    /// A thread just dispatched via `thread.create`, opened as soon as its
    /// shell entry streams in (see `handle_event`'s `Event::Shell` arm).
    pending_new_thread_id: Option<String>,
    /// Threads starting in a new worktree, by id, until the agent has their
    /// first message. Kept here because the draft's view is replaced by the
    /// thread's own once it streams in.
    worktree_setups: HashMap<String, crate::worktree_setup::WorktreeSetup>,
    /// Pairing screen opened by hand while already paired, to switch servers.
    switching_server: bool,
    pairing_pending: bool,
    providers: Vec<t3_client::ServerProvider>,
    drafts: HashMap<String, String>,
    /// New threads being composed. None exists on the server until its first
    /// message is sent; the composer text lives in `drafts` by the same ID.
    draft_threads: Vec<DraftThread>,
    draft_store: DraftStore,
    active_server: Option<String>,
    active_environment: String,
    environments: HashMap<String, String>,
    server_keys: HashMap<String, String>,
    draft_save_task: Option<Task<()>>,
    attachment_panels: HashMap<String, Entity<AttachmentPanel>>,
    workspace: Entity<WorkspacePanel>,
    workspace_open: bool,
    /// The floating Workspace / Version Control card.
    info: Entity<InfoPanel>,
    info_open: bool,
    script_dialog: Entity<ScriptDialog>,
    directory_picker: Entity<DirectoryPicker>,
    settings: Entity<SettingsPage>,
    usage: Entity<UsageView>,
    /// The usage page replaces the open thread in the main column.
    usage_open: bool,
    question_panels: HashMap<String, Entity<UserInputPanel>>,
    sending: HashSet<String>,
    _thread_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl T3App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (backend, events) = Backend::spawn();
        Self::new_with_backend(backend, events, window, cx)
    }

    /// Re-applies the theme when preferences or (in System mode) the OS
    /// appearance change, and redraws views that cache their last frame.
    fn appearance_subscriptions(
        mut subscriptions: Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        subscriptions.push(cx.observe_global::<crate::prefs::Prefs>(|this, cx| {
            let light = ui::is_light(crate::prefs::Prefs::global(cx).theme, cx.window_appearance());
            if light != this.light_applied {
                this.light_applied = light;
                ui::apply_theme(light, cx);
            }
            if let Some(thread) = &this.thread {
                thread.update(cx, |thread, cx| thread.refresh_appearance(cx));
            }
            cx.notify();
        }));
        subscriptions.push(window.observe_window_appearance(|window, cx| {
            let mode = crate::prefs::Prefs::global(cx).theme;
            if mode == crate::prefs::ThemeMode::System {
                ui::apply_theme(ui::is_light(mode, window.appearance()), cx);
                cx.refresh_windows();
            }
        }));
        subscriptions
    }

    fn new_with_backend(
        backend: Backend,
        events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::listen(events, window, cx);

        let pairing_link =
            cx.new(|cx| InputState::new(window, cx).placeholder("Pairing link or token"));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        let project_picker = cx.new(|cx| ProjectPicker::new(window, cx));
        let workspace = cx.new(|cx| WorkspacePanel::new(window, cx));
        let info = cx.new(|cx| InfoPanel::new(window, cx));
        let script_dialog = cx.new(|cx| ScriptDialog::new(window, cx));
        let directory_picker = cx.new(|cx| DirectoryPicker::new(window, cx));
        let settings = cx.new(|cx| SettingsPage::new(window, cx));
        let usage = cx.new(UsageView::new);
        cx.on_release(|this, cx| {
            this.capture_current_drafts(cx);
            this.backend.flush_drafts(this.draft_store.clone());
        })
        .detach();
        let subscriptions = vec![
            cx.subscribe_in(
                &directory_picker,
                window,
                |this, _, event: &DirectoryPickerEvent, _, cx| match event {
                    DirectoryPickerEvent::Browse { id, partial_path } => {
                        this.backend.send(Command::Workspace {
                            request_id: *id,
                            scope: WorkspaceScope {
                                project_id: Some("__directory_picker".into()),
                                ..Default::default()
                            },
                            request: t3_client::WorkspaceRequest::BrowseDirectories {
                                partial_path: partial_path.clone(),
                                cwd: this
                                    .current_project(cx)
                                    .map(|project| project.workspace_root.clone())
                                    .filter(|path| !path.trim().is_empty()),
                            },
                        })
                    }
                    DirectoryPickerEvent::Select { path, title } => {
                        this.backend.send(Command::CreateProject {
                            id: t3_client::new_id(),
                            title: title.clone(),
                            workspace_root: path.clone(),
                        })
                    }
                    DirectoryPickerEvent::Cancel => {}
                },
            ),
            cx.subscribe_in(&settings, window, |this, _, event: &SettingsEvent, window, cx| {
                match event {
                    SettingsEvent::Close => this.set_settings_open(false, window, cx),
                    SettingsEvent::RefreshProviders => this.backend.send(Command::RefreshConfig),
                    SettingsEvent::SwitchServer => {
                        this.settings.update(cx, |settings, cx| settings.set_open(false, cx));
                        this.switching_server = true;
                        this.pairing_link.update(cx, |input, cx| input.focus(window, cx));
                    }
                    SettingsEvent::UpdateServerSettings { request_id, patch, base } => {
                        this.backend.send(Command::UpdateSettings {
                            request_id: *request_id,
                            patch: patch.clone(),
                            base: base.clone(),
                        })
                    }
                    SettingsEvent::UpdateKeybindings { request_id, ops } => {
                        this.backend.send(Command::UpdateKeybindings {
                            request_id: *request_id,
                            ops: ops.clone(),
                        })
                    }
                    SettingsEvent::LoadArchived(request_id) => {
                        this.backend.send(Command::LoadArchived(request_id.clone()))
                    }
                    SettingsEvent::ThreadAction { thread_id, action } => {
                        this.backend.send(Command::ThreadAction {
                            thread_id: thread_id.clone(),
                            action: action.clone(),
                        })
                    }
                    SettingsEvent::ChooseManagedServer => {
                        let paths = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Choose T3 server executable".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = paths.await {
                                if let Some(path) = paths.into_iter().next() {
                                    let _ = this.update(cx, |this, cx| this.start_local(Some(path), cx));
                                }
                            }
                        })
                        .detach();
                    }
                    SettingsEvent::StartLocalServer => this.start_local(None, cx),
                }
                cx.notify();
            }),
            cx.subscribe(&usage, |this, _, event: &UsageEvent, _| {
                match event {
                    UsageEvent::Load { request_id, window } => this.backend.send(Command::LoadUsage { request_id: *request_id, window: window.clone() }),
                    UsageEvent::LoadLimits { request_id } => this.backend.send(Command::LoadLimits { request_id: *request_id }),
                    UsageEvent::ConsumeResetCredit { key, input } => this.backend.send(Command::ConsumeResetCredit { key: key.clone(), input: input.clone() }),
                }
            }),
            cx.subscribe(&workspace, |this, _, event: &WorkspaceEvent, _| {
                let WorkspaceEvent::Request { request_id, scope, request } = event;
                this.backend.send(Command::Workspace {
                    request_id: *request_id,
                    scope: scope.clone(),
                    request: request.clone(),
                });
            }),
            // The git status arrives after `sync_info`; pass its branch on.
            cx.observe(&info, |this, _, cx| this.sync_draft_branch(cx)),
            cx.subscribe_in(&info, window, |this, _, event: &InfoPanelEvent, window, cx| {
                this.on_info_event(event, window, cx)
            }),
            cx.subscribe(&script_dialog, |this, _, event: &ScriptDialogEvent, cx| {
                let ScriptDialogEvent::Save { project_id, scripts } = event;
                this.info.update(cx, |info, cx| {
                    info.save_scripts(project_id.clone(), scripts.clone(), cx)
                });
            }),
            cx.subscribe_in(&pairing_link, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.pair(window, cx);
                }
            }),
            cx.subscribe_in(&sidebar, window, |this, _, event: &SidebarEvent, window, cx| {
                match event {
                    SidebarEvent::OpenThread(thread_id) => {
                        this.open_thread(thread_id.clone(), window, cx)
                    }
                    SidebarEvent::OpenDraft(draft_id) => {
                        this.open_draft(draft_id.clone(), window, cx)
                    }
                    SidebarEvent::DiscardDraft(draft_id) => this.discard_draft(draft_id, cx),
                    SidebarEvent::SwitchServer => {
                        this.set_settings_open(false, window, cx);
                        this.switching_server = true;
                        this.pairing_link.update(cx, |state, cx| state.focus(window, cx));
                        cx.notify();
                    }
                    SidebarEvent::OpenSettings => {
                        this.set_settings_open(true, window, cx);
                    }
                    SidebarEvent::ToggleUsage => {
                        this.set_settings_open(false, window, cx);
                        this.set_usage_open(!this.usage_open, cx);
                        if this.usage_open {
                            this.focus_handle.focus(window, cx);
                        } else if let Some(thread) = &this.thread {
                            thread.update(cx, |view, cx| view.focus_composer(window, cx));
                        }
                    }
                    SidebarEvent::AddProject => this.add_project(window, cx),
                    SidebarEvent::NewThread => {
                        let projects = crate::project_picker::recent_projects(&this.shell);
                        this.project_picker
                            .update(cx, |picker, cx| picker.open(projects, window, cx));
                    }
                    SidebarEvent::LoadArchived(request_id) => {
                        this.backend.send(Command::LoadArchived(request_id.clone()))
                    }
                    SidebarEvent::ThreadAction(thread_id, action) => {
                        this.backend.send(Command::ThreadAction {
                            thread_id: thread_id.clone(),
                            action: action.clone(),
                        });
                    }
                }
            }),
            cx.subscribe_in(
                &project_picker,
                window,
                |this, _, event: &ProjectPickerEvent, window, cx| match event {
                    ProjectPickerEvent::Select(project) => {
                        this.new_draft_in(project.id.clone(), None, None, window, cx)
                    }
                    ProjectPickerEvent::Cancel => {}
                },
            ),
        ];

        Self {
            focus_handle: cx.focus_handle(),
            light_applied: ui::is_light(crate::prefs::Prefs::global(cx).theme, window.appearance()),
            backend,
            status: Status::Connecting(String::new()),
            error: None,
            shell: ShellState::default(),
            thread: None,
            pairing_link,
            sidebar,
            sidebar_open: true,
            project_picker,
            pending_new_thread_id: None,
            worktree_setups: HashMap::new(),
            switching_server: false,
            pairing_pending: false,
            providers: Vec::new(),
            drafts: HashMap::new(),
            draft_threads: Vec::new(),
            draft_store: DraftStore::default(),
            active_server: None,
            active_environment: "default".into(),
            environments: HashMap::new(),
            server_keys: HashMap::new(),
            draft_save_task: None,
            attachment_panels: HashMap::new(),
            workspace,
            workspace_open: false,
            info,
            info_open: crate::prefs::Prefs::global(cx).info_panel_open,
            script_dialog,
            directory_picker,
            settings,
            usage,
            usage_open: false,
            question_panels: HashMap::new(),
            sending: HashSet::new(),
            _thread_subscription: None,
            _subscriptions: Self::appearance_subscriptions(subscriptions, window, cx),
        }
    }

    /// Bridges backend events onto the window: spawned *with* the window
    /// (rather than `cx.spawn`) so `handle_event` can open a thread as soon
    /// as it streams in, which needs one to create the `ThreadView`.
    fn listen(mut events: UnboundedReceiver<Event>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |app, window, cx| app.handle_event(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::AssetUrl(result) => match result {
                Ok(url) => cx.open_url(&url),
                Err(error) => self.error = Some(error.into()),
            },
            Event::Thumbnail { attachment_id, mime_type, result } => {
                crate::transcript::Thumbnails::finish(&attachment_id, &mime_type, result, cx);
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| view.refresh_thumbnails(cx));
                }
            }
            Event::EnvironmentId { server, id } => {
                self.environments.insert(crate::drafts::normalize_server_url(&server), id);
            }
            Event::EnvironmentKey { server, key } => {
                self.server_keys.insert(crate::drafts::normalize_server_url(&server), key);
            }
            Event::DraftsLoaded(store) => {
                self.draft_store = store;
            }
            Event::AttachmentUploaded { thread_id, local_id, request_id, result } => {
                if let Some(panel) = self.attachment_panels.get(&thread_id) {
                    panel.update(cx, |panel, cx| {
                        panel.complete_upload(&local_id, request_id, result, cx)
                    });
                }
            }
            Event::WorkspaceResult { request_id, scope, result } => {
                if scope.project_id.as_deref() == Some(COMPOSER_FILES_SCOPE) {
                    let result = result.and_then(|response| match response {
                        t3_client::WorkspaceResponse::Entries(directory) => Ok(directory.entries),
                        _ => Err("Unexpected file search response".into()),
                    });
                    if let Some(view) = self.thread.clone().filter(|view| {
                        scope.thread_id.as_deref() == Some(view.read(cx).thread_id())
                    }) {
                        view.update(cx, |view, cx| view.apply_file_results(request_id, result, cx));
                    }
                } else if scope.project_id.as_deref() == Some("__directory_picker") {
                    let result = result.and_then(|response| {
                        if let t3_client::WorkspaceResponse::BrowseDirectories(result) = response {
                            Ok(result)
                        } else {
                            Err("Unexpected folder response".into())
                        }
                    });
                    self.directory_picker.update(cx, |picker, cx| {
                        picker.apply_result(request_id, result, window, cx);
                    });
                } else {
                    self.workspace
                        .update(cx, |panel, cx| panel.apply_result(request_id, &scope, result, cx));
                }
            }
            Event::InfoResult { request_id, result } => {
                self.info.update(cx, |info, cx| info.apply_result(request_id, result, cx))
            }
            Event::Terminal { scope, item } => {
                self.workspace.update(cx, |panel, cx| panel.apply_terminal_event(&scope, item, cx))
            }
            Event::PairFinished(success) => {
                self.pairing_pending = false;
                if success {
                    self.capture_current_drafts(cx);
                    self.backend.send(Command::SaveDrafts(self.draft_store.clone()));
                    self.active_server = None;
                    self.attachment_panels.clear();
                    self.workspace
                        .update(cx, |panel, cx| panel.set_scope(WorkspaceScope::default(), cx));
                    self.pairing_link.update(cx, |state, cx| state.set_value("", window, cx));
                    self.switching_server = false;
                    self.thread = None;
                    self._thread_subscription = None;
                    self.drafts.clear();
                    self.draft_threads.clear();
                    self.question_panels.clear();
                    self.sending.clear();
                    self.providers.clear();
                    self.shell = ShellState::default();
                    self.pending_new_thread_id = None;
                    self.sidebar.update(cx, |sidebar, cx| {
                        sidebar.reset_environment(cx);
                        sidebar.set_shell(ShellState::default(), cx);
                    });
                    self.usage.update(cx, |usage, cx| usage.reset(cx));
                }
            }
            Event::NewThreadFinished { thread_id, success } => {
                if !success && self.pending_new_thread_id.as_deref() == Some(thread_id.as_str()) {
                    self.pending_new_thread_id = None;
                }
            }
            Event::Status(status) => {
                let connected = matches!(status, Status::Connected(_));
                self.settings.update(cx, |panel, cx| panel.set_connected(connected, cx));
                self.workspace.update(cx, |panel, cx| panel.set_connected(connected, cx));
                self.info.update(cx, |info, cx| info.set_connected(connected, cx));
                let usage_open = self.usage_open;
                self.usage.update(cx, |usage, cx| {
                    usage.set_connected(connected, cx);
                    if connected && usage_open {
                        usage.ensure_fresh(cx);
                    }
                });
                if !connected {
                    crate::keymap::set_server_keybindings(cx, Vec::new());
                    self.directory_picker.update(cx, |picker, cx| picker.close(cx));
                }
                if !connected {
                    self.sending.clear();
                }
                if let Status::Connected(server) = &status {
                    let url = crate::drafts::normalize_server_url(server);
                    let environment =
                        self.environments.get(&url).cloned().unwrap_or_else(|| "default".into());
                    let server = self.server_keys.get(&url).cloned().unwrap_or(url);
                    if self.active_server.as_deref() != Some(&server)
                        || self.active_environment != environment
                    {
                        if self.active_server.is_some() {
                            self.capture_current_drafts(cx);
                            self.backend.send(Command::SaveDrafts(self.draft_store.clone()));
                            self.thread = None;
                            self._thread_subscription = None;
                            self.question_panels.clear();
                            self.attachment_panels.clear();
                            self.backend.send(Command::CloseThread);
                            self.workspace.update(cx, |panel, cx| {
                                panel.set_scope(WorkspaceScope::default(), cx)
                            });
                        }
                        self.active_server = Some(server.clone());
                        self.active_environment = environment;
                        let saved = self.draft_store.environment(&server, &self.active_environment);
                        self.drafts =
                            saved.map(|env| env.thread_text.clone()).unwrap_or_default();
                        self.draft_threads =
                            saved.map(|env| env.new_threads.clone()).unwrap_or_default();
                        self.sync_sidebar_drafts(cx);
                    }
                }
                for panel in self.attachment_panels.values() {
                    panel.update(cx, |panel, cx| panel.set_connected(connected, cx));
                }
                if connected {
                    if !self.switching_server {
                        self.error = None;
                    }
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
                if self.thread.as_ref().is_some_and(|thread| {
                    !thread.read(cx).is_draft()
                        && self
                            .shell
                            .thread(thread.read(cx).thread_id())
                            .is_none_or(|thread| thread.archived_at.is_some())
                }) && self.shell.synchronized
                {
                    if let Some(thread) = &self.thread {
                        let view = thread.read(cx);
                        self.drafts.insert(view.thread_id().to_owned(), view.draft(cx));
                    }
                    self.thread = None;
                    self.backend.send(Command::CloseThread);
                    self._thread_subscription = None;
                    self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(None, cx));
                }
                self.sync_thread_shell(cx);
                self.sync_info(cx);
                let shell = self.shell.clone();
                self.settings
                    .update(cx, |panel, cx| panel.set_projects(&shell.projects, cx));
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_shell(shell, cx));
                // The thread this session's `thread.create` was waiting on
                // has streamed in: open it now that the sidebar has it too.
                if self
                    .pending_new_thread_id
                    .as_deref()
                    .is_some_and(|id| self.shell.thread(id).is_some())
                {
                    let thread_id = self.pending_new_thread_id.take().unwrap();
                    self.open_thread(thread_id, window, cx);
                }
                let before = self.draft_threads.len();
                let shell = &self.shell;
                self.draft_threads.retain(|draft| shell.thread(&draft.id).is_none());
                if before != self.draft_threads.len() {
                    self.capture_current_drafts(cx);
                    self.schedule_draft_save(cx);
                }
                self.sync_sidebar_drafts(cx);
            }
            Event::Thread { thread_id, item } => {
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| {
                        // A late item from the previously open thread is stale.
                        if view.thread_id() == thread_id {
                            view.apply(item, window, cx);
                        }
                    });
                }
            }
            Event::ThreadUnavailable(thread_id) => {
                if let Some(thread) = &self.thread {
                    if thread.read(cx).thread_id() == thread_id {
                        thread.update(cx, |view, cx| view.set_connected(false, cx));
                    }
                }
            }
            Event::Error(message) => self.error = Some(message.into()),
            Event::Settings(settings) => {
                self.settings.update(cx, |panel, cx| panel.set_server_settings(settings, cx));
            }
            Event::SettingsSaved { request_id, result } => {
                self.settings.update(cx, |panel, cx| panel.settings_saved(request_id, result, cx));
            }
            Event::KeybindingsSaved { request_id, result } => {
                if let Ok(rules) = &result {
                    crate::keymap::set_server_keybindings(cx, rules.clone());
                }
                self.settings
                    .update(cx, |panel, cx| panel.keybindings_saved(request_id, result.map(|_| ()), cx));
            }
            Event::Config(config) => {
                crate::keymap::set_server_keybindings(cx, config.keybindings.clone());
                if let Some(environment) = &config.environment {
                    let capabilities = environment.capabilities.clone();
                    self.settings.update(cx, |panel, cx| panel.set_capabilities(capabilities, cx));
                }
                self.usage.update(cx, |usage, cx| usage.set_config(config.clone(), cx));
                self.info.update(cx, |info, cx| {
                    info.set_available_editors(config.available_editors.clone(), cx)
                });
                self.handle_event(Event::Providers(config.providers), window, cx);
            }
            Event::Providers(providers) => {
                self.providers = providers;
                self.usage.update(cx, |usage, cx| usage.set_providers(self.providers.clone(), cx));
                self.settings
                    .update(cx, |panel, cx| panel.set_providers(self.providers.clone(), cx));
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.set_providers(self.providers.clone(), cx));
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| view.set_providers(self.providers.clone(), cx));
                }
            }
            Event::Usage { request_id, result } => {
                self.usage.update(cx, |usage, cx| usage.finish(request_id, result, cx));
            }
            Event::LimitsFinished { request_id, result } => {
                self.usage.update(cx, |usage, cx| usage.finish_limits(request_id, result, cx));
            }
            Event::ResetCreditFinished { key, result } => {
                self.usage.update(cx, |usage, cx| usage.finish_reset(&key, result, cx));
            }
            Event::Archived { request_id, snapshot } => {
                self.settings.update(cx, |page, cx| {
                    page.set_archived(&request_id, snapshot.clone(), cx)
                });
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.set_archived(&request_id, snapshot, cx));
            }
            Event::WorktreeSetup { thread_id, stage, update } => {
                if let Some(setup) = self.worktree_setups.get_mut(&thread_id) {
                    setup.apply(stage, update);
                    if setup.finished() {
                        self.worktree_setups.remove(&thread_id);
                    }
                    self.sync_worktree_setup(cx);
                }
            }
            Event::ThreadActionFinished { thread_id, action, success } => {
                self.settings.update(cx, |page, cx| {
                    page.archive_action_finished(&thread_id, &action, success, cx)
                });
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.action_finished(&thread_id, &action, success, cx)
                });
                if let Some(panel) = self.question_panels.get(&thread_id) {
                    panel.update(cx, |panel, cx| panel.response_finished(&action, success, cx));
                }
                if let Some(thread) = &self.thread {
                    if thread.read(cx).thread_id() == thread_id {
                        thread.update(cx, |view, cx| view.update_finished(&action, success, cx));
                    }
                }
            }
            Event::SendFinished { thread_id, text, success, attachment_ids } => {
                self.sending.remove(&thread_id);
                // A failed step stays up to explain the failure; anything else
                // that stops the send (offline, Cancel) just clears it.
                if !success
                    && self.worktree_setups.get(&thread_id).is_some_and(|setup| !setup.failed())
                {
                    self.worktree_setups.remove(&thread_id);
                    self.sync_worktree_setup(cx);
                }
                if success {
                    if self.drafts.get(&thread_id).is_some_and(|draft| draft.trim() == text) {
                        self.drafts.remove(&thread_id);
                    }
                    if let Some(panel) = self.attachment_panels.get(&thread_id) {
                        panel.update(cx, |panel, cx| panel.clear_sent(&attachment_ids, cx));
                    }
                }
                if let Some(thread) = &self.thread {
                    if thread.read(cx).thread_id() == thread_id {
                        thread
                            .update(cx, |view, cx| view.send_finished(&text, success, window, cx));
                    }
                }
                self.capture_current_drafts(cx);
                self.schedule_draft_save(cx);
                self.sync_sidebar_drafts(cx);
            }
        }
        cx.notify();
    }

    /// Opens the server-side folder picker and dispatches `project.create`
    /// for the chosen folder. The folder's name is the project title, same
    /// default as the web app's "Add project" flow.
    ///
    /// Starts in the folder that holds the open thread's project, so its
    /// siblings are listed: a browse path without a trailing separator is a
    /// name filter and would show only the project already added.
    fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.status, Status::Connected(_)) {
            let initial = self
                .current_project(cx)
                .and_then(|project| containing_folder(&project.workspace_root))
                .unwrap_or_else(|| "~".into());
            self.directory_picker.update(cx, |picker, cx| {
                picker.open(&initial, window, cx);
            });
        }
    }

    /// Starts composing a new thread in `project_id`. Nothing reaches the
    /// server until the first message is sent (see `Command::StartThread`);
    /// until then the draft lives in `draft_threads` and the sidebar.
    ///
    /// Model and modes default like `ChatView.tsx`'s new-thread flow: the
    /// project's default model, else the open thread's, else the first usable
    /// provider's default model.
    fn new_draft_in(
        &mut self,
        project_id: String,
        model: Option<serde_json::Value>,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reuse an untouched draft rather than piling up empty ones.
        if model.is_none() && text.is_none() {
            let untouched = self.draft_threads.iter().find(|draft| {
                draft.project_id == project_id
                    && !self.sending.contains(&draft.id)
                    && self.drafts.get(&draft.id).is_none_or(|text| text.trim().is_empty())
            });
            if let Some(draft) = untouched {
                let draft_id = draft.id.clone();
                return self.open_draft(draft_id, window, cx);
            }
        }
        let current = self.open_thread_shell(cx);
        let runtime_mode = current
            .map(|thread| thread.runtime_mode.clone())
            .unwrap_or_else(|| "full-access".into());
        let interaction_mode = current
            .map(|thread| thread.interaction_mode.clone())
            .unwrap_or_else(|| "default".into());
        let project_default = self
            .shell
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .and_then(|project| project.default_model_selection.clone());
        let model_selection = model
            .or(project_default)
            .or_else(|| current.and_then(|thread| thread.model_selection.clone()))
            .or_else(|| {
                self.providers.iter().find_map(|provider| {
                    if !crate::model_picker::usable(provider) {
                        return None;
                    }
                    let model = provider
                        .models
                        .iter()
                        .find(|model| model.is_default)
                        .or_else(|| provider.models.first())?;
                    Some(json!({ "instanceId": provider.instance_id, "model": model.id }))
                })
            })
            .unwrap_or_else(fallback_model_selection);
        let draft = DraftThread {
            id: t3_client::new_id(),
            project_id,
            model_selection,
            runtime_mode,
            interaction_mode,
            new_worktree: false,
        };
        if let Some(text) = text {
            self.drafts.insert(draft.id.clone(), text);
        }
        let draft_id = draft.id.clone();
        self.draft_threads.push(draft);
        self.open_draft(draft_id, window, cx);
    }

    /// Shows a draft thread in place of the open thread.
    fn open_draft(&mut self, draft_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_settings_open(false, window, cx);
        self.set_usage_open(false, cx);
        let Some(draft) = self.draft_threads.iter().find(|draft| draft.id == draft_id).cloned()
        else {
            return;
        };
        if let Some(thread) = &self.thread
            && thread.read(cx).thread_id() == draft_id
        {
            thread.update(cx, |view, cx| view.focus_composer(window, cx));
            return;
        }
        self.capture_current_drafts(cx);
        if self.thread.as_ref().is_some_and(|thread| !thread.read(cx).is_draft()) {
            self.backend.send(Command::CloseThread);
        }
        let connected = matches!(self.status, Status::Connected(_));
        let project_title = self
            .shell
            .projects
            .iter()
            .find(|project| project.id == draft.project_id)
            .map_or_else(|| SharedString::from("this project"), |project| project.title.clone().into());
        let questions = cx.new(UserInputPanel::new);
        let attachments = self
            .attachment_panels
            .entry(draft_id.clone())
            .or_insert_with(|| cx.new(AttachmentPanel::new))
            .clone();
        let view = cx.new(|cx| {
            let mut view =
                ThreadView::new_draft(draft, project_title, questions, attachments, window, cx);
            view.set_connected(connected, cx);
            view.set_providers(self.providers.clone(), cx);
            view.restore_draft(
                self.drafts.get(&draft_id).map_or("", String::as_str),
                self.sending.contains(&draft_id),
                window,
                cx,
            );
            view
        });
        self._thread_subscription = Some(cx.subscribe_in(&view, window, Self::on_thread_event));
        self.thread = Some(view);
        self.sync_worktree_setup(cx);
        self.sync_thread_shell(cx);
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(Some(draft_id), cx));
        self.after_switch(cx);
    }

    /// Drops a draft thread and its text; closes it if it is open.
    fn discard_draft(&mut self, draft_id: &str, cx: &mut Context<Self>) {
        if self.sending.contains(draft_id) {
            return;
        }
        if self.thread.as_ref().is_some_and(|thread| thread.read(cx).thread_id() == draft_id) {
            self.thread = None;
            self._thread_subscription = None;
            self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(None, cx));
        }
        self.draft_threads.retain(|draft| draft.id != draft_id);
        self.drafts.remove(draft_id);
        self.attachment_panels.remove(draft_id);
        self.capture_current_drafts(cx);
        self.schedule_draft_save(cx);
        self.sync_sidebar_drafts(cx);
        cx.notify();
    }

    /// Housekeeping after the open thread or draft changed: forget drafts
    /// left without text, persist, and refresh the sidebar and workspace.
    fn after_switch(&mut self, cx: &mut Context<Self>) {
        let open = self.thread.as_ref().map(|thread| thread.read(cx).thread_id().to_owned());
        let (drafts, sending) = (&self.drafts, &self.sending);
        self.draft_threads.retain(|draft| {
            open.as_deref() == Some(draft.id.as_str())
                || sending.contains(&draft.id)
                || drafts.get(&draft.id).is_some_and(|text| !text.trim().is_empty())
        });
        self.capture_current_drafts(cx);
        self.schedule_draft_save(cx);
        self.sync_sidebar_drafts(cx);
        if self.workspace_open {
            self.sync_workspace(cx);
        }
        self.sync_info(cx);
        cx.notify();
    }

    /// Gives the sidebar the draft threads and which threads have unsent text.
    fn sync_sidebar_drafts(&self, cx: &mut Context<Self>) {
        let new_threads = self
            .draft_threads
            .iter()
            .map(|draft| crate::sidebar::SidebarDraft {
                id: draft.id.clone(),
                project_id: draft.project_id.clone(),
                text: self.drafts.get(&draft.id).cloned().unwrap_or_default(),
            })
            .collect();
        let unsent = self
            .drafts
            .iter()
            .filter(|(id, text)| {
                !text.trim().is_empty() && !self.draft_threads.iter().any(|d| d.id == **id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_drafts(new_threads, unsent, cx));
    }

    /// The project of the open thread or draft.
    fn current_project(&self, cx: &App) -> Option<&ProjectShell> {
        let view = self.thread.as_ref()?.read(cx);
        let project_id = match view.draft_thread() {
            Some(draft) => draft.project_id.clone(),
            None => self.shell.thread(view.thread_id())?.project_id.clone(),
        };
        self.shell.projects.iter().find(|project| project.id == project_id)
    }

    /// Hands the open thread its shell entry: working state and modes.
    fn sync_thread_shell(&self, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else { return };
        let shell = self.shell.thread(thread.read(cx).thread_id()).cloned();
        let cwd = self.composer_cwd(cx);
        thread.update(cx, |view, cx| {
            view.set_cwd(cwd);
            view.set_shell(shell, cx);
        });
    }

    /// The folder the open composer's `@` mentions search: the thread's
    /// worktree, else its project's root.
    fn composer_cwd(&self, cx: &App) -> Option<String> {
        self.open_thread_shell(cx)
            .and_then(|thread| thread.worktree_path.clone())
            .or_else(|| self.current_project(cx).map(|project| project.workspace_root.clone()))
    }

    fn open_thread(&mut self, thread_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_settings_open(false, window, cx);
        self.set_usage_open(false, cx);
        if let Some(thread) = &self.thread {
            if thread.read(cx).thread_id() == thread_id && !thread.read(cx).is_draft() {
                if !thread.read(cx).is_ready() && matches!(self.status, Status::Connected(_)) {
                    thread.update(cx, |view, cx| {
                        view.reset(cx);
                        view.set_connected(true, cx);
                    });
                    self.backend.send(Command::OpenThread(thread_id));
                    self.error = None;
                    cx.notify();
                }
                return;
            }
        }
        self.capture_current_drafts(cx);
        let connected = matches!(self.status, Status::Connected(_));
        let panel = self
            .question_panels
            .entry(thread_id.clone())
            .or_insert_with(|| cx.new(UserInputPanel::new))
            .clone();
        let saved_answers = self
            .active_server
            .as_deref()
            .and_then(|server| self.draft_store.environment(server, &self.active_environment))
            .and_then(|env| env.question_answers.get(&thread_id))
            .cloned()
            .unwrap_or_default();
        panel.update(cx, |panel, cx| panel.import_answer_drafts(saved_answers, window, cx));
        let attachments = self
            .attachment_panels
            .entry(thread_id.clone())
            .or_insert_with(|| cx.new(AttachmentPanel::new))
            .clone();
        let view = cx.new(|cx| {
            let mut view =
                ThreadView::new_with_attachments(thread_id.clone(), panel, attachments, window, cx);
            view.set_connected(connected, cx);
            view.set_providers(self.providers.clone(), cx);
            view.restore_draft(
                self.drafts.get(&thread_id).map_or("", String::as_str),
                self.sending.contains(&thread_id),
                window,
                cx,
            );
            view
        });
        self._thread_subscription = Some(cx.subscribe_in(&view, window, Self::on_thread_event));
        self.thread = Some(view);
        self.sync_worktree_setup(cx);
        self.sync_thread_shell(cx);
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_thread(Some(thread_id.clone()), cx));
        self.backend.send(Command::OpenThread(thread_id));
        self.after_switch(cx);
    }

    fn on_thread_event(
        &mut self,
        view: &Entity<ThreadView>,
        event: &ThreadViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = view.read(cx).thread_id().to_owned();
        match event {
            ThreadViewEvent::DraftChanged(text) => {
                self.drafts.insert(id.clone(), text.clone());
                if let Some(server) = &self.active_server {
                    let drafts = self.draft_store.environment_mut(server, &self.active_environment);
                    if text.is_empty() {
                        drafts.thread_text.remove(&id);
                    } else {
                        drafts.thread_text.insert(id, text.clone());
                    }
                }
                self.schedule_draft_save(cx);
                self.sync_sidebar_drafts(cx);
                return;
            }
            ThreadViewEvent::DraftSettingsChanged(draft) => {
                if let Some(entry) = self.draft_threads.iter_mut().find(|d| d.id == draft.id) {
                    *entry = draft.clone();
                }
                self.capture_current_drafts(cx);
                self.schedule_draft_save(cx);
                self.sync_info(cx);
                return;
            }
            ThreadViewEvent::QuestionDraftsChanged(answers) => {
                if let Some(server) = &self.active_server {
                    let drafts = self.draft_store.environment_mut(server, &self.active_environment);
                    if answers.is_empty() {
                        drafts.question_answers.remove(&id);
                    } else {
                        drafts.question_answers.insert(id, answers.clone());
                    }
                }
                self.schedule_draft_save(cx);
                return;
            }
            ThreadViewEvent::Attachment(event) => {
                match event {
                    AttachmentPanelEvent::ChooseFiles => {
                        let paths = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: true,
                            prompt: Some("Attach files".into()),
                        });
                        let panel = self.attachment_panels.get(&id).cloned();
                        cx.spawn(async move |_, cx| {
                            if let (Some(panel), Ok(Ok(Some(paths)))) = (panel, paths.await) {
                                let _ = panel.update(cx, |panel, cx| panel.add_paths(paths, cx));
                            }
                        })
                        .detach();
                    }
                    AttachmentPanelEvent::UploadRequested { request_id, attachment } => {
                        self.backend.send(Command::UploadAttachment {
                            thread_id: id,
                            request_id: *request_id,
                            attachment: attachment.clone(),
                        })
                    }
                    AttachmentPanelEvent::Rejected(message) => {
                        self.error = Some(message.clone().into());
                        cx.notify();
                    }
                }
                return;
            }
            ThreadViewEvent::CancelWorktreeSetup => {
                self.backend.send(Command::CancelStartThread(id.clone()));
                self.worktree_setups.remove(&id);
                self.sync_worktree_setup(cx);
                return;
            }
            ThreadViewEvent::SearchFiles { request_id, query } => {
                let Some(cwd) = self.composer_cwd(cx) else {
                    view.update(cx, |view, cx| {
                        view.apply_file_results(*request_id, Err("This thread has no folder.".into()), cx)
                    });
                    return;
                };
                self.backend.send(Command::Workspace {
                    request_id: *request_id,
                    scope: WorkspaceScope {
                        project_id: Some(COMPOSER_FILES_SCOPE.into()),
                        thread_id: Some(id),
                        cwd: Some(cwd.clone()),
                    },
                    request: t3_client::WorkspaceRequest::SearchEntries {
                        cwd,
                        query: query.clone(),
                        limit: crate::mentions::FILE_RESULT_LIMIT,
                    },
                });
                return;
            }
            _ => {}
        }
        if let Some(draft) = view.read(cx).draft_thread().cloned() {
            match event {
                ThreadViewEvent::Send(text, attachments) => {
                    let worktree_base = if draft.new_worktree {
                        let root = self.current_project(cx).map(|p| p.workspace_root.clone());
                        let branch = self.info.read(cx).current_branch().map(str::to_owned);
                        let (Some(root), Some(branch)) = (root, branch) else {
                            self.error = Some(
                                "The checked-out branch is still loading, so the worktree has no base yet. Try sending again in a moment, or use the local checkout.".into(),
                            );
                            view.update(cx, |view, cx| view.send_finished(text, false, window, cx));
                            return cx.notify();
                        };
                        Some((root, branch))
                    } else {
                        None
                    };
                    let setup_script = worktree_base.as_ref().and_then(|_| {
                        let project = self.current_project(cx)?;
                        project.scripts.iter().find(|s| s.run_on_worktree_create).cloned()
                    });
                    if worktree_base.is_some() {
                        self.worktree_setups.insert(draft.id.clone(), Default::default());
                        self.sync_worktree_setup(cx);
                    }
                    self.sending.insert(draft.id.clone());
                    self.pending_new_thread_id = Some(draft.id.clone());
                    self.backend.send(Command::StartThread {
                        title: thread_title(text),
                        draft,
                        text: text.clone(),
                        attachments: attachments.clone(),
                        worktree_base,
                        setup_script,
                    });
                }
                ThreadViewEvent::OpenAttachment(attachment) => {
                    self.backend.send(Command::OpenAsset(attachment.clone()))
                }
                ThreadViewEvent::LoadThumbnail(attachment) => {
                    self.backend.send(Command::LoadThumbnail(attachment.clone()))
                }
                _ => {}
            }
            return;
        }
        let Some(thread) = self.shell.thread(view.read(cx).thread_id()).cloned() else {
            self.error = Some("This thread is no longer available.".into());
            return cx.notify();
        };
        match event {
            ThreadViewEvent::Send(text, attachments) => {
                self.sending.insert(thread.id.clone());
                self.backend.send(Command::SendMessage {
                    thread,
                    text: text.clone(),
                    attachments: attachments.clone(),
                });
            }
            ThreadViewEvent::OpenAttachment(attachment) => {
                self.backend.send(Command::OpenAsset(attachment.clone()))
            }
            ThreadViewEvent::LoadThumbnail(attachment) => {
                self.backend.send(Command::LoadThumbnail(attachment.clone()))
            }
            ThreadViewEvent::Update(action) => self
                .backend
                .send(Command::ThreadAction { thread_id: thread.id, action: action.clone() }),
            ThreadViewEvent::ContinueInNewThread(model) => {
                let text = continuation_prompt(&thread.id, &thread.title);
                self.new_draft_in(
                    thread.project_id.clone(),
                    Some(model.clone()),
                    Some(text),
                    window,
                    cx,
                );
            }
            ThreadViewEvent::DraftChanged(_)
            | ThreadViewEvent::DraftSettingsChanged(_)
            | ThreadViewEvent::QuestionDraftsChanged(_)
            | ThreadViewEvent::Attachment(_)
            | ThreadViewEvent::SearchFiles { .. }
            | ThreadViewEvent::CancelWorktreeSetup => {}
            ThreadViewEvent::OpenUsageLimits => {
                self.set_settings_open(false, window, cx);
                self.usage.update(cx, |usage, cx| usage.show_limits(cx));
                self.set_usage_open(true, cx);
                self.focus_handle.focus(window, cx);
            }
            ThreadViewEvent::Stop => {
                let run_id = thread
                    .active_run_id
                    .clone()
                    .or_else(|| thread.session.as_ref().and_then(|s| s.active_turn_id.clone()));
                if let Some(run_id) = run_id {
                    self.backend.send(Command::Interrupt { thread_id: thread.id, run_id });
                }
            }
        }
    }

    fn pair(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let link = self.pairing_link.read(cx).value().trim().to_owned();
        if link.is_empty() || self.pairing_pending {
            return;
        }
        self.error = None;
        self.pairing_pending = true;
        self.switching_server = true;
        if let Some(thread) = &self.thread {
            thread.update(cx, |view, cx| view.set_connected(false, cx));
        }
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.set_status(Status::Connecting(String::new()), cx));
        self.backend.send(Command::Pair(link));
        cx.notify();
    }

    fn capture_current_drafts(&mut self, cx: &App) {
        if let Some(thread) = &self.thread {
            self.drafts.insert(thread.read(cx).thread_id().to_owned(), thread.read(cx).draft(cx));
        }
        if let Some(server) = &self.active_server {
            let drafts = self.draft_store.environment_mut(server, &self.active_environment);
            drafts.thread_text = self
                .drafts
                .iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(id, text)| (id.clone(), text.clone()))
                .collect();
            drafts.new_threads = self
                .draft_threads
                .iter()
                .filter(|draft| self.drafts.get(&draft.id).is_some_and(|t| !t.trim().is_empty()))
                .cloned()
                .collect();
            for (id, panel) in &self.question_panels {
                let answers = panel.read(cx).snapshot_answer_drafts(cx);
                if answers.is_empty() {
                    drafts.question_answers.remove(id);
                } else {
                    drafts.question_answers.insert(id.clone(), answers);
                }
            }
        }
    }

    fn schedule_draft_save(&mut self, cx: &mut Context<Self>) {
        self.draft_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let _ = this.update(cx, |this, _| {
                this.backend.send(Command::SaveDrafts(this.draft_store.clone()))
            });
        }));
    }

    fn set_usage_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.usage_open == open {
            return;
        }
        self.usage_open = open;
        if open {
            self.usage.update(cx, |usage, cx| usage.ensure_fresh(cx));
        }
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_usage_open(open, cx));
        self.sync_info(cx);
        cx.notify();
    }

    fn set_settings_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.read(cx).is_open() == open {
            return;
        }
        self.settings.update(cx, |page, cx| {
            page.set_open(open, cx);
            if open {
                page.focus(window, cx);
            }
        });
        self.sync_info(cx);
        if open {
            // Settings can change from other clients; refresh when viewed.
            self.backend.send(Command::LoadSettings);
        }
        if !open {
            if self.status == Status::NeedsPairing || self.switching_server {
                self.pairing_link.update(cx, |input, cx| input.focus(window, cx));
            } else if !self.usage_open {
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| view.focus_composer(window, cx));
                } else {
                    self.focus_handle.focus(window, cx);
                }
            } else {
                self.focus_handle.focus(window, cx);
            }
        }
        cx.notify();
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_settings_open(!self.settings.read(cx).is_open(), window, cx);
    }

    fn toggle_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_settings_open(false, window, cx);
        self.workspace_open = !self.workspace_open;
        if self.workspace_open {
            if window.bounds().size.width < px(1100.) {
                self.sidebar_open = false;
            }
            self.sync_workspace(cx);
        }
        cx.notify();
    }

    fn sync_workspace(&self, cx: &mut Context<Self>) {
        let thread = self.open_thread_shell(cx);
        let project = self.current_project(cx);
        let scope = WorkspaceScope {
            project_id: project.map(|p| p.id.clone()),
            thread_id: thread.map(|t| t.id.clone()),
            cwd: thread
                .and_then(|t| t.worktree_path.clone())
                .or_else(|| project.map(|p| p.workspace_root.clone())),
        };
        self.workspace.update(cx, |panel, cx| panel.set_scope(scope, cx));
    }

    fn toggle_info(&mut self, cx: &mut Context<Self>) {
        self.info_open = !self.info_open;
        let open = self.info_open;
        crate::prefs::Prefs::update(cx, |prefs| prefs.info_panel_open = open);
        self.sync_info(cx);
        cx.notify();
    }

    /// Whether the info card is on screen: toggled on, over an open thread.
    fn info_visible(&self, cx: &App) -> bool {
        self.info_open
            && self.thread.is_some()
            && !self.usage_open
            && !self.switching_server
            && !self.settings.read(cx).is_open()
    }

    /// Hands the info card the open thread or draft and its project.
    fn sync_info(&self, cx: &mut Context<Self>) {
        let visible = self.info_visible(cx);
        let thread = self.open_thread_shell(cx);
        let draft = self.thread.as_ref().and_then(|view| view.read(cx).draft_thread().cloned());
        let project = self.current_project(cx);
        let scope = InfoScope {
            project_id: project.map(|p| p.id.clone()),
            project_title: project.map(|p| p.title.clone()).unwrap_or_default(),
            project_root: project.map(|p| p.workspace_root.clone()),
            scripts: project.map(|p| p.scripts.clone()).unwrap_or_default(),
            thread_id: thread.map(|t| t.id.clone()),
            draft_new_worktree: draft.map(|d| d.new_worktree),
            worktree_path: thread.and_then(|t| t.worktree_path.clone()),
            turn_marker: thread
                .and_then(|t| t.latest_turn.as_ref())
                .and_then(|turn| turn.completed_at.clone()),
        };
        let wants_branch = scope.draft_new_worktree.is_some();
        self.info.update(cx, |info, cx| {
            info.set_scope(scope, cx);
            info.set_visible(visible, cx);
            // The draft's workspace picker shows the checked-out branch, and
            // a new worktree branches from it; have it ready by the time the
            // draft is sent.
            if wants_branch {
                info.load_branch(cx);
            }
        });
        self.sync_draft_branch(cx);
    }

    /// Hands the open view its thread's worktree setup progress, if any.
    fn sync_worktree_setup(&self, cx: &mut Context<Self>) {
        let Some(view) = &self.thread else { return };
        let setup = self.worktree_setups.get(view.read(cx).thread_id()).cloned();
        view.update(cx, |view, cx| view.set_worktree_setup(setup, cx));
    }

    /// Shows the checked-out branch in the open draft's workspace picker.
    fn sync_draft_branch(&self, cx: &mut Context<Self>) {
        let Some(view) = &self.thread else { return };
        if view.read(cx).draft_thread().is_none() {
            return;
        }
        let branch = self.info.read(cx).current_branch().map(str::to_owned);
        view.update(cx, |view, cx| view.set_draft_branch(branch, cx));
    }

    fn on_info_event(
        &mut self,
        event: &InfoPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InfoPanelEvent::Request { request_id, request } => self
                .backend
                .send(Command::Info { request_id: *request_id, request: request.clone() }),
            InfoPanelEvent::ShowChanges => {
                self.show_workspace(window, cx);
                self.workspace.update(cx, |panel, cx| panel.select_tab(WorkspaceTab::Changes, cx));
            }
            InfoPanelEvent::RunScript(command) => {
                self.show_workspace(window, cx);
                self.workspace.update(cx, |panel, cx| panel.run_command(command.clone(), cx));
            }
            InfoPanelEvent::SetDraftWorktree(new_worktree) => {
                if let Some(thread) = &self.thread {
                    thread.update(cx, |view, cx| view.set_draft_worktree(*new_worktree, cx));
                }
            }
            InfoPanelEvent::AddScript => {
                if let Some(project) = self.current_project(cx) {
                    let (id, scripts) = (project.id.clone(), project.scripts.clone());
                    self.script_dialog
                        .update(cx, |dialog, cx| dialog.open(id, scripts, window, cx));
                }
            }
        }
    }

    fn show_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace_open {
            self.toggle_workspace(window, cx);
        }
    }

    fn open_thread_shell(&self, cx: &App) -> Option<&ThreadShell> {
        self.shell.thread(self.thread.as_ref()?.read(cx).thread_id())
    }
}

impl Render for T3App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(crate::prefs::Prefs::global(cx).font_size_interface as f32));
        let settings_open = self.settings.read(cx).is_open();
        let usage_active = self.usage_open && !settings_open && !self.switching_server;
        let workspace_visible = self.workspace_open && !self.switching_server && !settings_open;
        let workspace_width = px((f32::from(window.bounds().size.width) * 0.4).clamp(240., 420.));
        let info_visible = self.info_visible(cx);
        self.usage.update(cx, |usage, _| usage.set_active(usage_active));
        let main = if settings_open {
            self.settings.clone().into_any_element()
        } else if self.status == Status::NeedsPairing || self.switching_server {
            self.render_pairing(window, cx).into_any_element()
        } else if self.usage_open {
            self.usage.clone().into_any_element()
        } else if let Some(thread) = &self.thread {
            // Cached: `T3App` re-renders on plenty of events (shell deltas,
            // status changes) that have nothing to do with this thread. Skip
            // re-rendering the transcript unless the thread view notifies
            // itself (see `ThreadView::apply`/`set_shell`/`set_connected`).
            thread.clone().cached(StyleRefinement::default().size_full()).into_any_element()
        } else {
            self.render_empty(window, cx).into_any_element()
        };

        v_flex()
            .track_focus(&self.focus_handle)
            .key_context("T3App")
            .on_action(cx.listener(|this, _: &NewThread, window, cx| {
                if matches!(this.status, Status::Connected(_)) {
                    let projects = crate::project_picker::recent_projects(&this.shell);
                    this.project_picker.update(cx, |picker, cx| picker.open(projects, window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
                this.set_settings_open(false, window, cx);
                this.set_usage_open(false, cx);
                if let Some(thread) = &this.thread {
                    thread.update(cx, |view, cx| view.focus_composer(window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleWorkspace, window, cx| {
                this.toggle_workspace(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleInfo, _, cx| this.toggle_info(cx)))
            .on_action(cx.listener(|this, _: &UndoThreadAction, _, cx| {
                if !this.sidebar.update(cx, |sidebar, cx| sidebar.undo(cx)) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.toggle_settings(window, cx)
            }))
            .on_action(cx.listener(|this, _: &DismissModal, window, cx| {
                if this.settings.read(cx).is_open() {
                    this.set_settings_open(false, window, cx);
                    return;
                } else if this.script_dialog.read(cx).is_open() {
                    this.script_dialog.update(cx, |dialog, cx| dialog.close(cx));
                } else if this.directory_picker.read(cx).is_open() {
                    this.directory_picker.update(cx, |picker, cx| picker.cancel(cx));
                } else {
                    cx.propagate();
                    return;
                }
                if let Some(thread) = &this.thread {
                    thread.update(cx, |view, cx| view.focus_composer(window, cx));
                } else {
                    this.pairing_link.update(cx, |input, cx| input.focus(window, cx));
                }
            }))
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .relative()
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
                                div().p_2().child(Alert::error("backend-error", error).on_close(
                                    cx.listener(|this, _, _, cx| {
                                        this.error = None;
                                        cx.notify();
                                    }),
                                ))
                            }))
                            .child(
                                div()
                                    .id("main-content")
                                    .test_support()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .child(main),
                            ),
                    )
                    .when(workspace_visible, |row| {
                        row.child(
                            div()
                                .w(workspace_width)
                                .flex_shrink_0()
                                .h_full()
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .child(self.workspace.clone()),
                        )
                    })
                    // Floats over the content, left of the workspace panel.
                    .when(info_visible, |row| {
                        row.child(
                            div()
                                .absolute()
                                .top_2()
                                .bottom_2()
                                .right(if workspace_visible {
                                    workspace_width + px(8.)
                                } else {
                                    px(8.)
                                })
                                .w(INFO_PANEL_WIDTH)
                                .child(self.info.clone()),
                        )
                    }),
            )
            // Painted last so it stacks above the sidebar and main column.
            .child(self.project_picker.clone())
            .child(self.directory_picker.clone())
            .child(self.script_dialog.clone())
    }
}

impl T3App {
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let toggle = Button::new("toggle-sidebar")
            .ghost()
            .small()
            .icon(icon(IconName::PanelLeft))
            .accessibility_label("Toggle sidebar")
            .tooltip(if self.sidebar_open {
                "Hide sidebar (Ctrl+B)"
            } else {
                "Show sidebar (Ctrl+B)"
            })
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
            .child(div().text_sm().font_semibold().child("Rust code"))
            .when(self.sidebar_open, |brand| {
                brand
                    .w(SIDEBAR_WIDTH)
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.sidebar_border)
            });
        let settings_open = self.settings.read(cx).is_open();
        let shell = self.open_thread_shell(cx).filter(|_| !self.usage_open && !settings_open);
        let project =
            shell.and_then(|thread| self.shell.projects.iter().find(|p| p.id == thread.project_id));
        let breadcrumb = h_flex()
            .gap_2()
            .px_4()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_sm()
            .when_some(project, |row, project| {
                row.child(ui::project_tag(&project.id, &project.title))
                    .child(
                        div()
                            .max_w(px(160.))
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(project.title.clone()),
                    )
                    .child(div().text_color(theme.muted_foreground).child("/"))
            })
            .when_some(shell, |row, thread| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_semibold()
                        .child(ui::display_title(&thread.title)),
                )
            })
            .when(self.usage_open && !settings_open, |row| {
                row.child(div().font_semibold().child("Usage"))
            })
            .when(settings_open, |row| row.child(div().font_semibold().child("Settings")));
        let controls = h_flex()
            .gap_1()
            .px_2()
            .h_full()
            .flex_shrink_0()
            .child(
                Button::new("info")
                    .ghost()
                    .small()
                    .icon(icon(IconName::TextAlignStart))
                    .selected(self.info_open)
                    .accessibility_label("Workspace info")
                    .tooltip("Workspace and version control (Ctrl+I)")
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_info(cx))),
            )
            .child(
                Button::new("workspace")
                    .ghost()
                    .small()
                    .icon(icon(IconName::PanelRight))
                    .selected(self.workspace_open)
                    .accessibility_label("Workspace")
                    .tooltip("Files, changes and terminal (Ctrl+J)")
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_workspace(window, cx))),
            )
            .child(
                Button::new("settings")
                    .ghost()
                    .small()
                    .icon(icon(IconName::Settings))
                    .accessibility_label("Settings")
                    .tooltip("Settings (Ctrl+,)")
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(window, cx)
                    })),
            );
        TitleBar::new().pl_0().child(
            h_flex().w_full().h_full().min_w_0().child(brand).child(breadcrumb).child(controls),
        )
    }

    /// The main column with no thread open: what is on the server, the two
    /// ways to start, and the shortcuts that get there faster.
    fn render_empty(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let connected = matches!(self.status, Status::Connected(_));
        let active = self
            .shell
            .threads
            .iter()
            .filter(|thread| thread.archived_at.is_none() && !thread.is_settled())
            .count();
        let summary: SharedString = match &self.status {
            Status::Connected(_) => format!(
                "{} project{} · {active} active thread{}",
                self.shell.projects.len(),
                if self.shell.projects.len() == 1 { "" } else { "s" },
                if active == 1 { "" } else { "s" },
            )
            .into(),
            Status::Reconnecting { reason, .. } => format!("Reconnecting: {reason}").into(),
            _ => "Connecting to the server…".into(),
        };
        let shortcut = |keys: &str, label: &'static str| {
            h_flex()
                .justify_between()
                .gap_4()
                .py_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label)
                .children(Keystroke::parse(keys).ok().map(Kbd::new))
        };

        div().flex().flex_1().items_center().justify_center().p_6().child(
            v_flex()
                .id("empty-state")
                .w(px(360.))
                .gap_6()
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(40.))
                                .rounded_lg()
                                .border_1()
                                .border_color(theme.primary.opacity(0.35))
                                .bg(theme.primary.opacity(0.08))
                                .child(
                                    icon(IconName::SquareTerminal)
                                        .size(px(20.))
                                        .text_color(theme.primary),
                                ),
                        )
                        .child(div().text_xl().font_semibold().child("Pick up a thread"))
                        .child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .when(!connected, |row| {
                                    row.child(ui::loader("empty-connecting", Size::XSmall))
                                })
                                .child(summary),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("empty-new-thread")
                                .primary()
                                .small()
                                .icon(icon(IconName::SquarePen))
                                .label("New thread")
                                .disabled(!connected || self.shell.projects.is_empty())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let projects = crate::project_picker::recent_projects(&this.shell);
                                    this.project_picker
                                        .update(cx, |picker, cx| picker.open(projects, window, cx));
                                })),
                        )
                        .child(
                            Button::new("empty-add-project")
                                .outline()
                                .small()
                                .icon(icon(IconName::FolderPlus))
                                .label("Add project")
                                .disabled(!connected)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_project(window, cx)),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .pt_4()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(shortcut("ctrl-n", "New thread"))
                        .child(shortcut("ctrl-l", "Focus the composer"))
                        .child(shortcut("ctrl-j", "Files, changes and terminal"))
                        .child(shortcut("ctrl-b", "Toggle the sidebar"))
                        .child(shortcut("ctrl-,", "Settings")),
                ),
        )
    }

    /// Run T3 on this machine against the shared T3 home: attach to the
    /// server already running there, or start one. `None` finds the executable.
    fn start_local(&mut self, executable: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.capture_current_drafts(cx);
        self.pairing_pending = true;
        self.switching_server = true;
        self.backend.send(Command::StartLocal(executable));
        self.settings.update(cx, |settings, cx| settings.set_open(false, cx));
        cx.notify();
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
                .child(Input::new(&self.pairing_link).disabled(self.pairing_pending))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .when(self.switching_server, |row| {
                            row.child(
                                Button::new("cancel-pair")
                                    .ghost()
                                    .label("Cancel")
                                    .disabled(self.pairing_pending)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.switching_server = false;
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            Button::new("pair-local")
                                .outline()
                                .label("Use T3 on this machine")
                                .disabled(self.pairing_pending)
                                .on_click(cx.listener(|this, _, _, cx| this.start_local(None, cx))),
                        )
                        .child(
                            Button::new("pair")
                                .primary()
                                .label(if self.pairing_pending { "Connecting…" } else { "Connect" })
                                .disabled(
                                    self.pairing_pending
                                        || self.pairing_link.read(cx).value().trim().is_empty(),
                                )
                                .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                        ),
                ),
        )
    }
}

/// The folder holding `path`, with a trailing separator in the path's own
/// style: `C:\repos\app` gives `C:\repos\`, `/home/me/app/` gives `/home/me/`.
/// A new thread's title: the first line of its first message, shortened.
fn thread_title(text: &str) -> String {
    const MAX: usize = 60;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    if line.is_empty() {
        return "New thread".into();
    }
    if line.chars().count() <= MAX {
        return line.to_owned();
    }
    let short: String = line.chars().take(MAX - 1).collect();
    format!("{}…", short.trim_end())
}

/// The opening message of a thread that continues `thread_id` on another
/// provider: the new agent has none of the old conversation, so it is told
/// to read that thread first.
fn continuation_prompt(thread_id: &str, title: &str) -> String {
    format!(
        "Continue the work from thread `{thread_id}` (\"{}\"). First read that thread's \
         full history to understand what was done and what is left, then continue from \
         where it stopped.",
        ui::display_title(title)
    )
}

fn containing_folder(path: &str) -> Option<String> {
    let path = path.trim().trim_end_matches(['/', '\\']);
    let split = path.rfind(['/', '\\'])?;
    Some(path[..=split].to_owned())
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn new_thread_titles_use_the_first_line_and_stay_short() {
        assert_eq!(super::thread_title("\n  Fix the build  \nand more"), "Fix the build");
        assert_eq!(super::thread_title("   "), "New thread");
        let long = super::thread_title(&"word ".repeat(30));
        assert_eq!(long.chars().count(), 60);
        assert!(long.ends_with('\u{2026}'));
    }

    #[::core::prelude::v1::test]
    fn containing_folder_keeps_the_path_style() {
        assert_eq!(containing_folder(r"C:\repos\app").as_deref(), Some(r"C:\repos\"));
        assert_eq!(containing_folder("/home/me/app/").as_deref(), Some("/home/me/"));
        assert_eq!(containing_folder("C:/repos/existing").as_deref(), Some("C:/repos/"));
        assert_eq!(containing_folder("app"), None);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;

    #[gpui_kit::test]
    fn empty_server_keybindings_restore_defaults_after_a_switch(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
        });
        let (backend, _commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx,
                |window, cx| cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx))).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            let old_config: t3_client::ServerConfig = serde_json::from_value(serde_json::json!({
                "keybindings": [{ "command": "chat.new", "shortcut": { "key": "k", "modKey": true } }]
            })).unwrap();
            app.update(cx, |app, cx| {
                app.handle_event(Event::Config(old_config.clone()), window, cx);
                assert!(!crate::keymap::server_keybindings(cx).is_empty());
                app.handle_event(Event::Config(t3_client::ServerConfig::default()), window, cx);
                assert!(crate::keymap::server_keybindings(cx).is_empty());
                let keys = crate::keymap::shortcuts(crate::keymap::Command::NewThread,
                    &crate::keymap::server_keybindings(cx), crate::prefs::Prefs::global(cx));
                assert_eq!(keys, ["mod+n", "mod+shift+o"]);
                app.handle_event(Event::Config(old_config), window, cx);
                app.handle_event(Event::Status(Status::Connecting("New server".into())), window, cx);
                assert!(crate::keymap::server_keybindings(cx).is_empty());
            });
        }).unwrap();
    }

    #[gpui_kit::test]
    fn settings_returns_to_empty_usage_and_pairing_views_with_working_shortcuts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
        });
        let (backend, _commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1400.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx)),
            ).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_event(Event::Status(Status::Connected("Test server".into())), window, cx);
            });
            window.render_frame(cx);
            window.click("settings", cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            window.render_frame(cx);
            assert_eq!(window.find("settings-page").bounds(), window.find("main-content").bounds());
            assert!(window.find("settings-nav-connections").bounds().origin.y
                > window.find("settings-nav-appearance").bounds().origin.y);
            window.press("escape", cx);
            assert!(!app.read(cx).settings.read(cx).is_open());
            window.press("ctrl-,", cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            window.press("escape", cx);
            window.click("sidebar-usage", cx);
        }).unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert!(app.read(cx).usage_open);
            window.press("ctrl-,", cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            window.press("escape", cx);
            assert!(app.read(cx).usage_open);
            window.press("ctrl-,", cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            window.press("escape", cx);
            app.update(cx, |app, cx| {
                app.handle_event(Event::Status(Status::NeedsPairing), window, cx);
            });
            window.press("ctrl-,", cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            window.click("settings-nav-connections", cx);
            window.render_frame(cx);
            assert!(window.find("settings-managed-server").visible());
            window.press("escape", cx);
            assert!(!app.read(cx).settings.read(cx).is_open());
            window.input("Pairing draft", cx);
            assert_eq!(app.read(cx).pairing_link.read(cx).value(), "Pairing draft");
        }).unwrap();
    }

    #[gpui_kit::test]
    fn workspace_and_settings_shortcuts_fit_minimum_window(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
        });
        let (backend, _commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(720.), px(480.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx)),
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_event(
                    Event::Status(Status::Connected("http://localhost:3773".into())),
                    window,
                    cx,
                );
                app.open_thread("thread-1".into(), window, cx);
            });
            window.render_frame(cx);
            assert!(
                window.find("workspace").bounds().origin.x
                    > window.find("toggle-sidebar").bounds().origin.x + SIDEBAR_WIDTH
            );
            window.click("toggle-sidebar", cx);
            assert!(!app.read(cx).sidebar_open);
            window.click("toggle-sidebar", cx);
            assert!(app.read(cx).sidebar_open);
            window.press("ctrl-j", cx);
            assert!(app.read(cx).workspace_open);
            assert!(!app.read(cx).sidebar_open);
            window.press("ctrl-,", cx);
            window.render_frame(cx);
            assert!(app.read(cx).settings.read(cx).is_open());
            let page = window.find("settings-page");
            assert_eq!(page.bounds(), window.find("main-content").bounds());
            assert!(page.bounds().size.height > px(480.) * 0.8);
            assert_eq!(page.bounds().size.width, px(720.));
            assert_eq!(page.focused(), Some(true));
            window.input("Do not edit the hidden composer", cx);
            assert!(app.read(cx).thread.as_ref().unwrap().read(cx).draft(cx).is_empty());
            window.click("settings-nav-keybindings", cx);
            window.render_frame(cx);
            assert!(window.find("settings-close").visible());
            window.press("escape", cx);
            assert!(!app.read(cx).settings.read(cx).is_open());
            window.input("Typing after settings", cx);
            assert_eq!(
                app.read(cx).thread.as_ref().unwrap().read(cx).draft(cx),
                "Typing after settings"
            );
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            // Opening from the sidebar uses the same page and preserves the draft.
            window.press("ctrl-j", cx);
            window.click("toggle-sidebar", cx);
            window.render_frame(cx);
            window.click("sidebar-settings", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert!(app.read(cx).settings.read(cx).is_open());
            window.render_frame(cx);
            assert_eq!(window.find("settings-page").bounds(), window.find("main-content").bounds());
            assert!(window.find("settings-page").bounds().origin.x >= SIDEBAR_WIDTH);
            window.click("settings-close", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.input(" again", cx);
            assert_eq!(app.read(cx).thread.as_ref().unwrap().read(cx).draft(cx), "Typing after settings again");
            window.press("ctrl-,", cx);
            window.press("ctrl-,", cx);
            assert!(!app.read(cx).settings.read(cx).is_open());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn new_thread_picker_shows_projects_and_folder_browser_uses_selected_root(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            crate::project_picker::init(cx);
        });
        let (backend, mut commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1000.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx)),
            )
            .unwrap()
        });
        cx.update_window(handle,|_,window,cx| {
            app.update(cx,|app,cx| {
                app.handle_event(Event::Status(Status::Connected("http://localhost:3773".into())),window,cx);
                app.shell.projects.push(serde_json::from_value(json!({"id":"project-1","title":"Existing project","workspaceRoot":"C:/repos/existing"})).unwrap());
                app.shell.threads.push(serde_json::from_value(json!({"id":"thread-1","projectId":"project-1","title":"Current thread","runtimeMode":"full-access"})).unwrap());
                app.sidebar.update(cx,|sidebar,cx|sidebar.set_shell(app.shell.clone(),cx));
                app.open_thread("thread-1".into(),window,cx);
            });
            window.render_frame(cx);
            window.click("new-thread",cx);
        }).unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(("project-picker-row", 0usize)).visible());
            window.click(("project-picker-row", 0usize), cx);
        })
        .unwrap();
        // Picking a project opens a draft; nothing is created on the server
        // until its first message is sent.
        while let Ok(command) = commands.try_recv() {
            assert!(!matches!(command, Command::StartThread { .. }));
        }
        app.read_with(cx, |app, cx| {
            let draft = app.thread.as_ref().unwrap().read(cx).draft_thread().cloned().unwrap();
            assert_eq!(draft.project_id, "project-1");
            assert_eq!(app.draft_threads, vec![draft]);
        });
        cx.update(|cx| {
            let view = app.read(cx).thread.clone().unwrap();
            view.update(cx, |_, cx| {
                cx.emit(ThreadViewEvent::Send("Fix the build\nthen run tests".into(), Vec::new()))
            });
        });
        let command = commands.try_recv().unwrap();
        assert!(
            matches!(&command, Command::StartThread { draft, title, .. } if draft.project_id == "project-1" && title == "Fix the build"),
            "the first send creates the thread"
        );
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("add-project", cx);
        })
        .unwrap();
        let command = commands.try_recv().unwrap();
        assert!(
            matches!(command,Command::Workspace { request: t3_client::WorkspaceRequest::BrowseDirectories { partial_path, .. }, .. } if partial_path == "C:/repos/")
        );
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("directory-picker-panel").visible());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn drafts_restore_by_server_and_environment(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (backend, _commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx))
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |app, cx| {
                let server = "http://localhost:3773".to_owned();
                app.handle_event(Event::EnvironmentId { server: server.clone(), id: "env-a".into() }, window, cx);
                app.handle_event(Event::Status(Status::Connected(server.clone())), window, cx);
                app.open_thread("same-thread".into(), window, cx);
                app.thread.as_ref().unwrap().update(cx, |view,cx| view.restore_draft("Environment A draft", false, window, cx));
                app.handle_event(Event::PairFinished(true), window, cx);
                assert_eq!(app.draft_store.environment(&server,"env-a").unwrap().thread_text["same-thread"], "Environment A draft");
                app.handle_event(Event::EnvironmentId { server: server.clone(), id: "env-b".into() }, window, cx);
                app.handle_event(Event::Status(Status::Connected(server.clone())), window, cx);
                app.open_thread("same-thread".into(), window, cx);
                assert!(app.thread.as_ref().unwrap().read(cx).draft(cx).is_empty());
                app.thread.as_ref().unwrap().update(cx, |view,cx| view.restore_draft("Environment B draft", false, window, cx));
                app.handle_event(Event::PairFinished(true), window, cx);
                app.handle_event(Event::EnvironmentId { server: server.clone(), id: "env-a".into() }, window, cx);
                app.handle_event(Event::Status(Status::Connected(server.clone())), window, cx);
                app.open_thread("same-thread".into(), window, cx);
                assert_eq!(app.thread.as_ref().unwrap().read(cx).draft(cx), "Environment A draft");
                assert_eq!(app.draft_store.environment(&server,"env-b").unwrap().thread_text["same-thread"], "Environment B draft");
                assert_eq!(app.active_environment, "env-a");
            });
        }).unwrap();
    }

    #[gpui_kit::test]
    fn failed_pairing_keeps_thread_and_drafts_success_clears_environment(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (backend, mut commands) = Backend::for_test();
        let (_events, receiver) = futures::channel::mpsc::unbounded();
        let (handle, app) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1000.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| T3App::new_with_backend(backend, receiver, window, cx)),
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_event(Event::Status(Status::Connected("Previous server".into())), window, cx);
                app.shell.threads.push(serde_json::from_value(json!({ "id": "thread-1", "projectId": "project-1", "title": "Test", "runtimeMode": "full-access" })).unwrap());
                app.open_thread("thread-1".into(), window, cx);
                app.thread.as_ref().unwrap().update(cx, |view, cx| view.restore_draft("Keep my draft", false, window, cx));
                app.handle_event(Event::Thread { thread_id: "thread-1".into(), item: serde_json::from_value(json!({ "kind": "snapshot", "snapshot": { "snapshotSequence": 1, "thread": { "id": "thread-1", "projectId": "project-1", "title": "Test", "messages": [] } } })).unwrap() }, window, cx);
                assert!(app.thread.as_ref().unwrap().read(cx).is_ready());
                app.handle_event(Event::ThreadUnavailable("thread-1".into()), window, cx);
                assert!(!app.thread.as_ref().unwrap().read(cx).is_ready());
                app.open_thread("thread-1".into(), window, cx);
                assert!(!app.thread.as_ref().unwrap().read(cx).is_ready());
                assert_eq!(app.thread.as_ref().unwrap().read(cx).draft(cx), "Keep my draft");
                app.drafts.insert("other-thread".into(), "Other draft".into());
                app.pairing_link.update(cx, |input, cx| input.set_value("invalid-token", window, cx));
                app.pair(window, cx);
                app.pair(window, cx);
                assert!(app.pairing_pending);
                assert_eq!(app.thread.as_ref().unwrap().read(cx).draft(cx), "Keep my draft");
                app.handle_event(Event::Error("Pairing failed".into()), window, cx);
                app.handle_event(Event::PairFinished(false), window, cx);
                app.handle_event(Event::Status(Status::Connected("Previous server".into())), window, cx);
                assert!(!app.pairing_pending);
                assert!(app.switching_server);
                assert_eq!(app.error.as_deref(), Some("Pairing failed"));
                assert_eq!(app.thread.as_ref().unwrap().read(cx).draft(cx), "Keep my draft");
                assert_eq!(app.drafts.get("other-thread").map(String::as_str), Some("Other draft"));
                assert_eq!(app.pairing_link.read(cx).value(), "invalid-token");
                app.pairing_link.update(cx, |input, cx| input.set_value("new-token", window, cx));
                app.pair(window, cx);
                app.handle_event(Event::PairFinished(true), window, cx);
                assert!(app.thread.is_none());
                assert!(app.drafts.is_empty());
                assert!(app.shell.threads.is_empty());
                assert!(app.pairing_link.read(cx).value().is_empty());
            });
            window.render_frame(cx);
        }).unwrap();
        assert!(matches!(commands.try_recv(), Ok(Command::OpenThread(_))));
        assert!(matches!(commands.try_recv(), Ok(Command::OpenThread(_))));
        assert!(matches!(commands.try_recv(), Ok(Command::Pair(link)) if link == "invalid-token"));
        assert!(matches!(commands.try_recv(), Ok(Command::Pair(link)) if link == "new-token"));
        assert!(commands.try_recv().is_err());
    }
}
