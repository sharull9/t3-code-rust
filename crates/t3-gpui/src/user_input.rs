//! Native question form. Answers are retained until the server resolves a request.
use std::collections::HashMap;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::ThreadAction;
use t3_client::pending::{AnswerDraft, PendingUserInput, build_answers};

use crate::ui::CONTENT_WIDTH;

pub enum UserInputEvent {
    Respond(ThreadAction),
    /// Choosing an option carries displaced written text into the composer.
    DisplacedText(String),
}

pub struct UserInputPanel {
    requests: Vec<PendingUserInput>,
    drafts: HashMap<String, HashMap<String, AnswerDraft>>,
    inputs: HashMap<(String, String), Entity<InputState>>,
    question_index: usize,
    connected: bool,
    /// The failure revision when this attempt began. A new failure unlocks retry.
    responding: Option<(String, Option<String>)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<UserInputEvent> for UserInputPanel {}

impl UserInputPanel {
    pub fn has_requests(&self) -> bool {
        !self.requests.is_empty()
    }

    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            requests: Vec::new(),
            drafts: HashMap::new(),
            inputs: HashMap::new(),
            question_index: 0,
            connected: false,
            responding: None,
            _subscriptions: Vec::new(),
        }
    }

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.sync_inputs(cx);
        self.connected = false;
        self.responding = None;
        // Keep drafts through reconnect. The new snapshot decides which are stale.
        self.requests.clear();
        self.inputs.clear();
        self._subscriptions.clear();
        cx.notify();
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            cx.notify();
        }
    }

    pub fn set_requests(
        &mut self,
        requests: Vec<PendingUserInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_inputs(cx);
        if self.requests == requests {
            return;
        }
        if self.requests.first().map(|r| &r.request_id) != requests.first().map(|r| &r.request_id) {
            self.question_index = 0;
        }
        if let Some((id, revision)) = &self.responding {
            if !requests.iter().any(|r| {
                &r.request_id == id && r.failure.as_ref().map(|f| f.0.clone()) == *revision
            }) {
                self.responding = None;
            }
        }
        self.drafts.retain(|id, _| requests.iter().any(|request| &request.request_id == id));
        self.inputs.clear();
        self._subscriptions.clear();
        self.requests = requests;
        for request in &self.requests {
            for question in &request.questions {
                if question.allow_custom_answer == Some(false) {
                    continue;
                }
                let request_id = request.request_id.clone();
                let question_id = question.id.clone();
                let text = self
                    .drafts
                    .get(&request_id)
                    .and_then(|d| d.get(&question_id))
                    .map(|d| d.custom.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    let mut state = InputState::new(window, cx).placeholder("Write your answer");
                    state.set_value(text, window, cx);
                    state
                });
                self._subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            let draft = this
                                .drafts
                                .entry(request_id.clone())
                                .or_default()
                                .entry(question_id.clone())
                                .or_default();
                            draft.custom = input.read(cx).value().to_string();
                            if !draft.custom.trim().is_empty() {
                                draft.selected.clear();
                            }
                            cx.notify();
                        } else if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                            this.advance(cx);
                        }
                    },
                ));
                self.inputs.insert((request.request_id.clone(), question.id.clone()), input);
            }
        }
        if let Some(request) = self.requests.first() {
            self.question_index =
                self.question_index.min(request.questions.len().saturating_sub(1));
        }
        cx.notify();
    }

    pub fn response_finished(
        &mut self,
        action: &ThreadAction,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        let id = match action {
            ThreadAction::UserInput { request_id, .. }
            | ThreadAction::DismissUserInput { request_id } => request_id,
            _ => return,
        };
        if !success && self.responding.as_ref().is_some_and(|(pending, _)| pending == id) {
            self.responding = None;
            cx.notify();
        }
    }

    fn choose(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.connected || self.responding.is_some() {
            return;
        }
        self.sync_inputs(cx);
        let Some(request) = self.requests.first() else { return };
        let question = &request.questions[self.question_index];
        let key = (request.request_id.clone(), question.id.clone());
        let draft = self.drafts.entry(key.0.clone()).or_default().entry(key.1.clone()).or_default();
        let displaced = draft.custom.trim().to_owned();
        draft.custom.clear();
        if question.multi_select {
            if draft.selected.contains(&value) {
                draft.selected.retain(|selected| selected != &value);
            } else {
                draft.selected.push(value);
            }
        } else {
            draft.selected = vec![value];
        }
        if let Some(input) = self.inputs.get(&key) {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        if !displaced.is_empty() {
            cx.emit(UserInputEvent::DisplacedText(displaced));
        }
        cx.notify();
    }

    fn respond(&mut self, dismiss: bool, cx: &mut Context<Self>) {
        if !self.connected || self.responding.is_some() {
            return;
        }
        self.sync_inputs(cx);
        let Some(request) = self.requests.first() else { return };
        let action = if dismiss {
            if !request.dismissible {
                return;
            }
            ThreadAction::DismissUserInput { request_id: request.request_id.clone() }
        } else {
            let drafts = self.drafts.get(&request.request_id).cloned().unwrap_or_default();
            let Some(answers) = build_answers(request, &drafts) else { return };
            ThreadAction::UserInput { request_id: request.request_id.clone(), answers }
        };
        self.responding =
            Some((request.request_id.clone(), request.failure.as_ref().map(|f| f.0.clone())));
        cx.emit(UserInputEvent::Respond(action));
        cx.notify();
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        if !self.connected || self.responding.is_some() {
            return;
        }
        self.sync_inputs(cx);
        let Some(request) = self.requests.first() else { return };
        let question = &request.questions[self.question_index];
        let draft = self
            .drafts
            .get(&request.request_id)
            .and_then(|drafts| drafts.get(&question.id))
            .cloned()
            .unwrap_or_default();
        if question.resolve_answer(&draft).is_none() {
            return;
        }
        if self.question_index + 1 == request.questions.len() {
            self.respond(false, cx);
        } else {
            self.question_index += 1;
            cx.notify();
        }
    }

    /// Read the control values before replacing inputs or submitting. GPUI
    /// delivers Change subscriptions after the current update, so a snapshot
    /// arriving in that same update must not discard newly typed text.
    fn sync_inputs(&mut self, cx: &App) {
        for ((request_id, question_id), input) in &self.inputs {
            let value = input.read(cx).value().to_string();
            let draft = self
                .drafts
                .entry(request_id.clone())
                .or_default()
                .entry(question_id.clone())
                .or_default();
            if draft.custom != value {
                draft.custom = value;
                if !draft.custom.trim().is_empty() {
                    draft.selected.clear();
                }
            }
        }
    }
}

