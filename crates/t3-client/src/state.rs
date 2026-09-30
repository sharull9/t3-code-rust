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
            ShellStreamItem::Snapshot { snapshot } => {
                self.sequence = snapshot.snapshot_sequence;
                self.projects = snapshot.projects;
                self.threads = snapshot.threads;
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
                    existing.streaming = payload.streaming;
                    if payload.turn_id.is_some() {
                        existing.turn_id = payload.turn_id;
                    }
                }
                None => thread.messages.push(Message {
                    id: payload.message_id,
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
        "thread.meta-updated" => {
            if let Some(title) = event.payload.get("title").and_then(|v| v.as_str()) {
                thread.title = title.to_owned();
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
}
