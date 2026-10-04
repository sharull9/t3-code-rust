//! One open thread: transcript, working state and composer.
//!
//! The view owns connection/shell state and the composer; the transcript
//! itself (rows, scroll position) lives in its own entity, [`Transcript`] —
//! see `transcript.rs` for why. Sending and stopping are emitted as events;
//! the app turns them into backend commands.

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, Textarea, TextareaState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, Size, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{Session, ThreadShell, ThreadStreamItem};

use crate::attachments::{AttachmentPanel, AttachmentPanelEvent};
use crate::drafts::DraftThread;
use crate::mentions::{self, MentionKind, MentionMenu, Pick};
use crate::model_picker::{ModelPicker, ModelPickerEvent};
use crate::transcript::{Transcript, TranscriptEvent};
use crate::ui;
use crate::user_input::{UserInputEvent, UserInputPanel};

pub enum ThreadViewEvent {
    Send(String, Vec<t3_client::attachments::UploadedAttachment>),
    DraftChanged(String),
    QuestionDraftsChanged(crate::user_input::QuestionDrafts),
    Attachment(AttachmentPanelEvent),
    Stop,
    OpenAttachment(t3_client::attachments::UploadedAttachment),
    Update(t3_client::ThreadAction),
    /// A draft thread's model or modes changed; nothing is sent to the server.
    DraftSettingsChanged(DraftThread),
    /// Continue this thread in a new one on another provider's model.
    ContinueInNewThread(serde_json::Value),
    /// Search the thread's workspace for `@` mention candidates; the app
    /// answers with [`ThreadView::apply_file_results`].
    SearchFiles { request_id: u64, query: String },
    /// The composer's limit meters were clicked: show every limit.
    OpenUsageLimits,
}

