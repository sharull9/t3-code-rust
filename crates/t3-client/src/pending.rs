//! Approval state derived from the same activities as T3's client-runtime.
use crate::Activity;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ApprovalOption {
    pub decision: String,
    pub label: String,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub request_id: String,
    pub created_at: String,
    pub summary: String,
    pub detail: Option<String>,
    pub options: Vec<ApprovalOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub value: Option<String>,
}

impl QuestionOption {
    pub fn answer_value(&self) -> &str {
        self.value.as_deref().unwrap_or(&self.label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub allow_custom_answer: Option<bool>,
    #[serde(default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUserInput {
    pub request_id: String,
    pub created_at: String,
    pub questions: Vec<UserInputQuestion>,
    pub dismissible: bool,
    /// The latest retryable provider failure, identified by its activity ID.
    pub failure: Option<(String, String)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerDraft {
    pub selected: Vec<String>,
    pub custom: String,
}

impl UserInputQuestion {
    pub fn resolve_answer(&self, draft: &AnswerDraft) -> Option<serde_json::Value> {
        if self.allow_custom_answer != Some(false) && !draft.custom.trim().is_empty() {
            return Some(serde_json::Value::String(draft.custom.trim().into()));
        }
        let mut selected = Vec::new();
        for value in &draft.selected {
            if self.options.iter().any(|option| option.answer_value() == value)
                && !selected.contains(value)
            {
                selected.push(value.clone());
            }
        }
        if self.multi_select {
            (!selected.is_empty()).then(|| serde_json::json!(selected))
        } else {
            selected.first().map(|value| serde_json::json!(value))
        }
    }
}

pub fn build_answers(
    request: &PendingUserInput,
    drafts: &HashMap<String, AnswerDraft>,
) -> Option<serde_json::Value> {
    let mut answers = serde_json::Map::new();
    for question in &request.questions {
        answers.insert(
            question.id.clone(),
            question.resolve_answer(drafts.get(&question.id).unwrap_or(&AnswerDraft::default()))?,
        );
    }
    Some(serde_json::Value::Object(answers))
}

pub fn user_inputs(activities: &[Activity]) -> Vec<PendingUserInput> {
    let mut pending = HashMap::new();
    let mut closed = HashSet::new();
    let mut failures = HashMap::new();
    for activity in activities {
        let payload = &activity.payload;
        let Some(id) =
            payload.get("requestId").and_then(|v| v.as_str()).filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        let failure = activity.kind == "provider.user-input.respond.failed";
        let detail = payload.get("detail").and_then(|v| v.as_str()).unwrap_or("");
        let stale = failure
            && [
                "stale pending user-input request",
                "unknown pending user-input request",
                "unknown pending user input request",
                "unknown pending codex user input request",
            ]
            .iter()
            .any(|fragment| detail.to_lowercase().contains(fragment));
        if activity.kind == "user-input.resolved" || stale {
            closed.insert(id.to_owned());
            pending.remove(id);
        } else if failure {
            failures.insert(id.to_owned(), (activity.id.clone(), detail.to_owned()));
        } else if activity.kind == "user-input.requested" && !closed.contains(id) {
            let questions: Vec<_> = payload
                .get("questions")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    let mut value = value.clone();
                    let options = value.get_mut("options")?.as_array_mut()?;
                    options.retain(|option| {
                        serde_json::from_value::<QuestionOption>(option.clone()).is_ok()
                    });
                    value["multiSelect"] = serde_json::json!(
                        value.get("multiSelect").and_then(|v| v.as_bool()) == Some(true)
                    );
                    if !value.get("allowCustomAnswer").is_some_and(|v| v.is_boolean()) {
                        value.as_object_mut()?.remove("allowCustomAnswer");
                    }
                    serde_json::from_value::<UserInputQuestion>(value).ok()
                })
                .filter(|question| {
                    !question.options.is_empty() || question.allow_custom_answer != Some(false)
                })
                .collect();
            if !questions.is_empty() {
                pending.insert(
                    id.to_owned(),
                    PendingUserInput {
                        request_id: id.into(),
                        created_at: activity.created_at.clone(),
                        questions,
                        dismissible: payload.get("responseMode").and_then(|v| v.as_str())
                            == Some("message"),
                        failure: None,
                    },
                );
            }
        }
    }
    let mut requests: Vec<_> = pending.into_values().collect();
    for request in &mut requests {
        request.failure = failures.remove(&request.request_id);
    }
    requests.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.request_id.cmp(&b.request_id)));
    requests
}

