//! One open thread: transcript, working state and composer.
//!
//! The view owns connection/shell state and the composer; the transcript
//! itself (rows, scroll position) lives in its own entity, [`Transcript`] —
//! see `transcript.rs` for why. Sending and stopping are emitted as events;
//! the app turns them into backend commands.

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::{Session, ThreadShell, ThreadStreamItem};

use crate::transcript::Transcript;
use crate::ui::{self, CONTENT_WIDTH};
use crate::user_input::{UserInputEvent, UserInputPanel};

pub enum ThreadViewEvent {
    Send(String),
    Stop,
    Update(t3_client::ThreadAction),
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
    thread_loaded: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

impl ThreadView {
    pub fn new(
        thread_id: String,
        user_input: Entity<UserInputPanel>,
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
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.submit(window, cx);
                }
            })];
        user_input.update(cx, |panel, cx| panel.set_connected(false, cx));
        subscriptions.push(cx.subscribe_in(
            &user_input,
            window,
            |this, _, event: &UserInputEvent, window, cx| match event {
                UserInputEvent::Respond(action) => cx.emit(ThreadViewEvent::Update(action.clone())),
                UserInputEvent::DisplacedText(text) => {
                    let draft = this.draft(cx);
                    let draft = if draft.trim().is_empty() {
                        text.clone()
                    } else {
                        format!("{}\n\n{text}", draft.trim_end())
                    };
                    this.composer.update(cx, |state, cx| state.set_value(draft, window, cx));
                }
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
            thread_loaded: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
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
        self.composer.update(cx, |state, cx| state.set_value(draft, window, cx));
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

    fn update_setting(&mut self, action: t3_client::ThreadAction, cx: &mut Context<Self>) {
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
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }

    /// A reconnect resubscribes and resends the snapshot; drop the stale copy.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
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
        }
        if self.approvals != approvals || working != self.is_working(cx) {
            self.approvals = approvals;
            cx.notify();
        }
    }

    fn is_working(&self, cx: &App) -> bool {
        let shell_session = self.shell.as_ref().and_then(|t| t.session.as_ref());
        let detail_session = self.transcript.read(cx).session();
        shell_session.is_some_and(|s| s.is_working())
            || detail_session.is_some_and(|s| s.is_working())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_owned();
        if text.is_empty()
            || !self.connected
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
        cx.emit(ThreadViewEvent::Send(text));
    }
}

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
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
        let provider = ui::provider_label(session.and_then(|s| s.provider_name.as_deref()));
        let model_label = shell
            .and_then(|t| t.model_selection.as_ref())
            .and_then(|m| m.get("model"))
            .and_then(|m| m.as_str())
            .unwrap_or(&provider)
            .to_owned();
        let selection = shell.and_then(|t| t.model_selection.clone());
        let providers = self.providers.clone();
        let view = cx.entity().downgrade();
        let settings_disabled = !self.connected
            || working
            || self.sending
            || self.pending_update.is_some()
            || shell.is_none();
        let model_picker = Button::new("model-picker").ghost().small()
            .label(model_label).icon(Icon::new(IconName::Bot).xsmall())
            .disabled(settings_disabled).dropdown_menu(move |mut menu, _, _| {
                for provider in &providers {
                    if !provider.enabled || !provider.installed || provider.availability.as_deref() == Some("unavailable") { continue; }
                    menu = menu.label(provider.display_name.clone().unwrap_or_else(|| ui::provider_label(Some(&provider.driver))));
                    for model in &provider.models {
                        let selected = selection.as_ref().is_some_and(|s| s["instanceId"] == provider.instance_id && s["model"] == model.id);
                        let action = t3_client::ThreadAction::Model(serde_json::json!({ "instanceId": provider.instance_id, "model": model.id }));
                        let view = view.clone();
                        menu = menu.item(PopupMenuItem::new(model.label.clone()).checked(selected)
                            .disabled(provider.requires_new_thread_for_model_change && !selected)
                            .on_click(move |_, _, cx| { let _ = view.update(cx, |view, cx| view.update_setting(action.clone(), cx)); }));
                    }
                }
                if providers.is_empty() { menu = menu.label("Models unavailable. Reconnect to retry."); }
                menu
            });
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
                    button.bg(theme.danger).child(Icon::new(IconName::Square).xsmall()).on_click(
                        cx.listener(|this, _, _, cx| {
                            if this.connected {
                                cx.emit(ThreadViewEvent::Stop);
                            }
                        }),
                    )
                } else {
                    button
                        .bg(theme.primary)
                        .hover(|style| style.bg(theme.primary_hover))
                        .child(Icon::new(IconName::ArrowUp).small())
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
                }
            })
            .when(
                !self.connected
                    || (!working
                        && (self.sending || has_user_input || self.pending_update.is_some())),
                |button| button.opacity(0.4),
            );

        let composer = v_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(div().px_2().pt_2().child(Textarea::new(&self.composer).appearance(false)))
            .child(
                h_flex()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .child(model_picker)
                    .child(separator(cx))
                    .child(self.mode_picker(
                        "runtime-picker",
                        runtime_icon,
                        runtime_label,
                        shell.map_or("", |t| t.runtime_mode.as_str()),
                        true,
                        settings_disabled,
                        cx,
                    ))
                    .child(separator(cx))
                    .child(self.mode_picker(
                        "interaction-picker",
                        mode_icon,
                        mode_label,
                        shell.map_or("", |t| t.interaction_mode.as_str()),
                        false,
                        settings_disabled,
                        cx,
                    ))
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
            .child(if shell.and_then(|t| t.worktree_path.as_ref()).is_some() {
                "Worktree"
            } else {
                "Local checkout"
            })
            .child(div().flex_1())
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
                        .max_w(CONTENT_WIDTH)
                        .px_1()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error)
                }))
                .children(approvals.iter().take(1).enumerate().map(|(ix, approval)| {
                    v_flex()
                        .w_full()
                        .max_w(CONTENT_WIDTH)
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
                            .cached(StyleRefinement::default().w_full().max_w(CONTENT_WIDTH)),
                    )
                })
                .child(composer)
                .child(footer),
        )
    }
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