pub struct ThreadView {
    thread_id: String,
    transcript: Entity<Transcript>,
    composer: Entity<TextareaState>,
    /// The thread's shell entry. It carries the modes, and can report a turn
    /// before detail catches up.
    shell: Option<ThreadShell>,
    connected: bool,
    providers: Vec<t3_client::ServerProvider>,
    sending: bool,
    approvals: Vec<t3_client::pending::PendingApproval>,
    pending_update: Option<t3_client::ThreadAction>,
    user_input: Entity<UserInputPanel>,
    attachments: Entity<AttachmentPanel>,
    thread_loaded: bool,
    model_picker: Entity<ModelPicker>,
    /// Set while this view composes a thread that does not exist on the
    /// server yet, with the project title for its heading.
    draft: Option<(DraftThread, SharedString)>,
    /// The workspace the composer's mentions refer to.
    cwd: Option<String>,
    mention: Option<MentionMenu>,
    /// Start of a mention the user dismissed with Escape; it stays closed
    /// until the cursor leaves that word.
    dismissed_mention: Option<usize>,
    next_file_search: u64,
    /// Names of the skills the composer can chip; a draft restored before
    /// they arrived is chipped again once they change.
    skill_names: Vec<String>,
    retokenize_pending: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

impl ThreadView {
    /// Redraws after an appearance preference changed: this view and its
    /// transcript are cached, so they would otherwise keep their last frame.
    pub fn refresh_appearance(&mut self, cx: &mut Context<Self>) {
        self.transcript.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    #[cfg(test)]
    pub fn new(
        thread_id: String,
        user_input: Entity<UserInputPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let attachments = cx.new(AttachmentPanel::new);
        Self::new_with_attachments(thread_id, user_input, attachments, window, cx)
    }

    pub fn new_with_attachments(
        thread_id: String,
        user_input: Entity<UserInputPanel>,
        attachments: Entity<AttachmentPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let transcript = cx.new(Transcript::new);
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .submit_on_enter(true)
                .placeholder("Ask anything  (Shift+Enter for a new line)")
        });
        let mut subscriptions =
            vec![cx.subscribe_in(&composer, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                    InputEvent::Change => {
                        cx.emit(ThreadViewEvent::DraftChanged(this.draft(cx)));
                        this.refresh_mention(cx);
                        cx.notify();
                    }
                    InputEvent::Blur => this.close_mention(cx),
                    _ => {}
                }
            })];
        user_input.update(cx, |panel, cx| panel.set_connected(false, cx));
        subscriptions.push(cx.subscribe_in(
            &user_input,
            window,
            |this, _, event: &UserInputEvent, window, cx| match event {
                UserInputEvent::DraftChanged(drafts) => {
                    cx.emit(ThreadViewEvent::QuestionDraftsChanged(drafts.clone()))
                }
                UserInputEvent::Respond(action) => cx.emit(ThreadViewEvent::Update(action.clone())),
                UserInputEvent::DisplacedText(text) => {
                    let draft = this.draft(cx);
                    let draft = if draft.trim().is_empty() {
                        text.clone()
                    } else {
                        format!("{}\n\n{text}", draft.trim_end())
                    };
                    let content = this.tokenized(&draft);
                    this.composer.update(cx, |state, cx| state.set_value(content, window, cx));
                }
            },
        ));
        subscriptions.push(cx.subscribe(&transcript, |_, _, event: &TranscriptEvent, cx| {
            let TranscriptEvent::OpenAttachment(attachment) = event;
            cx.emit(ThreadViewEvent::OpenAttachment(attachment.clone()));
        }));
        subscriptions.push(cx.subscribe(&attachments, |_, _, event: &AttachmentPanelEvent, cx| {
            cx.emit(ThreadViewEvent::Attachment(event.clone()))
        }));
        subscriptions.push(cx.observe(&attachments, |_, _, cx| cx.notify()));
        let model_picker = cx.new(|cx| ModelPicker::new(window, cx));
        subscriptions.push(cx.subscribe_in(
            &model_picker,
            window,
            |this, _, event: &ModelPickerEvent, window, cx| {
                match event {
                    ModelPickerEvent::Select(model) => {
                        this.update_setting(t3_client::ThreadAction::Model(model.clone()), cx)
                    }
                    ModelPickerEvent::ContinueInNewThread(model) => {
                        cx.emit(ThreadViewEvent::ContinueInNewThread(model.clone()))
                    }
                    ModelPickerEvent::Dismiss => {}
                }
                this.focus_composer(window, cx);
                cx.notify();
            },
        ));
        composer.update(cx, |state, cx| state.focus(window, cx));

        Self {
            thread_id,
            transcript,
            composer,
            shell: None,
            connected: true,
            providers: Vec::new(),
            sending: false,
            approvals: Vec::new(),
            pending_update: None,
            user_input,
            attachments,
            thread_loaded: false,
            model_picker,
            draft: None,
            cwd: None,
            mention: None,
            dismissed_mention: None,
            next_file_search: 0,
            skill_names: Vec::new(),
            retokenize_pending: false,
            _subscriptions: subscriptions,
        }
    }

    /// A view for a draft thread: the composer under a heading, sendable as
    /// soon as the server is connected. The first send creates the thread.
    pub fn new_draft(
        draft: DraftThread,
        project_title: SharedString,
        user_input: Entity<UserInputPanel>,
        attachments: Entity<AttachmentPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view =
            Self::new_with_attachments(draft.id.clone(), user_input, attachments, window, cx);
        view.draft = Some((draft, project_title));
        view.thread_loaded = true;
        view.attachments.update(cx, |panel, cx| panel.set_connected(view.connected, cx));
        view.sync_model_picker(cx);
        view
    }

    pub fn is_draft(&self) -> bool {
        self.draft.is_some()
    }

    pub fn draft_thread(&self) -> Option<&DraftThread> {
        self.draft.as_ref().map(|(draft, _)| draft)
    }

    /// The model the composer shows: the draft's, else the thread's.
    fn selection(&self) -> Option<serde_json::Value> {
        match &self.draft {
            Some((draft, _)) => Some(draft.model_selection.clone()),
            None => self.shell.as_ref().and_then(|t| t.model_selection.clone()),
        }
    }

    /// A thread that has run a turn is bound to its provider instance.
    fn is_started(&self, cx: &App) -> bool {
        self.draft.is_none()
            && (self.shell.as_ref().is_some_and(ThreadShell::is_started)
                || self.transcript.read(cx).has_messages())
    }

    fn sync_model_picker(&mut self, cx: &mut Context<Self>) {
        let (providers, selection, started) =
            (self.providers.clone(), self.selection(), self.is_started(cx));
        self.model_picker
            .update(cx, |picker, cx| picker.set_context(providers, selection, started, cx));
        let skill_names: Vec<String> = self
            .selected_provider()
            .map(|provider| provider.invocable_skills(self.cwd.as_deref()))
            .unwrap_or_default()
            .into_iter()
            .map(|skill| skill.name.clone())
            .collect();
        if skill_names != self.skill_names {
            self.skill_names = skill_names;
            self.retokenize_pending = true;
            cx.notify();
        }
    }

    /// Chips the mentions in the composer that are still plain text, such
    /// as `$skill` in a draft restored before the provider's skills arrived.
    /// Existing chips and the cursor stay where they are.
    fn retokenize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.tokenized(&self.draft(cx));
        let existing = self.composer.read(cx).tokens().to_vec();
        let missing: Vec<_> = content
            .tokens()
            .iter()
            .filter(|span| {
                !existing.iter().any(|chip| {
                    chip.range().start < span.range().end && span.range().start < chip.range().end
                })
            })
            .cloned()
            .collect();
        if missing.is_empty() {
            return;
        }
        self.composer.update(cx, |state, cx| {
            let selection = state.selected_range();
            for span in missing.iter().rev() {
                let _ = state.replace_range_with_token(span.range(), span.token().clone(), window, cx);
            }
            // A chip's text equals the text it replaces, so offsets are unchanged.
            state.set_selected_range(selection, cx);
        });
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn is_ready(&self) -> bool {
        self.connected && self.thread_loaded
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub fn draft(&self, cx: &App) -> String {
        self.composer.read(cx).value().to_string()
    }

    pub fn restore_draft(
        &mut self,
        draft: &str,
        sending: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.tokenized(draft);
        self.composer.update(cx, |state, cx| state.set_value(content, window, cx));
        self.sending = sending;
        cx.notify();
    }

    pub fn send_finished(
        &mut self,
        text: &str,
        success: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sending = false;
        if success && self.draft(cx).trim() == text {
            self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    pub fn set_providers(
        &mut self,
        providers: Vec<t3_client::ServerProvider>,
        cx: &mut Context<Self>,
    ) {
        self.providers = providers;
        self.sync_model_picker(cx);
        cx.notify();
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
        self.sync_model_picker(cx);
        if self.pending_update.as_ref().is_some_and(|action| self.shell_has_action(action)) {
            self.pending_update = None;
            cx.notify();
        }
    }

    fn shell_has_action(&self, action: &t3_client::ThreadAction) -> bool {
        let Some(shell) = &self.shell else {
            return false;
        };
        match action {
            t3_client::ThreadAction::Model(model) => {
                shell.model_selection.as_ref().is_some_and(|selected| {
                    selected["instanceId"] == model["instanceId"]
                        && selected["model"] == model["model"]
                })
            }
            t3_client::ThreadAction::RuntimeMode(mode) => shell.runtime_mode == *mode,
            t3_client::ThreadAction::InteractionMode(mode) => shell.interaction_mode == *mode,
            _ => true,
        }
    }

    /// Chooses whether a draft starts in a new worktree (see `DraftThread`).
    pub fn set_draft_worktree(&mut self, new_worktree: bool, cx: &mut Context<Self>) {
        if let Some((draft, _)) = &mut self.draft
            && draft.new_worktree != new_worktree
        {
            draft.new_worktree = new_worktree;
            cx.emit(ThreadViewEvent::DraftSettingsChanged(draft.clone()));
            cx.notify();
        }
    }

    fn update_setting(&mut self, action: t3_client::ThreadAction, cx: &mut Context<Self>) {
        if let Some((draft, _)) = &mut self.draft {
            match action {
                t3_client::ThreadAction::Model(model) => draft.model_selection = model,
                t3_client::ThreadAction::RuntimeMode(mode) => draft.runtime_mode = mode,
                t3_client::ThreadAction::InteractionMode(mode) => draft.interaction_mode = mode,
                _ => return,
            }
            cx.emit(ThreadViewEvent::DraftSettingsChanged(draft.clone()));
            self.sync_model_picker(cx);
            cx.notify();
            return;
        }
        self.pending_update = Some(action.clone());
        cx.emit(ThreadViewEvent::Update(action));
        cx.notify();
    }

    pub fn update_finished(
        &mut self,
        action: &t3_client::ThreadAction,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        if self.pending_update.as_ref() == Some(action)
            && (!success || self.shell_has_action(action))
        {
            self.pending_update = None;
            cx.notify();
        }
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        self.user_input
            .update(cx, |panel, cx| panel.set_connected(connected && self.thread_loaded, cx));
        if !connected {
            self.sending = false;
            self.pending_update = None;
        }
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        // A draft has no server state to resubscribe to.
        if self.draft.is_some() {
            return;
        }
        self.thread_loaded = false;
        self.user_input.update(cx, |panel, cx| panel.suspend(cx));
        self.pending_update = None;
        self.approvals.clear();
        self.transcript.update(cx, |transcript, cx| transcript.reset(cx));
    }

    pub fn apply(&mut self, item: ThreadStreamItem, window: &mut Window, cx: &mut Context<Self>) {
        let working = self.is_working(cx);
        self.transcript.update(cx, |transcript, cx| transcript.apply(item, cx));
        let approvals = self.transcript.read(cx).approvals();
        if let Some(requests) = self.transcript.read(cx).user_inputs() {
            let has_requests = self.user_input.read(cx).has_requests();
            self.user_input.update(cx, |panel, cx| {
                panel.set_requests(requests, window, cx);
                panel.set_connected(self.connected, cx);
            });
            if !self.thread_loaded || has_requests != self.user_input.read(cx).has_requests() {
                cx.notify();
            }
            self.thread_loaded = true;
            self.attachments.update(cx, |panel, cx| panel.set_connected(self.connected, cx));
        }
        if self.approvals != approvals || working != self.is_working(cx) {
            self.approvals = approvals;
            cx.notify();
        }
        if !self.model_picker.read(cx).is_open() {
            self.sync_model_picker(cx);
        }
    }

    fn is_working(&self, cx: &App) -> bool {
        let shell_session = self.shell.as_ref().and_then(|t| t.session.as_ref());
        let detail_session = self.transcript.read(cx).session();
        shell_session.is_some_and(|s| s.is_working())
            || detail_session.is_some_and(|s| s.is_working())
    }

    pub fn set_cwd(&mut self, cwd: Option<String>) {
        self.cwd = cwd;
    }

    fn selected_provider(&self) -> Option<&t3_client::ServerProvider> {
        let selection = self.selection()?;
        self.providers.iter().find(|p| selection["instanceId"] == p.instance_id)
    }

    /// `text` with its `@` and `$` mentions as chips.
    fn tokenized(&self, text: &str) -> gpui_kit::component::input::InputContent {
        let skills = self
            .selected_provider()
            .map(|provider| provider.invocable_skills(self.cwd.as_deref()))
            .unwrap_or_default();
        mentions::tokenize(text, |name| {
            skills.iter().find(|skill| skill.name == name).map(|skill| skill.label())
        })
    }

    /// Opens, updates or closes the suggestion menu for the word at the cursor.
    fn refresh_mention(&mut self, cx: &mut Context<Self>) {
        let state = self.composer.read(cx);
        let selection = state.selected_range();
        let trigger = (selection.is_empty())
            .then(|| mentions::detect_trigger(&state.value(), state.cursor()))
            .flatten()
            // Editing next to an existing chip is not a new mention.
            .filter(|trigger| {
                !state.tokens().iter().any(|span| {
                    span.range().start < trigger.range.end && trigger.range.start < span.range().end
                })
            })
            .filter(|_| self.connected && self.thread_loaded);
        let Some(trigger) = trigger else {
            self.dismissed_mention = None;
            return self.close_mention(cx);
        };
        if self.dismissed_mention == Some(trigger.range.start) {
            return self.close_mention(cx);
        }
        self.dismissed_mention = None;
        if self.mention.as_ref().is_some_and(|menu| menu.trigger == trigger) {
            return;
        }
        let mut menu = MentionMenu::new(trigger.clone());
        // Keep the previous results on screen until the new search answers.
        if let Some(previous) = self.mention.take()
            && previous.trigger.kind == trigger.kind
            && previous.trigger.range.start == trigger.range.start
        {
            menu.items = previous.items;
        }
        match trigger.kind {
            MentionKind::Skill => {
                let skills = self
                    .selected_provider()
                    .map(|provider| provider.invocable_skills(self.cwd.as_deref()))
                    .unwrap_or_default();
                menu.items = mentions::search_skills(&skills, &trigger.query);
            }
            MentionKind::Command => {
                let text = self.composer.read(cx).value();
                let otherwise_empty = text[..trigger.range.start].trim().is_empty()
                    && text[trigger.range.end..].trim().is_empty()
                    && self.attachments.read(cx).is_empty();
                let commands = self
                    .selected_provider()
                    .map(|provider| provider.slash_commands(self.cwd.as_deref()))
                    .unwrap_or_default();
                menu.items = mentions::search_commands(
                    mentions::BuiltinCommands { interaction_modes: true },
                    &commands,
                    &trigger.query,
                    text[..trigger.range.start].trim().is_empty(),
                    otherwise_empty,
                );
            }
            MentionKind::File => {
                self.next_file_search += 1;
                menu.pending_request = Some(self.next_file_search);
                cx.emit(ThreadViewEvent::SearchFiles {
                    request_id: self.next_file_search,
                    query: trigger.query.clone(),
                });
            }
        }
        self.mention = Some(menu);
        cx.notify();
    }

    fn close_mention(&mut self, cx: &mut Context<Self>) {
        if self.mention.take().is_some() {
            cx.notify();
        }
    }

    /// Results of a [`ThreadViewEvent::SearchFiles`]; stale answers are ignored.
    pub fn apply_file_results(
        &mut self,
        request_id: u64,
        result: Result<Vec<t3_client::WorkspaceEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.mention.as_mut().filter(|m| m.pending_request == Some(request_id))
        else {
            return;
        };
        menu.pending_request = None;
        match result {
            Ok(entries) => {
                menu.items = mentions::file_suggestions(&entries);
                menu.error = None;
            }
            Err(error) => {
                menu.items.clear();
                menu.error = Some(error.into());
            }
        }
        menu.highlighted = menu.highlighted.min(menu.items.len().saturating_sub(1));
        cx.notify();
    }

    fn mention_has_items(&self) -> bool {
        self.mention.as_ref().is_some_and(|menu| !menu.items.is_empty())
    }

    /// Replaces the typed `@query`/`$query` with the chosen chip and a
    /// space, or carries out the chosen `/` command.
    fn choose_mention(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.mention.take() else { return };
        let Some(item) = menu.items.get(ix).cloned() else {
            self.mention = Some(menu);
            return;
        };
        let range = menu.trigger.range.clone();
        let replacement = match &item.pick {
            Pick::Token(_) => None,
            Pick::Text(text) => Some(text.clone()),
            Pick::OpenModelPicker | Pick::InteractionMode(_) => Some(String::new()),
        };
        self.composer.update(cx, |state, cx| {
            match (&item.pick, replacement) {
                (Pick::Token(token), _) => {
                    if state.replace_range_with_token(range, token.clone(), window, cx).is_ok() {
                        state.insert(" ", window, cx);
                    }
                }
                (_, Some(text)) => {
                    state.set_selected_range(range, cx);
                    state.replace(text, window, cx);
                }
                _ => {}
            }
            state.focus(window, cx);
        });
        // Settings can't change mid-turn or while another change is in flight.
        let settings_locked = !self.connected
            || !self.thread_loaded
            || self.sending
            || self.pending_update.is_some()
            || self.is_working(cx);
        match item.pick {
            Pick::OpenModelPicker if !settings_locked => {
                self.model_picker.update(cx, |picker, cx| picker.open(window, cx));
            }
            Pick::InteractionMode(mode) if !settings_locked => {
                self.update_setting(t3_client::ThreadAction::InteractionMode(mode.into()), cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_mention_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if !action.shift && self.mention_has_items() {
            cx.stop_propagation();
            let ix = self.mention.as_ref().map_or(0, |menu| menu.highlighted);
            self.choose_mention(ix, window, cx);
        }
    }

    fn on_mention_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.mention_has_items() {
            cx.stop_propagation();
            let ix = self.mention.as_ref().map_or(0, |menu| menu.highlighted);
            self.choose_mention(ix, window, cx);
        }
    }

    fn on_mention_escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.mention.take() {
            cx.stop_propagation();
            self.dismissed_mention = Some(menu.trigger.range.start);
            cx.notify();
        }
    }

    fn on_mention_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.mention.as_mut() {
            cx.stop_propagation();
            menu.move_highlight(-1);
            cx.notify();
        }
    }

    fn on_mention_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.mention.as_mut() {
            cx.stop_propagation();
            menu.move_highlight(1);
            cx.notify();
        }
    }

    /// Pasted images and copied files become attachments; pasted text keeps
    /// its line breaks with Windows and web artifacts cleaned up. Returns
    /// whether the paste was handled here.
    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut files = Vec::new();
        let mut image = None;
        let mut text = None;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => files.extend(paths.paths().iter().cloned()),
                ClipboardEntry::Image(pasted) => image = image.or(Some(pasted)),
                ClipboardEntry::String(string) => text = text.or(Some(string.text())),
            }
        }
        // Office apps put a picture of copied text beside the text itself;
        // only a bare image is an image paste.
        let image = image.filter(|_| files.is_empty() && text.is_none_or(|t| t.trim().is_empty()));
        if (!files.is_empty() || image.is_some()) && !(self.connected && self.thread_loaded) {
            cx.emit(ThreadViewEvent::Attachment(AttachmentPanelEvent::Rejected(
                "Wait for the thread to connect before pasting files or images.".into(),
            )));
            return true;
        }
        if let Some(image) = image {
            match crate::attachments::save_pasted_image(image) {
                Ok(path) => files.push(path),
                Err(error) => {
                    cx.emit(ThreadViewEvent::Attachment(AttachmentPanelEvent::Rejected(format!(
                        "Could not paste the image: {error}"
                    ))));
                    return true;
                }
            }
        }
        if !files.is_empty() {
            self.attachments.update(cx, |panel, cx| panel.add_paths(files, cx));
            return true;
        }
        let Some(text) = text else { return false };
        let cleaned = clean_pasted_text(text);
        if cleaned == *text {
            return false;
        }
        self.composer.update(cx, |state, cx| state.replace(cleaned, window, cx));
        true
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_owned();
        if (text.is_empty() && self.attachments.read(cx).uploaded_attachments().is_empty())
            || !self.attachments.read(cx).can_send()
            || !self.connected
            || !self.thread_loaded
            || self.sending
            || self.pending_update.is_some()
            || (self.thread_loaded && self.user_input.read(cx).has_requests())
            || self.is_working(cx)
        {
            return;
        }
        let _ = window;
        self.sending = true;
        cx.notify();
        cx.emit(ThreadViewEvent::Send(text, self.attachments.read(cx).uploaded_attachments()));
    }
}