impl Render for UserInputPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_inputs(cx);
        let Some(request) = self.requests.first() else { return div().into_any_element() };
        let question = &request.questions[self.question_index];
        let draft = self
            .drafts
            .get(&request.request_id)
            .and_then(|drafts| drafts.get(&question.id))
            .cloned()
            .unwrap_or_default();
        let disabled = !self.connected || self.responding.is_some();
        let complete = self
            .drafts
            .get(&request.request_id)
            .and_then(|drafts| build_answers(request, drafts))
            .is_some();
        let can_advance = question.resolve_answer(&draft).is_some();
        let last = self.question_index + 1 == request.questions.len();
        let theme = cx.theme();
        v_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child(question.header.clone()))
                    .child(format!("{}/{}", self.question_index + 1, request.questions.len()))
                    .when(request.dismissible, |row| {
                        row.child(
                            Button::new("dismiss-question")
                                .ghost()
                                .small()
                                .label("Dismiss")
                                .disabled(disabled)
                                .tooltip("Dismiss without sending an answer")
                                .on_click(cx.listener(|this, _, _, cx| this.respond(true, cx))),
                        )
                    }),
            )
            .child(
                div().id("question-body").max_h(px(240.)).overflow_y_scrollbar().child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(question.question.clone()))
                        .when(question.multi_select, |body| {
                            body.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("Select one or more options."),
                            )
                        })
                        .children(question.options.iter().enumerate().map(|(ix, option)| {
                            let value = option.answer_value().to_owned();
                            let selected =
                                draft.custom.trim().is_empty() && draft.selected.contains(&value);
                            v_flex()
                                .gap_1()
                                .child(
                                    Button::new(("question-option", ix))
                                        .ghost()
                                        .small()
                                        .w_full()
                                        .label(format!(
                                            "{}{}",
                                            if selected { "✓ " } else { "" },
                                            option.label
                                        ))
                                        .disabled(disabled)
                                        .when(selected, |button| button.bg(theme.sidebar_accent))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.choose(value.clone(), window, cx)
                                        })),
                                )
                                .when(
                                    !option.description.is_empty()
                                        && option.description != option.label,
                                    |row| {
                                        row.child(
                                            div()
                                                .px_2()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(option.description.clone()),
                                        )
                                    },
                                )
                        }))
                        .when_some(
                            self.inputs.get(&(request.request_id.clone(), question.id.clone())),
                            |body, input| {
                                body.child(
                                    Input::new(input)
                                        .id("custom-answer")
                                        .aria_label(format!("Answer to {}", question.header))
                                        .small()
                                        .disabled(disabled),
                                )
                            },
                        ),
                ),
            )
            .children(
                request.failure.as_ref().map(|(_, detail)| {
                    div().text_xs().text_color(theme.danger).child(detail.clone())
                }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("previous-question")
                            .ghost()
                            .small()
                            .label("Back")
                            .disabled(disabled || self.question_index == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.question_index = this.question_index.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("next-question")
                            .primary()
                            .small()
                            .label(if self.responding.is_some() {
                                "Sending..."
                            } else if last {
                                "Send answers"
                            } else {
                                "Next"
                            })
                            .disabled(disabled || if last { !complete } else { !can_advance })
                            .on_click(cx.listener(|this, _, _, cx| this.advance(cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The GPUI test macro expands to Rust's #[test], not itself.
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn request(dismissible: bool) -> PendingUserInput {
        PendingUserInput { request_id: "request-1".into(), created_at: "2026-10-01T00:00:00Z".into(), dismissible, failure: None,
            questions: serde_json::from_value(json!([
                { "id": "choice", "header": "Pick", "question": "Which options?", "multiSelect": true, "allowCustomAnswer": false,
                  "options": [{ "label": "One", "value": " exact ", "description": "First option" }, { "label": "Two", "description": "Second option" }] },
                { "id": "details", "header": "Details", "question": "Explain your choice", "options": [] }
            ])).unwrap() }
    }

    fn panel(
        cx: &mut TestAppContext,
        request: PendingUserInput,
    ) -> (AnyWindowHandle, Entity<UserInputPanel>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(800.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let panel = cx.new(UserInputPanel::new);
                    panel.update(cx, |panel, cx| {
                        panel.set_requests(vec![request], window, cx);
                        panel.set_connected(true, cx);
                    });
                    panel
                },
            )
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn choices_and_written_answers_submit_once_and_survive_rejected_submission(
        cx: &mut TestAppContext,
    ) {
        let (handle, panel) = panel(cx, request(false));
        let actions = Rc::new(RefCell::new(Vec::new()));
        let captured = actions.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&panel, move |_, event: &UserInputEvent, _| {
                if let UserInputEvent::Respond(action) = event {
                    captured.borrow_mut().push(action.clone());
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("dismiss-question").is_none());
            window.click("next-question", cx);
            assert_eq!(panel.read(cx).question_index, 0);
            window.click(("question-option", 0usize), cx);
            window.click(("question-option", 1usize), cx);
            window.click("next-question", cx);
            window.click("custom-answer", cx);
            window.input("Written answer", cx);
            window.click("next-question", cx);
            assert!(panel.read(cx).responding.is_some());
            window.click("next-question", cx);
        })
        .unwrap();
        let action = actions.borrow()[0].clone();
        assert_eq!(actions.borrow().len(), 1);
        assert_eq!(
            action.command("thread-1")["answers"],
            json!({ "choice": [" exact ", "Two"], "details": "Written answer" })
        );
        cx.update_window(handle, |_, window, cx| {
            panel.update(cx, |panel, cx| panel.response_finished(&action, false, cx));
            window.render_frame(cx);
            assert_eq!(window.find("custom-answer").value(), Some("Written answer"));
            assert!(panel.read(cx).responding.is_none());
            window.click("next-question", cx);
        })
        .unwrap();
        assert_eq!(actions.borrow().len(), 2);
    }

    #[gpui_kit::test]
    fn reconnect_preserves_custom_text_and_only_async_questions_can_be_dismissed(
        cx: &mut TestAppContext,
    ) {
        let mut request = request(true);
        request.questions.remove(0);
        let (handle, panel) = panel(cx, request.clone());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("custom-answer", cx);
            window.input("Keep this through reconnect", cx);
            panel.update(cx, |panel, cx| panel.suspend(cx));
            window.render_frame(cx);
            assert!(window.try_find("custom-answer").is_none());
            panel.update(cx, |panel, cx| panel.set_requests(vec![request.clone()], window, cx));
            window.render_frame(cx);
            assert_eq!(window.find("custom-answer").value(), Some("Keep this through reconnect"));
            window.click("dismiss-question", cx);
            assert!(panel.read(cx).responding.is_none());
            panel.update(cx, |panel, cx| panel.set_connected(true, cx));
            window.render_frame(cx);
            window.click("dismiss-question", cx);
            assert!(panel.read(cx).responding.is_some());
            request.failure = Some(("failure-1".into(), "Retry this reply".into()));
            panel.update(cx, |panel, cx| panel.set_requests(vec![request.clone()], window, cx));
            window.render_frame(cx);
            assert!(panel.read(cx).responding.is_none());
            assert_eq!(window.find("custom-answer").value(), Some("Keep this through reconnect"));
        })
        .unwrap();
    }
}
