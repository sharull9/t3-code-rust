//! Client-side projections of the shell and thread streams.
//!
//! Mirrors the parts of `packages/client-runtime/src/state/threadReducer.ts`
//! this client renders. Events at or below the current sequence are replays
//! and are skipped.

use crate::types::*;

#[derive(Debug, Default, Clone)]
pub struct ShellState {
    pub sequence: u64,
    pub synchronized: bool,
    pub projects: Vec<ProjectShell>,
    pub threads: Vec<ThreadShell>,
}

impl ShellState {
    pub fn apply(&mut self, item: ShellStreamItem) {
        match item {
            ShellStreamItem::Synchronized => self.synchronized = true,
            ShellStreamItem::Snapshot { snapshot, resolved_repository_identity_roots } => {
                if resolved_repository_identity_roots.is_some() {
                    // These refresh only repository identity, which this client
                    // does not yet render. They must never change structure or
                    // sequence, even when delivered after a project mutation.
                    return;
                } else {
                    self.sequence = snapshot.snapshot_sequence;
                    self.projects = snapshot.projects;
                    self.threads = snapshot.threads;
                }
            }
            ShellStreamItem::ProjectUpserted { sequence, project } => {
                if self.advance(sequence) {
                    upsert(&mut self.projects, project, |p| &p.id);
                }
            }
            ShellStreamItem::ProjectRemoved { sequence, project_id } => {
                if self.advance(sequence) {
                    self.projects.retain(|p| p.id != project_id);
                }
            }
            ShellStreamItem::ThreadUpserted { sequence, thread } => {
                if self.advance(sequence) {
                    upsert(&mut self.threads, thread, |t| &t.id);
                }
            }
            ShellStreamItem::ThreadRemoved { sequence, thread_id } => {
                if self.advance(sequence) {
                    self.threads.retain(|t| t.id != thread_id);
                }
            }
        }
    }

    pub fn thread(&self, thread_id: &str) -> Option<&ThreadShell> {
        self.threads.iter().find(|t| t.id == thread_id)
    }