impl Render for ThreadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.retokenize_pending) {
            cx.defer_in(window, |this, window, cx| this.retokenize(window, cx));
        }
        let theme = cx.theme();
        let content_width = ui::content_width(cx);
        let prompt_size = px(crate::prefs::Prefs::global(cx).font_size_prompt as f32);
        let working = self.is_working(cx);
        let has_user_input = self.thread_loaded && self.user_input.read(cx).has_requests();
        let transcript_state = self.transcript.read(cx);
        let shell = self.shell.as_ref();
        let branch = transcript_state
            .branch()
            .map(str::to_owned)
            .or_else(|| shell.and_then(|t| t.branch.clone()));
        let session = transcript_state.session().or_else(|| shell.and_then(|t| t.session.as_ref()));
        let session_error = session.and_then(|s| s.last_error.clone());
        let approvals = &self.approvals;
        // V2 sessions carry a provider instance id: show that instance's
        // display name when the server lists it.
        let provider = session.and_then(|s| s.provider_name.as_deref());
        let provider = self
            .providers
            .iter()
            .find(|p| Some(p.instance_id.as_str()) == provider)
            .map_or_else(|| ui::provider_label(provider), crate::model_picker::provider_name);
        let selection = self.selection();
        let model_id = selection.as_ref().and_then(|m| m["model"].as_str()).unwrap_or(&provider);
        // The server's display name ("Claude Opus 5.5"), not its id ("claude-opus-5-5").
        let selected_provider = selection.as_ref().and_then(|selection| {
            self.providers.iter().find(|p| selection["instanceId"] == p.instance_id)
        });
        let model_label = selected_provider
            .and_then(|provider| provider.models.iter().find(|model| model.id == model_id))
            .map_or_else(|| model_id.to_owned(), |model| model.label.clone());
        let settings_disabled = !self.connected
            || !self.thread_loaded
            || working
            || self.sending
            || self.pending_update.is_some()
            || (shell.is_none() && self.draft.is_none());
        // Provider mark first, then the model, like T3's composer chip.
        let model_mark = match selected_provider {
            Some(provider) => crate::model_picker::provider_mark(
                &provider.instance_id,
                &provider.driver,
                &crate::model_picker::provider_name(provider),
                px(14.),
            )
            .into_any_element(),
            None => Icon::new(IconName::Bot).xsmall().into_any_element(),
        };
        let model_button = Button::new("model-picker")
            .ghost()
            .small()
            .tooltip(model_label.clone())
            .max_w(px(240.))
            .disabled(settings_disabled)
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .items_center()
                    .child(model_mark)
                    .child(div().min_w_0().truncate().child(model_label))
                    .child(Icon::new(IconName::ChevronDown).xsmall()),
            );
        let picker = self.model_picker.clone();
        let model_picker = Popover::new("model-picker-popover")
            .anchor(Anchor::BottomLeft)
            .p_0()
            .overflow_hidden()
            .open(self.model_picker.read(cx).is_open())
            .on_open_change(move |open, window, cx| {
                picker.update(cx, |picker, cx| {
                    if *open && !settings_disabled {
                        picker.open(window, cx);
                    } else {
                        picker.close(cx);
                    }
                });
            })
            .trigger(model_button)
            .child(self.model_picker.clone());
        let (runtime_mode, interaction_mode) = match &self.draft {
            Some((draft, _)) => (draft.runtime_mode.clone(), draft.interaction_mode.clone()),
            None => (
                shell.map_or_else(String::new, |t| t.runtime_mode.clone()),
                shell.map_or_else(String::new, |t| t.interaction_mode.clone()),
            ),
        };
        let (runtime_label, runtime_icon) = ui::runtime_mode(&runtime_mode);
        let (mode_label, mode_icon) = ui::interaction_mode(&interaction_mode);

        // `Transcript` is embedded cached and styled to fill the remaining
        // column; it repaints only when it notifies itself (new stream
        // items, scrolling), never because this view redraws for an
        // unrelated reason such as the composer's loader animation below
        // (see `transcript.rs`).
        let transcript = match &self.draft {
            Some((_, project)) => draft_hero(project, cx).into_any_element(),
            None => self
                .transcript
                .clone()
                .cached(StyleRefinement::default().flex_1().min_h_0())
                .into_any_element(),
        };

        let action = Button::new(if working { "stop" } else { "send" })
            .ghost()
            .small()
            .size_8()
            .rounded_full()
            .icon(Icon::new(if working { IconName::Square } else { IconName::ArrowUp }).small())
            .tooltip(if working { "Stop response" } else { "Send message" })
            .text_color(theme.primary_foreground)
            .bg(if working { theme.danger } else { theme.primary })
            .disabled(
                !self.connected
                    || (!working
                        && (!self.thread_loaded
                            || self.sending
                            || has_user_input
                            || self.pending_update.is_some()
                            || !self.attachments.read(cx).can_send()
                            || (self.draft(cx).trim().is_empty()
                                && self.attachments.read(cx).uploaded_attachments().is_empty()))),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if working {
                    if this.connected {
                        cx.emit(ThreadViewEvent::Stop);
                    }
                } else {
                    this.submit(window, cx);
                }
            }));

        let attach_files = Button::new("attach-files")
            .ghost()
            .small()
            .icon(Icon::new(IconName::Paperclip).xsmall())
            .accessibility_label("Attach files")
            .tooltip("Attach files")
            .disabled(!self.connected || !self.thread_loaded)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(ThreadViewEvent::Attachment(AttachmentPanelEvent::ChooseFiles));
            }));
        let has_attachments = !self.attachments.read(cx).is_empty();
        let mode_controls = h_flex()
            .flex_1()
            .min_w_0()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(attach_files)
            .child(separator(cx))
            .child(model_picker)
            .child(separator(cx))
            .child(self.mode_picker(
                "runtime-picker",
                runtime_icon,
                runtime_label,
                &runtime_mode,
                true,
                settings_disabled,
                cx,
            ))
            .child(separator(cx))
            .child(self.mode_picker(
                "interaction-picker",
                mode_icon,
                mode_label,
                &interaction_mode,
                false,
                settings_disabled,
                cx,
            ));
        let trailing_controls = h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .when(working, |row| {
                row.child(div().mr_1().child(ui::loader("composer-working", Size::Small)))
            })
            .child(action);

        let composer_focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        let mention_menu = self.mention.as_ref().map(|menu| {
            let view = cx.entity().downgrade();
            let on_choose = move |ix: usize, window: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |view, cx| view.choose_mention(ix, window, cx));
            };
            deferred(
                div()
                    .absolute()
                    .bottom_full()
                    .left_0()
                    .w_full()
                    .pb_2()
                    .child(mentions::render_menu(menu, on_choose, cx)),
            )
            .with_priority(1)
        });
        let composer = v_flex()
            .relative()
            .w_full()
            .max_w(content_width)
            .children(mention_menu)
            .rounded_xl()
            .border_1()
            .border_color(if composer_focused { theme.primary.opacity(0.45) } else { theme.border })
            .bg(theme.secondary)
            .when(has_attachments, |composer| {
                composer.child(div().px_3().pt_2().child(self.attachments.clone()))
            })
            .child(
                div()
                    .px_3()
                    .pt_1()
                    .capture_action(cx.listener(Self::on_mention_enter))
                    .capture_action(cx.listener(Self::on_mention_tab))
                    .capture_action(cx.listener(Self::on_mention_escape))
                    .capture_action(cx.listener(Self::on_mention_up))
                    .capture_action(cx.listener(Self::on_mention_down))
                    .text_size(prompt_size)
                    .child(
                        Textarea::new(&self.composer)
                            .accessibility_id("composer")
                            .aria_label("Message")
                            .appearance(false)
                            .token(mentions::render_token)
                            .on_paste({
                                let view = cx.entity().downgrade();
                                move |item, window, cx| {
                                    view.update(cx, |view, cx| view.paste(item, window, cx))
                                        .unwrap_or(false)
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_3()
                    .pb_2()
                    .child(mode_controls)
                    .child(trailing_controls),
            );

        let footer = h_flex()
            .w_full()
            .max_w(content_width)
            .gap_1()
            .px_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .when(self.draft.is_some(), |footer| {
                footer
                    .child(Icon::new(IconName::Pencil).xsmall())
                    .child("Draft · the thread is created when you send")
            })
            .when(self.draft.is_none(), |footer| {
                footer.child(Icon::new(IconName::FolderClosed).xsmall()).child(
                    if shell.and_then(|t| t.worktree_path.as_ref()).is_some() {
                        "Worktree"
                    } else {
                        "Local checkout"
                    },
                )
            })
            .child(div().flex_1())
            .children(self.render_limits(cx))
            .children(branch.map(|branch| {
                h_flex().gap_1().child(Icon::new(IconName::GitBranch).xsmall()).child(branch)
            }));

        v_flex().size_full().min_h_0().child(transcript).child(
            v_flex()
                .items_center()
                .gap_2()
                .px_6()
                .pt_2()
                .pb_3()
                .children(session_error.map(|error| {
                    div()
                        .w_full()
                        .max_w(content_width)
                        .px_1()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error)
                }))
                .children(approvals.iter().take(1).enumerate().map(|(ix, approval)| {
                    v_flex()
                        .w_full()
                        .max_w(content_width)
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.warning.opacity(0.4))
                        .bg(theme.secondary)
                        .child(div().text_xs().text_color(theme.warning).child(
                            if approvals.len() > 1 {
                                format!("{} (1/{})", approval.summary, approvals.len())
                            } else {
                                approval.summary.clone()
                            },
                        ))
                        .children(
                            approval.detail.clone().map(|detail| div().text_sm().child(detail)),
                        )
                        .child(h_flex().flex_wrap().gap_2().children(
                            approval.options.iter().enumerate().map(|(option_ix, option)| {
                                let action = t3_client::ThreadAction::Approval {
                                    request_id: approval.request_id.clone(),
                                    decision: option.decision.clone(),
                                };
                                Button::new((
                                    SharedString::from(format!("approval-{ix}")),
                                    option_ix,
                                ))
                                .small()
                                .outline()
                                .label(option.label.clone())
                                .disabled(!self.connected)
                                .when_some(option.warning.clone(), |button, warning| {
                                    button.tooltip(warning)
                                })
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        cx.emit(ThreadViewEvent::Update(action.clone()))
                                    },
                                ))
                            }),
                        ))
                }))
                .when(self.thread_loaded && self.user_input.read(cx).has_requests(), |column| {
                    column.child(
                        self.user_input
                            .clone()
                            .cached(StyleRefinement::default().w_full().max_w(content_width)),
                    )
                })
                .child(composer)
                .child(footer),
        )
    }
}