pub fn approvals(activities: &[Activity]) -> Vec<PendingApproval> {
    let mut pending = HashMap::new();
    let mut closed = HashSet::new();
    for activity in activities {
        let payload = &activity.payload;
        let Some(id) =
            payload.get("requestId").and_then(|v| v.as_str()).filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        let stale = activity.kind == "provider.approval.respond.failed"
            && payload.get("detail").and_then(|v| v.as_str()).is_some_and(|detail| {
                let detail = detail.to_lowercase();
                [
                    "stale pending approval request",
                    "unknown pending approval request",
                    "unknown pending permission request",
                    "unknown pending codex approval request",
                ]
                .iter()
                .any(|fragment| detail.contains(fragment))
            });
        if activity.kind == "approval.resolved" || stale {
            closed.insert(id.to_owned());
            pending.remove(id);
        } else if activity.kind == "approval.requested" && !closed.contains(id) {
            if matches!(
                payload.get("requestType").and_then(|v| v.as_str()),
                Some("tool_user_input" | "auth_tokens_refresh")
            ) {
                continue;
            }
            let mut options: Vec<ApprovalOption> = payload
                .get("options")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|value| serde_json::from_value::<ApprovalOption>(value.clone()).ok())
                .filter(|option| {
                    matches!(
                        option.decision.as_str(),
                        "accept" | "acceptForSession" | "acceptAlways" | "decline" | "cancel"
                    )
                })
                .collect();
            if options.is_empty() {
                options = [
                    ("accept", "Allow once"),
                    ("acceptForSession", "Allow for session"),
                    ("decline", "Reject"),
                ]
                .into_iter()
                .map(|(decision, label)| ApprovalOption {
                    decision: decision.into(),
                    label: label.into(),
                    warning: None,
                })
                .collect();
            }
            pending.insert(
                id.to_owned(),
                PendingApproval {
                    request_id: id.into(),
                    created_at: activity.created_at.clone(),
                    summary: activity.summary.clone(),
                    detail: payload.get("detail").and_then(|v| v.as_str()).map(str::to_owned),
                    options,
                },
            );
        }
    }
    let mut pending: Vec<_> = pending.into_values().collect();
    pending.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.request_id.cmp(&b.request_id)));
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn activity(kind: &str, payload: serde_json::Value) -> Activity {
        serde_json::from_value(json!({ "id": kind, "kind": kind, "tone": "approval", "summary": "Command approval", "createdAt": "2026-10-01T00:00:00Z", "payload": payload })).unwrap()
    }
    #[test]
    fn resolution_stays_final_even_when_request_arrives_late() {
        let events = [
            activity("approval.resolved", json!({ "requestId": "r1" })),
            activity("approval.requested", json!({ "requestId": "r1" })),
        ];
        assert!(approvals(&events).is_empty());
    }
    #[test]
    fn retryable_failure_keeps_request_and_stale_failure_closes_it() {
        let mut events = vec![activity(
            "approval.requested",
            json!({ "requestId": "r1", "options": [{ "decision": "accept", "label": "Approve", "warning": "Review this command" }] }),
        )];
        events.push(activity(
            "provider.approval.respond.failed",
            json!({ "requestId": "r1", "detail": "Provider unavailable" }),
        ));
        assert_eq!(
            approvals(&events)[0].options[0].warning.as_deref(),
            Some("Review this command")
        );
        events.push(activity(
            "provider.approval.respond.failed",
            json!({ "requestId": "r1", "detail": "Unknown pending approval request" }),
        ));
        assert!(approvals(&events).is_empty());
    }

    fn question_request(mode: Option<&str>) -> Activity {
        activity(
            "user-input.requested",
            json!({ "requestId": "input-1", "responseMode": mode, "questions": [
            { "id": " exact id ", "header": "Choose", "question": "Which?", "multiSelect": true, "allowCustomAnswer": false,
              "options": [{ "label": "First", "value": " raw value ", "description": "One" }, { "label": "Second", "description": "Two" }] },
            { "id": "text", "header": "Details", "question": "Explain", "options": [] }
        ] }),
        )
    }

    #[test]
    fn questions_keep_native_ids_and_multi_choice_values_and_require_all_answers() {
        let request = user_inputs(&[question_request(None)]).remove(0);
        assert!(!request.dismissible);
        let mut drafts = HashMap::from([(
            " exact id ".into(),
            AnswerDraft {
                selected: vec![
                    " raw value ".into(),
                    "Second".into(),
                    " raw value ".into(),
                    "invalid".into(),
                ],
                custom: "Ignored because custom is forbidden".into(),
            },
        )]);
        assert!(build_answers(&request, &drafts).is_none());
        drafts.insert(
            "text".into(),
            AnswerDraft { custom: "  Written reply\nwith details  ".into(), ..Default::default() },
        );
        assert_eq!(
            build_answers(&request, &drafts),
            Some(
                json!({ " exact id ": [" raw value ", "Second"], "text": "Written reply\nwith details" })
            )
        );
    }

    #[test]
    fn async_dismissal_and_retryable_failures_are_derived_from_server_activities() {
        let mut events = vec![question_request(Some("message"))];
        assert!(user_inputs(&events)[0].dismissible);
        let mut failure = activity(
            "provider.user-input.respond.failed",
            json!({ "requestId": "input-1", "detail": "Provider temporarily unavailable" }),
        );
        failure.id = "failure-1".into();
        events.push(failure);
        assert_eq!(user_inputs(&events)[0].failure.as_ref().unwrap().0, "failure-1");
        events.push(activity(
            "provider.user-input.respond.failed",
            json!({ "requestId": "input-1", "detail": "Unknown pending user input request" }),
        ));
        assert!(user_inputs(&events).is_empty());
    }

    #[test]
    fn resolved_questions_do_not_reappear_when_request_is_replayed_late() {
        assert!(
            user_inputs(&[
                activity("user-input.resolved", json!({ "requestId": "input-1" })),
                question_request(None)
            ])
            .is_empty()
        );
    }

    #[test]
    fn malformed_options_do_not_drop_other_valid_questions() {
        let request = activity(
            "user-input.requested",
            json!({ "requestId": "r", "questions": [
            { "id": "q", "header": "Choice", "question": "Choose", "options": [{ "label": 1 }, { "label": "Valid", "description": "" }], "allowCustomAnswer": false },
            { "id": "impossible", "header": "Invalid", "question": "No answers", "options": [], "allowCustomAnswer": false }
        ] }),
        );
        let requests = user_inputs(&[request]);
        assert_eq!(requests[0].questions.len(), 1);
        assert_eq!(requests[0].questions[0].options[0].answer_value(), "Valid");
    }
}
