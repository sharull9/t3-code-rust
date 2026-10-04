//! Orchestration V2 turn items, in the V1 shapes this client renders.
//!
//! V2 thread projections carry tool calls, commands, file edits, reasoning
//! and requests as `turnItems` (`OrchestrationV2TurnItem` in
//! `orchestration.ts`), updated in place by `turn-item.updated` events. V1
//! sent them as append-only `activities` plus reasoning messages. Converting
//! here keeps the transcript, approvals and questions working on both.

use serde_json::{Value, json};

/// One turn item, as a V1 message or activity (both raw JSON for
/// [`crate::Message`] / [`crate::Activity`]). Items that are already messages
/// on their own (user and assistant text) convert to nothing.
pub enum Converted {
    Message(Value),
    Activity(Value),
}

/// Statuses where a request is still waiting on the user.
fn is_open(item: &Value) -> bool {
    matches!(item["status"].as_str(), Some("idle" | "pending" | "running" | "waiting"))
}

fn text(item: &Value, key: &str) -> Option<String> {
    item[key].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The first line of `text`, cut to a readable length.
fn headline(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    match line.char_indices().nth(120) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

pub fn convert(item: &Value) -> Option<Converted> {
    let id = item["id"].as_str()?;
    let kind = item["type"].as_str()?;
    let created_at = item["startedAt"].as_str().or(item["updatedAt"].as_str())?.to_owned();
    let updated_at = item["updatedAt"].as_str().unwrap_or(&created_at).to_owned();
    let run_id = item["runId"].clone();
    let title = text(item, "title");
    let failed = matches!(item["status"].as_str(), Some("failed"))
        || item["outputIndicatesFailure"].as_bool() == Some(true);

    if kind == "reasoning" {
        return Some(Converted::Message(json!({
            "id": id,
            "role": "reasoning",
            "text": item["text"].as_str().unwrap_or_default(),
            "turnId": run_id,
            "streaming": item["streaming"].as_bool().unwrap_or(false),
            "createdAt": created_at,
            "updatedAt": updated_at,
        })));
    }

    let (tone, activity_kind, summary, payload) = match kind {
        "user_message" | "assistant_message" => return None,
        "command_execution" => {
            let input = item["input"].as_str().unwrap_or_default();
            let summary = title.unwrap_or_else(|| headline(input));
            (
                "tool",
                "command",
                if failed { format!("{summary} (failed)") } else { summary },
                json!({
                    "command": input,
                    "output": item["output"],
                    "exitCode": item["exitCode"],
                    "status": item["status"],
                }),
            )
        }
        "file_change" => {
            let file = item["fileName"].as_str().unwrap_or("file");
            let counts = match (item["additions"].as_u64(), item["deletions"].as_u64()) {
                (Some(a), Some(d)) => format!(" +{a} −{d}"),
                (Some(a), None) => format!(" +{a}"),
                (None, Some(d)) => format!(" −{d}"),
                (None, None) => String::new(),
            };
            (
                "tool",
                "file_change",
                format!("Edited {file}{counts}"),
                json!({ "file": file, "diff": item["diffStr"], "changes": item["changes"] }),
            )
        }
        "file_search" => (
            "tool",
            "file_search",
            title.unwrap_or_else(|| match text(item, "pattern") {
                Some(pattern) => format!("Searched files for {pattern}"),
                None => "Searched files".into(),
            }),
            json!({ "pattern": item["pattern"], "results": item["results"] }),
        ),
        "web_search" => {
            let patterns: Vec<&str> = item["patterns"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            (
                "tool",
                "web_search",
                title.unwrap_or_else(|| {
                    if patterns.is_empty() {
                        "Searched the web".into()
                    } else {
                        format!("Searched the web for {}", patterns.join(", "))
                    }
                }),
                json!({ "patterns": item["patterns"], "results": item["results"] }),
            )
        }
        "dynamic_tool" => (
            "tool",
            "tool",
            title.or_else(|| text(item, "toolName")).unwrap_or_else(|| "Tool call".into()),
            json!({ "tool": item["toolName"], "input": item["input"], "output": item["output"] }),
        ),
        "subagent" => (
            "tool",
            "subagent",
            format!(
                "Subagent · {}",
                title.unwrap_or_else(|| headline(item["prompt"].as_str().unwrap_or_default()))
            ),
            json!({
                "prompt": item["prompt"],
                "progress": item["progress"],
                "result": item["result"],
                "childThreadId": item["childThreadId"],
            }),
        ),
        "todo_list" => {
            let steps = item["steps"].as_array().map_or(0, Vec::len);
            (
                "info",
                "todo_list",
                title.unwrap_or_else(|| format!("Updated the plan · {steps} steps")),
                json!({ "steps": item["steps"], "explanation": item["explanation"] }),
            )
        }
        "proposed_plan" => (
            "info",
            "proposed_plan",
            title.unwrap_or_else(|| "Proposed a plan".into()),
            json!({ "plan": item["markdown"] }),
        ),
        "approval_request" => {
            let request_kind = item["requestKind"].as_str().unwrap_or("approval");
            (
                "approval",
                if is_open(item) { "approval.requested" } else { "approval.resolved" },
                title.unwrap_or_else(|| format!("{} approval", headline(&request_kind.replace(['_', '-'], " ")))),
                json!({
                    "requestId": item["requestId"],
                    "requestType": request_kind,
                    "detail": item["prompt"],
                    "options": item["options"],
                }),
            )
        }
        "user_input_request" => (
            "approval",
            if is_open(item) { "user-input.requested" } else { "user-input.resolved" },
            title.unwrap_or_else(|| "Question".into()),
            json!({
                "requestId": item["requestId"],
                "questions": item["questions"],
                "responseMode": item["responseMode"],
            }),
        ),
        "error" => (
            "error",
            "error",
            item["failure"]["message"].as_str().map(headline).unwrap_or_else(|| "Error".into()),
            json!({ "failure": item["failure"], "retry": item["retry"] }),
        ),
        "compaction" => (
            "info",
            "compaction",
            title.unwrap_or_else(|| "Compacted the conversation".into()),
            json!({
                "summary": item["summary"],
                "beforeTokenCount": item["beforeTokenCount"],
                "afterTokenCount": item["afterTokenCount"],
            }),
        ),
        "checkpoint" => {
            let files = item["files"].as_array().map_or(0, Vec::len);
            (
                "info",
                "checkpoint",
                title.unwrap_or_else(|| {
                    format!("Checkpoint · {files} file{}", if files == 1 { "" } else { "s" })
                }),
                json!({ "files": item["files"] }),
            )
        }
        "system_notice" | "run_interrupt_request" | "run_interrupt_result" => (
            "info",
            kind,
            title.or_else(|| text(item, "message").map(|m| headline(&m))).unwrap_or_else(|| kind.replace('_', " ")),
            json!({ "message": item["message"] }),
        ),
        // Notifications, handoffs, forks and anything newer: a status line.
        other => (
            "info",
            other,
            title.unwrap_or_else(|| other.replace('_', " ")),
            item.clone(),
        ),
    };
    Some(Converted::Activity(json!({
        "id": id,
        "tone": if failed && tone == "tool" { "error" } else { tone },
        "kind": activity_kind,
        "summary": summary,
        "turnId": run_id,
        "createdAt": created_at,
        "payload": payload,
    })))
}

/// Splits a projection's `turnItems` into extra messages and activities.
pub fn convert_all(items: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let mut messages = Vec::new();
    let mut activities = Vec::new();
    for item in items {
        match convert(item) {
            Some(Converted::Message(message)) => messages.push(message),
            Some(Converted::Activity(activity)) => activities.push(activity),
            None => {}
        }
    }
    (messages, activities)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(fields: Value) -> Value {
        let mut base = json!({
            "id": "item-1", "threadId": "t1", "runId": "run-1", "nodeId": null,
            "providerThreadId": null, "providerTurnId": null, "nativeItemRef": null,
            "parentItemId": null, "ordinal": 0, "status": "completed", "title": null,
            "startedAt": "2026-10-04T10:00:00Z", "completedAt": null,
            "updatedAt": "2026-10-04T10:00:05Z"
        });
        base.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        base
    }

    fn activity(value: Value) -> crate::Activity {
        match convert(&value) {
            Some(Converted::Activity(activity)) => serde_json::from_value(activity).unwrap(),
            _ => panic!("expected an activity"),
        }
    }

    #[test]
    fn commands_edits_and_tools_become_tool_activities() {
        let command = activity(item(json!({
            "type": "command_execution", "input": "cargo test\n--all", "output": "ok", "exitCode": 0
        })));
        assert_eq!(command.tone, crate::ActivityTone::Tool);
        assert_eq!(command.summary, "cargo test");
        assert_eq!(command.payload["output"], "ok");
        assert_eq!(command.created_at, "2026-10-04T10:00:00Z");
        assert_eq!(command.turn_id.as_deref(), Some("run-1"));

        let failed = activity(item(json!({
            "type": "command_execution", "input": "false", "outputIndicatesFailure": true
        })));
        assert_eq!(failed.tone, crate::ActivityTone::Error);
        assert_eq!(failed.summary, "false (failed)");

        let edit = activity(item(json!({
            "type": "file_change", "fileName": "src/lib.rs", "additions": 3, "deletions": 1
        })));
        assert_eq!(edit.summary, "Edited src/lib.rs +3 −1");

        let tool = activity(item(json!({ "type": "dynamic_tool", "toolName": "Read", "input": {} })));
        assert_eq!(tool.summary, "Read");
        assert!(convert(&item(json!({ "type": "assistant_message", "messageId": "m", "text": "hi", "streaming": false }))).is_none());
    }

    #[test]
    fn reasoning_becomes_a_thought_and_requests_open_then_resolve() {
        let Some(Converted::Message(message)) = convert(&item(json!({
            "type": "reasoning", "text": "Thinking…", "streaming": true
        }))) else {
            panic!("expected a message");
        };
        let message: crate::Message = serde_json::from_value(message).unwrap();
        assert_eq!(message.role, crate::MessageRole::Reasoning);
        assert!(message.streaming);

        let open = item(json!({
            "type": "approval_request", "status": "waiting", "requestId": "r1",
            "requestKind": "command", "prompt": "rm -rf build"
        }));
        let mut closed = open.clone();
        closed["status"] = json!("completed");
        let approvals = crate::pending::approvals(&[activity(open.clone())]);
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].detail.as_deref(), Some("rm -rf build"));
        assert!(crate::pending::approvals(&[activity(closed)]).is_empty());
    }
}