/// The empty state of a draft thread, in place of the transcript.
impl ThreadView {
    /// The selected model's account limits, one small meter per window, so
    /// they're in view while writing. Hover for resets; click for the Limits
    /// tab.
    fn render_limits(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let provider = self.selected_provider()?;
        let limits = provider.usage_limits.as_ref().filter(|limits| limits.unavailable.is_none())?;
        if limits.windows.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let color = if matches!(provider.driver.as_str(), "claude" | "claudeAgent") {
            ui::hex(0xd97757)
        } else {
            theme.foreground
        };
        let fill = theme.muted.blend(color.opacity(0.6));
        let now = chrono::Utc::now();
        let mut details: Vec<String> = limits
            .windows
            .iter()
            .map(|window| {
                let reset = match window.reset() {
                    Some(at) if at > now => {
                        format!(" · resets in {}", crate::limits_view::duration((at - now).num_seconds()))
                    }
                    _ => String::new(),
                };
                format!("{}: {:.0}% left{reset}", window.label, window.remaining())
            })
            .collect();
        details.insert(0, crate::model_picker::provider_name(provider));
        details.push("Click to see every limit".into());
        let tooltip: SharedString = details.join("\n").into();
        Some(
            h_flex()
                .id("composer-limits")
                .test_support()
                .gap_3()
                .mr_3()
                .cursor_pointer()
                .hover(|style| style.text_color(theme.foreground))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadViewEvent::OpenUsageLimits)))
                .children(limits.windows.iter().map(|window| {
                    let remaining = window.remaining();
                    h_flex()
                        .gap_1()
                        .child(window.label.clone())
                        .child(
                            div()
                                .w(px(36.))
                                .h(px(4.))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(theme.muted)
                                .child(div().h_full().w(relative(remaining as f32 / 100.)).bg(fill)),
                        )
                        .child(format!("{remaining:.0}%"))
                }))
                .into_any_element(),
        )
    }
}