    /// Unarchived threads of a project: pinned first, then most recently updated.
    pub fn project_threads(&self, project_id: &str) -> Vec<&ThreadShell> {
        let mut threads: Vec<_> = self
            .threads
            .iter()
            .filter(|t| t.project_id == project_id && t.archived_at.is_none())
            .collect();
        threads.sort_by(|a, b| {
            b.pinned_at
                .is_some()
                .cmp(&a.pinned_at.is_some())
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        threads
    }

    fn advance(&mut self, sequence: u64) -> bool {
        if sequence <= self.sequence {
            return false;
        }
        self.sequence = sequence;
        true
    }
}

/// Port of `sortSettledThreads` in
/// `packages/client-runtime/src/state/threadSort.ts`: settled rows are
/// history, so they order by when the work ended, newest first, with an id
/// tiebreak so the order is stable across re-sorts.
pub fn sort_settled_threads(threads: &mut [ThreadShell]) {
    threads.sort_by(|a, b| {
        b.settled_timestamp().cmp(&a.settled_timestamp()).then_with(|| a.id.cmp(&b.id))
    });
}

#[derive(Debug, Default, Clone)]
pub struct ThreadState {
    pub sequence: u64,
    pub synchronized: bool,
    pub thread: Option<ThreadDetail>,
}

impl ThreadState {
    pub fn apply(&mut self, item: ThreadStreamItem) {
        match item {
            ThreadStreamItem::Synchronized => self.synchronized = true,
            ThreadStreamItem::Snapshot { snapshot } => {
                self.sequence = snapshot.snapshot_sequence;
                self.thread = Some(snapshot.thread);
            }
            ThreadStreamItem::Event { event } => {
                if event.sequence <= self.sequence {
                    return;
                }
                self.sequence = event.sequence;
                if let Some(thread) = self.thread.as_mut() {
                    apply_event(thread, event);
                }
            }
        }
    }
}

fn apply_event(thread: &mut ThreadDetail, event: OrchestrationEvent) {
    match event.event_type.as_str() {
        "message.updated" => {
            // V2 updates carry the complete message, including while streaming.
            if let Ok(message) = serde_json::from_value::<Message>(event.payload) {
                upsert(&mut thread.messages, message, |message| &message.id);
            }
        }
        "turn-item.updated" => match crate::turn_items::convert(&event.payload) {
            // Each update carries the whole item; replace the earlier copy.
            Some(crate::turn_items::Converted::Message(message)) => {
                if let Ok(message) = serde_json::from_value::<Message>(message) {
                    upsert(&mut thread.messages, message, |message| &message.id);
                }
            }
            Some(crate::turn_items::Converted::Activity(activity)) => {
                if let Ok(activity) = serde_json::from_value::<crate::Activity>(activity) {
                    upsert(&mut thread.activities, activity, |activity| &activity.id);
                }
            }
            None => {}
        },
        "thread.message-sent" => {
            let Ok(payload) = serde_json::from_value::<MessageSentPayload>(event.payload) else {
                return;
            };
            match thread.messages.iter_mut().find(|m| m.id == payload.message_id) {
                Some(existing) => {
                    // Streaming events carry deltas; the final event carries the
                    // full text, or an empty string meaning "keep what you have".
                    if payload.streaming {
                        existing.text.push_str(&payload.text);
                    } else {
                        if !payload.text.is_empty() {
                            existing.text = payload.text;
                        }
                        existing.updated_at = payload.updated_at;
                    }
                    if !payload.attachments.is_empty() {
                        existing.attachments = payload.attachments;
                    }
                    existing.streaming = payload.streaming;
                    if payload.turn_id.is_some() {
                        existing.turn_id = payload.turn_id;
                    }
                }
                None => thread.messages.push(Message {
                    id: payload.message_id,
                    attachments: payload.attachments,
                    role: payload.role,
                    text: payload.text,
                    turn_id: payload.turn_id,
                    streaming: payload.streaming,
                    created_at: payload.created_at,
                    updated_at: payload.updated_at,
                }),
            }
        }
        "thread.session-set" => {
            if let Ok(payload) = serde_json::from_value::<SessionSetPayload>(event.payload) {
                thread.session = Some(payload.session);
            }
        }
        "thread.meta-updated" | "thread.metadata-updated" => {
            if let Some(title) = event.payload.get("title").and_then(|v| v.as_str()) {
                thread.title = title.to_owned();
            }
        }
        "thread.activity-appended" => {
            let Ok(payload) = serde_json::from_value::<ActivityAppendedPayload>(event.payload)
            else {
                return;
            };
            // The server never replaces an activity in place (a tool's
            // lifecycle transitions are separate activities with their own
            // ids), but guard against a duplicate delivery anyway.
            if !thread.activities.iter().any(|a| a.id == payload.activity.id) {
                thread.activities.push(payload.activity);
            }
        }
        _ => {}
    }
}

fn upsert<T>(items: &mut Vec<T>, item: T, key: impl Fn(&T) -> &String) {
    match items.iter().position(|existing| key(existing) == key(&item)) {
        Some(index) => items[index] = item,
        None => items.push(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repository_enrichment_keeps_existing_projects_and_threads() {
        let mut state = ShellState::default();
        state.apply(serde_json::from_value(json!({
            "kind": "snapshot", "snapshot": {
                "snapshotSequence": 10,
                "projects": [
                    {"id": "p1", "title": "One", "workspaceRoot": "/one"},
                    {"id": "p2", "title": "Two", "workspaceRoot": "/two"}
                ],
                "threads": [{"id": "t1", "projectId": "p1", "title": "Existing", "runtimeMode": "full-access"}]
            }
        })).unwrap());
        state.apply(serde_json::from_value(json!({
            "kind": "snapshot", "resolvedRepositoryIdentityRoots": ["/one"],
            "snapshot": {
                "snapshotSequence": 99,
                "projects": [{"id": "p1", "title": "Updated", "workspaceRoot": "/one"}],
                "threads": []
            }
        })).unwrap());
        assert_eq!(state.projects.len(), 2);
        assert_eq!(state.projects[0].title, "One");
        assert_eq!(state.sequence, 10);
        assert_eq!(state.threads.len(), 1);
        assert_eq!(state.threads[0].id, "t1");

        // An authoritative reconnect snapshot must still remove absent entries.
        state.apply(serde_json::from_value(json!({
            "kind": "snapshot", "snapshot": {
                "snapshotSequence": 11, "projects": [], "threads": []
            }
        })).unwrap());
        assert!(state.projects.is_empty());
        assert!(state.threads.is_empty());
    }

    #[test]
    fn v2_snapshot_and_full_message_updates_synchronize_without_duplicate_text() {
        let message = json!({
            "id": "m1", "role": "assistant", "text": "Hel", "runId": "run1",
            "streaming": true, "attachments": [],
            "createdAt": "2026-10-03T00:00:00Z", "updatedAt": "2026-10-03T00:00:00Z"
        });
        let mut state = ThreadState::default();
        state.apply(
            serde_json::from_value(json!({
                "kind": "snapshot", "snapshotSequence": 10,
                "projection": {
                    "thread": { "id": "t1", "projectId": "p1", "title": "V2 thread" },
                    "messages": [message.clone()]
                }
            }))
            .unwrap(),
        );
        let mut completed = message;
        completed["text"] = json!("Hello");
        completed["streaming"] = json!(false);
        let event = json!({
            "kind": "event", "sequence": 11,
            "event": { "type": "message.updated", "threadId": "t1", "payload": completed }
        });
        state.apply(serde_json::from_value(event.clone()).unwrap());
        state.apply(serde_json::from_value(event).unwrap());
        state.apply(ThreadStreamItem::Synchronized);
        assert!(state.synchronized);
        assert_eq!(state.sequence, 11);
        let thread = state.thread.unwrap();
        assert_eq!(thread.messages.len(), 1);
        assert_eq!(thread.messages[0].text, "Hello");
        assert_eq!(thread.messages[0].turn_id.as_deref(), Some("run1"));
        assert!(!thread.messages[0].streaming);
    }

    #[test]
    fn v2_turn_items_show_as_activities_and_update_in_place() {
        let command = |status: &str, output: Option<&str>| {
            json!({
                "id": "cmd-1", "type": "command_execution", "threadId": "t1", "runId": "run1",
                "nodeId": null, "providerThreadId": null, "providerTurnId": null,
                "nativeItemRef": null, "parentItemId": null, "ordinal": 1, "status": status,
                "title": null, "startedAt": "2026-10-04T00:00:01Z", "completedAt": null,
                "updatedAt": "2026-10-04T00:00:02Z", "input": "ls", "output": output
            })
        };
        let mut state = ThreadState::default();
        state.apply(
            serde_json::from_value(json!({
                "kind": "snapshot", "snapshotSequence": 3,
                "projection": {
                    "thread": { "id": "t1", "projectId": "p1", "title": "V2 thread" },
                    "messages": [],
                    "turnItems": [command("running", None), {
                        "id": "think-1", "type": "reasoning", "threadId": "t1", "runId": "run1",
                        "status": "completed", "startedAt": "2026-10-04T00:00:00Z",
                        "updatedAt": "2026-10-04T00:00:00Z", "text": "Plan it", "streaming": false
                    }]
                }
            }))
            .unwrap(),
        );
        let thread = state.thread.as_ref().unwrap();
        assert_eq!(thread.activities.len(), 1);
        assert_eq!(thread.activities[0].summary, "ls");
        assert_eq!(thread.messages[0].role, MessageRole::Reasoning);

        state.apply(
            serde_json::from_value(json!({
                "kind": "event", "sequence": 4,
                "event": { "type": "turn-item.updated", "threadId": "t1",
                           "payload": command("completed", Some("a.txt")) }
            }))
            .unwrap(),
        );
        let thread = state.thread.as_ref().unwrap();
        assert_eq!(thread.activities.len(), 1, "the update replaces the running copy");
        assert_eq!(thread.activities[0].payload["output"], "a.txt");
    }

    #[test]
    fn v2_shell_updates_add_and_remove_threads() {
        let mut state = ShellState::default();
        state.apply(serde_json::from_value(json!({
            "kind": "thread.updated", "sequence": 1, "location": "active",
            "thread": { "id": "t1", "projectId": "p1", "title": "V2", "runtimeMode": "full-access" }
        })).unwrap());
        assert_eq!(state.threads.len(), 1);
        state.apply(
            serde_json::from_value(json!({
                "kind": "thread.removed", "sequence": 2, "location": "active", "threadId": "t1"
            }))
            .unwrap(),
        );
        assert!(state.threads.is_empty());
    }

    #[test]
    fn v2_shell_run_state_fills_session_turn_and_pending_flags() {
        let mut state = ShellState::default();
        state.apply(serde_json::from_value(json!({
            "kind": "thread.updated", "sequence": 1, "location": "active",
            "thread": {
                "id": "t1", "projectId": "p1", "title": "V2", "runtimeMode": "full-access",
                "status": "completed", "activityRunStatus": "running", "activeRunId": "run2",
                "providerInstanceId": "codex",
                "latestRunRequestedAt": "2026-10-04T00:00:00Z",
                "latestRunStartedAt": "2026-10-04T00:00:01Z",
                "activityRunStartedAt": "2026-10-03T23:59:00Z",
                "latestRunCompletedAt": null,
                "pendingRuntimeRequest": { "id": "r1", "kind": "user_input", "createdAt": "2026-10-04T00:00:02Z" }
            }
        })).unwrap());
        let thread = &state.threads[0];
        let session = thread.session.as_ref().unwrap();
        assert!(session.is_working());
        assert_eq!(session.active_turn_id.as_deref(), Some("run2"));
        assert_eq!(session.provider_name.as_deref(), Some("codex"));
        let turn = thread.latest_turn.as_ref().unwrap();
        assert_eq!(turn.started_at.as_deref(), Some("2026-10-03T23:59:00Z"));
        assert!(thread.has_pending_user_input && !thread.has_pending_approvals);

        state.apply(serde_json::from_value(json!({
            "kind": "thread.updated", "sequence": 2, "location": "active",
            "thread": {
                "id": "t1", "projectId": "p1", "title": "V2", "runtimeMode": "full-access",
                "status": "failed", "activeRunId": null, "lastError": "boom",
                "pendingRuntimeRequest": null
            }
        })).unwrap());
        let session = state.threads[0].session.as_ref().unwrap();
        assert_eq!(session.status, SessionStatus::Error);
        assert_eq!(session.last_error.as_deref(), Some("boom"));
        assert!(!state.threads[0].has_pending_user_input);
    }

    fn thread_state() -> ThreadState {
        let mut state = ThreadState::default();
        state.apply(
            serde_json::from_value(json!({
                "kind": "snapshot",
                "snapshot": {
                    "snapshotSequence": 10,
                    "thread": {
                        "id": "t1", "projectId": "p1", "title": "Hello",
                        "branch": null, "session": null, "messages": []
                    }
                }
            }))
            .unwrap(),
        );
        state
    }

    fn message_event(sequence: u64, text: &str, streaming: bool) -> ThreadStreamItem {
        serde_json::from_value(json!({
            "kind": "event",
            "event": {
                "sequence": sequence,
                "type": "thread.message-sent",
                "aggregateId": "t1",
                "payload": {
                    "threadId": "t1", "messageId": "m1", "role": "assistant",
                    "text": text, "turnId": "turn1", "streaming": streaming,
                    "createdAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn streaming_deltas_append_and_final_keeps_text() {
        let mut state = thread_state();
        state.apply(message_event(11, "Hel", true));
        state.apply(message_event(12, "lo", true));
        state.apply(message_event(13, "", false));
        let message = &state.thread.unwrap().messages[0];
        assert_eq!(message.text, "Hello");
        assert!(!message.streaming);
    }

    #[test]
    fn replayed_events_are_skipped() {
        let mut state = thread_state();
        state.apply(message_event(11, "a", true));
        state.apply(message_event(11, "a", true));
        state.apply(message_event(9, "old", true));
        assert_eq!(state.thread.unwrap().messages[0].text, "a");
    }

    fn activity_event(sequence: u64, id: &str, tone: &str, kind: &str) -> ThreadStreamItem {
        serde_json::from_value(json!({
            "kind": "event",
            "event": {
                "sequence": sequence,
                "type": "thread.activity-appended",
                "aggregateId": "t1",
                "payload": {
                    "threadId": "t1",
                    "activity": {
                        "id": id, "tone": tone, "kind": kind, "summary": "cat foo.txt",
                        "payload": { "toolCallId": "call-1" },
                        "turnId": "turn1",
                        "createdAt": "2026-01-01T00:00:00.000Z"
                    }
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn activity_appended_pushes_and_dedupes() {
        let mut state = thread_state();
        state.apply(activity_event(11, "a1", "tool", "tool.started"));
        state.apply(activity_event(12, "a1", "tool", "tool.started"));
        state.apply(activity_event(13, "a2", "tool", "tool.completed"));
        let thread = state.thread.unwrap();
        assert_eq!(thread.activities.len(), 2);
        assert_eq!(thread.activities[0].id, "a1");
        assert_eq!(thread.activities[0].tone, ActivityTone::Tool);
        assert_eq!(thread.activities[1].id, "a2");
    }

    #[test]
    fn activity_with_unknown_tone_decodes_as_unknown() {
        let mut state = thread_state();
        state.apply(activity_event(11, "a1", "task-progress", "task.progress"));
        let thread = state.thread.unwrap();
        assert_eq!(thread.activities[0].tone, ActivityTone::Unknown);
    }

    #[test]
    fn activity_missing_required_field_is_skipped() {
        let mut state = thread_state();
        // No `summary`: undecodable, must not panic and must not be added.
        state.apply(
            serde_json::from_value(json!({
                "kind": "event",
                "event": {
                    "sequence": 11,
                    "type": "thread.activity-appended",
                    "aggregateId": "t1",
                    "payload": {
                        "threadId": "t1",
                        "activity": {
                            "id": "a1", "tone": "tool", "kind": "tool.started",
                            "createdAt": "2026-01-01T00:00:00.000Z"
                        }
                    }
                }
            }))
            .unwrap(),
        );
        assert!(state.thread.unwrap().activities.is_empty());
    }

    #[test]
    fn shell_snapshot_then_upsert() {
        let mut shell = ShellState::default();
        shell.apply(
            serde_json::from_value(json!({
                "kind": "snapshot",
                "snapshot": {
                    "snapshotSequence": 3,
                    "projects": [{ "id": "p1", "title": "Repo", "workspaceRoot": "/repo" }],
                    "threads": [],
                    "updatedAt": "2026-01-01T00:00:00.000Z"
                }
            }))
            .unwrap(),
        );
        shell.apply(
            serde_json::from_value(json!({
                "kind": "thread-upserted",
                "sequence": 4,
                "thread": {
                    "id": "t1", "projectId": "p1", "title": "Fix bug",
                    "runtimeMode": "full-access", "updatedAt": "2026-01-01T00:00:00.000Z",
                    "somethingNew": { "ignored": true }
                }
            }))
            .unwrap(),
        );
        assert_eq!(shell.sequence, 4);
        assert_eq!(shell.project_threads("p1")[0].title, "Fix bug");
    }

    fn thread(id: &str, fields: serde_json::Value) -> ThreadShell {
        let mut base = json!({
            "id": id, "projectId": "p1", "title": id, "runtimeMode": "full-access",
            "updatedAt": "2026-01-01T00:00:00.000Z",
        });
        for (key, value) in fields.as_object().unwrap() {
            base[key] = value.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn settled_only_when_override_is_settled() {
        assert!(!thread("t1", json!({})).is_settled());
        assert!(!thread("t1", json!({ "settledOverride": "active" })).is_settled());
        assert!(thread("t1", json!({ "settledOverride": "settled" })).is_settled());
    }

    #[test]
    fn settled_timestamp_prefers_settled_at_then_falls_back() {
        let stamped = thread("t1", json!({ "settledAt": "2026-02-01T00:00:00.000Z" }));
        assert_eq!(stamped.settled_timestamp(), Some("2026-02-01T00:00:00.000Z"));

        let unstamped = thread(
            "t1",
            json!({
                "latestUserMessageAt": "2026-01-05T00:00:00.000Z",
                "latestTurn": { "completedAt": "2026-01-06T00:00:00.000Z" },
            }),
        );
        assert_eq!(unstamped.settled_timestamp(), Some("2026-01-06T00:00:00.000Z"));

        let bare = thread("t1", json!({}));
        assert_eq!(bare.settled_timestamp(), Some("2026-01-01T00:00:00.000Z"));
    }

    #[test]
    fn sort_settled_threads_orders_newest_first_with_id_tiebreak() {
        let mut threads = vec![
            thread("b", json!({ "settledAt": "2026-01-01T00:00:00.000Z" })),
            thread("a", json!({ "settledAt": "2026-01-02T00:00:00.000Z" })),
            thread("c", json!({ "settledAt": "2026-01-02T00:00:00.000Z" })),
        ];
        sort_settled_threads(&mut threads);
        assert_eq!(threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);
    }
}