fn draft_hero(project: &SharedString, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .gap_3()
        .px_6()
        .child(
            div()
                .text_2xl()
                .font_semibold()
                .text_color(theme.foreground)
                .child(format!("What should we build in {project}?")),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Pick a model below and describe the task. Nothing is created until you send."),
        )
}

impl ThreadView {
    fn mode_picker(
        &self,
        id: &'static str,
        icon: IconName,
        label: &'static str,
        selected: &str,
        runtime: bool,
        disabled: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let selected = selected.to_owned();
        Button::new(id)
            .ghost()
            .small()
            .icon(Icon::new(icon).xsmall())
            .label(label)
            .disabled(disabled)
            .dropdown_menu(move |mut menu, _, _| {
                let choices: &[&str] = if runtime {
                    &["approval-required", "auto-accept-edits", "auto", "full-access"]
                } else {
                    &["default", "plan"]
                };
                for &mode in choices {
                    let label = if runtime {
                        ui::runtime_mode(mode).0
                    } else {
                        ui::interaction_mode(mode).0
                    };
                    let action = if runtime {
                        t3_client::ThreadAction::RuntimeMode(mode.into())
                    } else {
                        t3_client::ThreadAction::InteractionMode(mode.into())
                    };
                    let view = view.clone();
                    menu = menu.item(PopupMenuItem::new(label).checked(selected == mode).on_click(
                        move |_, _, cx| {
                            let _ =
                                view.update(cx, |view, cx| view.update_setting(action.clone(), cx));
                        },
                    ));
                }
                menu
            })
    }
}

fn separator(cx: &App) -> impl IntoElement {
    div().w_px().h_4().bg(cx.theme().border)
}

/// Windows line endings become `\n`, and the non-breaking spaces web pages
/// copy become plain spaces; indentation and blank lines are kept.
fn clean_pasted_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace(['\u{a0}', '\u{202f}'], " ")
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
                && a.model_selection == b.model_selection
                && a.worktree_path == b.worktree_path
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

#[cfg(test)]
mod composer_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    fn snapshot() -> ThreadStreamItem {
        serde_json::from_value(json!({ "kind": "snapshot", "snapshot": {
            "snapshotSequence": 1, "thread": { "id": "thread-1", "projectId": "project-1", "title": "Test", "messages": [], "activities": [] }
        } })).unwrap()
    }

    #[gpui_kit::test]
    fn sending_waits_for_detail_and_preserves_edits_and_failed_drafts(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1000.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let panel = cx.new(UserInputPanel::new);
                    cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
                },
            )
            .unwrap()
        });
        let sent = Rc::new(RefCell::new(Vec::new()));
        let capture = sent.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &ThreadViewEvent, _| {
                if let ThreadViewEvent::Send(text, _) = event {
                    capture.borrow_mut().push(text.clone());
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let composer_id = view.read(cx).composer.entity_id();
            window.click(("input", composer_id), cx);
            window.input("First message", cx);
            window.click("send", cx);
            assert!(!view.read(cx).sending);
            view.update(cx, |view, cx| view.apply(snapshot(), window, cx));
            window.render_frame(cx);
            window.click("send", cx);
            window.click("send", cx);
            assert!(view.read(cx).sending);
        })
        .unwrap();
        assert_eq!(*sent.borrow(), vec!["First message".to_owned()]);
        cx.update_window(handle, |_, window, cx| {
            let composer_id = view.read(cx).composer.entity_id();
            window.click(("input", composer_id), cx);
            window.input(" plus edits", cx);
            view.update(cx, |view, cx| view.send_finished("First message", true, window, cx));
            assert!(view.read(cx).draft(cx).contains("plus edits"));
            window.render_frame(cx);
            window.click("send", cx);
            let draft = view.read(cx).draft(cx);
            view.update(cx, |view, cx| view.send_finished(draft.trim(), false, window, cx));
            assert_eq!(view.read(cx).draft(cx), draft);
            view.update(cx, |view, cx| view.set_connected(false, cx));
            window.render_frame(cx);
            window.click("send", cx);
            assert!(!view.read(cx).sending);
            assert_eq!(view.read(cx).draft(cx), draft);
            view.update(cx, |view, cx| {
                view.reset(cx);
                view.set_connected(true, cx);
            });
            window.render_frame(cx);
            window.click("send", cx);
            assert!(!view.read(cx).sending);
        })
        .unwrap();
        assert_eq!(sent.borrow().len(), 2);
    }

    #[gpui_kit::test]
    fn attachment_picker_is_in_toolbar_and_disabled_until_thread_is_ready(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1000.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let panel = cx.new(UserInputPanel::new);
                    cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
                },
            )
            .unwrap()
        });
        let attachment_events = Rc::new(RefCell::new(Vec::new()));
        let capture = attachment_events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &ThreadViewEvent, _| {
                if let ThreadViewEvent::Attachment(event) = event {
                    capture.borrow_mut().push(matches!(event, AttachmentPanelEvent::ChooseFiles));
                }
            })
        });

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("attach-files", cx);
            view.update(cx, |view, cx| view.apply(snapshot(), window, cx));
            window.render_frame(cx);
            window.click("attach-files", cx);
            view.update(cx, |view, cx| view.set_connected(false, cx));
            window.render_frame(cx);
            window.click("attach-files", cx);
        })
        .unwrap();
        assert_eq!(*attachment_events.borrow(), [true]);
    }

    #[gpui_kit::test]
    fn mentions_insert_chips_and_enter_picks_instead_of_sending(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let draft = DraftThread {
            id: "draft-1".into(),
            project_id: "project-1".into(),
            model_selection: json!({ "instanceId": "claude-a", "model": "opus" }),
            runtime_mode: "full-access".into(),
            interaction_mode: "default".into(),
            new_worktree: false,
        };
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let panel = cx.new(UserInputPanel::new);
                let attachments = cx.new(AttachmentPanel::new);
                cx.new(|cx| {
                    ThreadView::new_draft(draft, "Demo".into(), panel, attachments, window, cx)
                })
            })
            .unwrap()
        });
        let providers: Vec<t3_client::ServerProvider> = serde_json::from_value(json!([
            {"instanceId":"claude-a","driver":"claudeAgent","enabled":true,"installed":true,
             "models":[{"slug":"opus","name":"Opus"}],
             "skills":[{"name":"repo-explorer","path":"/s/repo","enabled":true,"scope":"user"},
                       {"name":"fallow","path":"/s/fallow","enabled":true},
                       {"name":"agent-only","path":"/s/a","enabled":true,"userInvocable":false}]}
        ]))
        .unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let capture = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &ThreadViewEvent, _| match event {
                ThreadViewEvent::Send(text, _) => capture.borrow_mut().push(format!("send:{text}")),
                ThreadViewEvent::SearchFiles { request_id, query } => {
                    capture.borrow_mut().push(format!("search:{request_id}:{query}"))
                }
                _ => {}
            })
        });
        view.update(cx, |view, cx| view.set_providers(providers, cx));

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let composer_id = view.read(cx).composer.entity_id();
            window.click(("input", composer_id), cx);
            window.input("use $rep", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(("mention-row", 0usize)).visible());
            assert!(window.try_find(("mention-row", 1usize)).is_none());
            window.press("enter", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(view.read(cx).draft(cx), "use $repo-explorer ");
            let tokens = view.read(cx).composer.read(cx).tokens().to_vec();
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].token().label().as_ref(), "Repo Explorer");
            window.input("@", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| window.input("ind", cx)).unwrap();
        let request_id = events
            .borrow()
            .iter()
            .rev()
            .find_map(|event| event.strip_prefix("search:").map(str::to_owned))
            .and_then(|event| event.split(':').next()?.parse::<u64>().ok())
            .expect("a file search");
        cx.update_window(handle, |_, window, cx| {
            let entries = vec![t3_client::WorkspaceEntry {
                path: "src/api/index.ts".into(),
                kind: "file".into(),
                ignored: false,
            }];
            view.update(cx, |view, cx| view.apply_file_results(request_id, Ok(entries), cx));
            window.render_frame(cx);
            window.press("tab", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(view.read(cx).draft(cx), "use $repo-explorer @src/api/index.ts ");
            assert_eq!(view.read(cx).composer.read(cx).tokens().len(), 2);
            assert!(view.read(cx).mention.is_none());
            window.input("$", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert!(view.read(cx).mention.is_none(), "Escape dismisses the menu");
            window.press("backspace", cx);
            window.press("enter", cx);
        })
        .unwrap();
        assert!(events.borrow().iter().any(|e| e == "send:use $repo-explorer @src/api/index.ts"));
        assert!(events.borrow().iter().any(|e| e == "search:1:"), "`@` alone browses recent files");
        assert!(events.borrow().iter().any(|e| e == "search:2:ind"));
    }

    #[gpui_kit::test]
    fn slash_commands_type_provider_commands_and_switch_modes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let draft = DraftThread {
            id: "draft-1".into(),
            project_id: "project-1".into(),
            model_selection: json!({ "instanceId": "claude-a", "model": "opus" }),
            runtime_mode: "full-access".into(),
            interaction_mode: "default".into(),
            new_worktree: false,
        };
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let panel = cx.new(UserInputPanel::new);
                let attachments = cx.new(AttachmentPanel::new);
                cx.new(|cx| {
                    ThreadView::new_draft(draft, "Demo".into(), panel, attachments, window, cx)
                })
            })
            .unwrap()
        });
        let providers: Vec<t3_client::ServerProvider> = serde_json::from_value(json!([
            {"instanceId":"claude-a","driver":"claudeAgent","enabled":true,"installed":true,
             "models":[{"slug":"opus","name":"Opus"}],
             "slashCommands":[{"name":"compact","description":"Compact the conversation"},
                              {"name":"review","description":"Review the changes"}],
             "usageLimits":{"checkedAt":"2026-10-04T00:00:00Z","windows":[
                {"id":"session","kind":"session","label":"Session","usedPercent":15}]}}
        ]))
        .unwrap();
        view.update(cx, |view, cx| view.set_providers(providers, cx));

        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("composer-limits").visible(), "limits show under the composer");
            let composer_id = view.read(cx).composer.entity_id();
            window.click(("input", composer_id), cx);
            window.input("/rev", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(("mention-row", 0usize)).visible());
            window.press("enter", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(view.read(cx).draft(cx), "/review ");
            assert!(view.read(cx).composer.read(cx).tokens().is_empty(), "typed, not chipped");
            window.input("then /pla", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // Mid-message only the composer's own commands are offered.
            let menu = view.read(cx).mention.as_ref().unwrap();
            assert_eq!(menu.items.len(), 1);
            assert_eq!(menu.items[0].title.as_ref(), "/plan");
            window.press("enter", cx);
        })
        .unwrap();
        cx.update_window(handle, |_, _, cx| {
            assert_eq!(view.read(cx).draft(cx), "/review then ");
            let (draft, _) = view.read(cx).draft.as_ref().unwrap();
            assert_eq!(draft.interaction_mode, "plan");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn restored_drafts_show_mentions_as_chips(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let panel = cx.new(UserInputPanel::new);
                cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.restore_draft("check @\"docs/a b.md\" now", false, window, cx)
            });
            let tokens = view.read(cx).composer.read(cx).tokens().to_vec();
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].token().label().as_ref(), "a b.md");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn skills_in_a_restored_draft_become_chips_once_the_provider_arrives(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let panel = cx.new(UserInputPanel::new);
                cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
            })
            .unwrap()
        });
        let shell: ThreadShell = serde_json::from_value(json!({
            "id": "thread-1", "projectId": "project-1", "title": "T", "runtimeMode": "full-access",
            "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-01T00:00:00Z",
            "modelSelection": { "instanceId": "claude-a", "model": "opus" }
        }))
        .unwrap();
        let providers: Vec<t3_client::ServerProvider> = serde_json::from_value(json!([
            {"instanceId":"claude-a","driver":"claudeAgent","enabled":true,"installed":true,
             "models":[{"slug":"opus","name":"Opus"}],
             "skills":[{"name":"fallow","path":"/s/fallow","enabled":true}]}
        ]))
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| view.restore_draft("run $fallow on @src/a.ts now", false, window, cx));
            assert_eq!(view.read(cx).composer.read(cx).tokens().len(), 1, "skills are unknown yet");
            view.update(cx, |view, cx| {
                view.set_shell(Some(shell), cx);
                view.set_providers(providers, cx);
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.update_window(handle, |_, _, cx| {
            let tokens = view.read(cx).composer.read(cx).tokens().to_vec();
            let labels: Vec<_> = tokens.iter().map(|t| t.token().label().to_string()).collect();
            assert_eq!(labels, ["Fallow", "a.ts"]);
            assert_eq!(view.read(cx).draft(cx), "run $fallow on @src/a.ts now");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn pasting_files_before_the_thread_is_ready_reports_an_error(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let panel = cx.new(UserInputPanel::new);
                cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
            })
            .unwrap()
        });
        let rejected = Rc::new(RefCell::new(Vec::new()));
        let capture = rejected.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &ThreadViewEvent, _| {
                if let ThreadViewEvent::Attachment(AttachmentPanelEvent::Rejected(message)) = event {
                    capture.borrow_mut().push(message.clone());
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            let item = ClipboardItem {
                entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                    vec![std::path::PathBuf::from("C:/missing/file.txt")].into(),
                ))],
            };
            let handled = view.update(cx, |view, cx| view.paste(&item, window, cx));
            assert!(handled);
        })
        .unwrap();
        assert_eq!(rejected.borrow().len(), 1, "the paste is reported, not silently dropped");
    }

    #[test]
    fn pasted_text_keeps_lines_without_windows_artifacts() {
        assert_eq!(clean_pasted_text("fn a() {\r\n\tb();\r\n}\r\n"), "fn a() {\n\tb();\n}\n");
        assert_eq!(clean_pasted_text("a\u{a0}b\rc"), "a b\nc");
    }

    #[gpui_kit::test]
    fn send_action_stays_inside_a_narrow_composer(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(384.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let panel = cx.new(UserInputPanel::new);
                    cx.new(|cx| ThreadView::new("thread-1".into(), panel, window, cx))
                },
            )
            .unwrap()
        });

        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| view.apply(snapshot(), window, cx));
            window.render_frame(cx);
            let send = window.find("send").bounds();
            assert!(send.origin.x + send.size.width <= px(384.));
            assert!(window.find("attach-files").visible());
            assert!(window.find("model-picker").visible());
            assert!(window.find("runtime-picker").visible());
            assert!(window.find("interaction-picker").visible());
        })
        .unwrap();
    }
}
