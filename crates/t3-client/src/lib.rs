//! Rust client for a T3 Code server (`npx t3`).
//!
//! ```no_run
//! # async fn demo() -> Result<(), t3_client::Error> {
//! let http = reqwest::Client::new();
//! let link = t3_client::PairingLink::parse("http://localhost:3773/pair#token=...")?;
//! let credentials = t3_client::auth::pair(&http, &link, "T3 GPUI").await?;
//! let connection = t3_client::Connection::connect(&http, &credentials).await?;
//! let mut shell = connection.subscribe_shell()?;
//! while let Some(item) = shell.next().await {
//!     println!("{:?}", item?);
//! }
//! # Ok(()) }
//! ```

pub mod attachments;
pub mod auth;
mod error;
pub mod pending;
pub mod quotas;
pub mod rpc;
pub mod state;
pub mod types;
pub mod usage;
pub mod workspace;

use std::sync::Arc;

use serde_json::{Value, json};

pub use reqwest;

pub use auth::{Credentials, PairingLink};
pub use error::{Error, RpcError};
pub use rpc::{RpcSession, Subscription};
pub use state::{ShellState, ThreadState, sort_settled_threads};
pub use types::*;
pub use usage::{UsageReport, UsageSummary, UsageWindow};
pub use workspace::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadAction {
    Pin(bool),
    Settle(bool),
    Archive,
    Unarchive,
    Rename(String),
    RuntimeMode(String),
    InteractionMode(String),
    Model(Value),
    Approval { request_id: String, decision: String },
    UserInput { request_id: String, answers: Value },
    DismissUserInput { request_id: String },
}

impl ThreadAction {
    pub fn command(&self, thread_id: &str) -> Value {
        let kind = match self {
            Self::Pin(true) => "thread.pin",
            Self::Pin(false) => "thread.unpin",
            Self::Settle(true) => "thread.settle",
            Self::Settle(false) => "thread.unsettle",
            Self::Archive => "thread.archive",
            Self::Unarchive => "thread.unarchive",
            Self::Rename(_) => "thread.meta.update",
            Self::RuntimeMode(_) => "thread.runtime-mode.set",
            Self::InteractionMode(_) => "thread.interaction-mode.set",
            Self::Model(_) => "thread.meta.update",
            Self::Approval { .. } => "thread.approval.respond",
            Self::UserInput { .. } => "thread.user-input.respond",
            Self::DismissUserInput { .. } => "thread.user-input.dismiss",
        };
        let mut command = json!({ "type": kind, "commandId": new_id(), "threadId": thread_id });
        match self {
            Self::Settle(false) => command["reason"] = json!("user"),
            Self::RuntimeMode(mode) => {
                command["runtimeMode"] = json!(mode);
                command["createdAt"] = json!(now_iso());
            }
            Self::InteractionMode(mode) => {
                command["interactionMode"] = json!(mode);
                command["createdAt"] = json!(now_iso());
            }
            Self::Model(model) => command["modelSelection"] = model.clone(),
            Self::Rename(title) => command["title"] = json!(title.trim()),
            Self::Approval { request_id, decision } => {
                command["requestId"] = json!(request_id);
                command["decision"] = json!(decision);
                command["createdAt"] = json!(now_iso());
            }
            Self::UserInput { request_id, answers } => {
                command["requestId"] = json!(request_id);
                command["answers"] = answers.clone();
                command["createdAt"] = json!(now_iso());
            }
            Self::DismissUserInput { request_id } => {
                command["requestId"] = json!(request_id);
                command["createdAt"] = json!(now_iso());
            }
            _ => {}
        }
        command
    }
}

/// An authenticated RPC session with one environment.
#[derive(Clone)]
pub struct Connection {
    rpc: Arc<RpcSession>,
}

impl Connection {
    pub async fn connect(http: &reqwest::Client, credentials: &Credentials) -> Result<Self, Error> {
        let url = auth::websocket_url(http, credentials).await?;
        let rpc = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            RpcSession::connect(url.as_str()),
        )
        .await
        .map_err(|_| RpcError::Timeout)??;
        Ok(Self { rpc })
    }

    pub fn rpc(&self) -> &RpcSession {
        &self.rpc
    }

    pub async fn server_config(&self) -> Result<ServerConfig, RpcError> {
        let value = self.rpc.call("server.getConfig", json!({})).await?;
        serde_json::from_value(value).map_err(|error| RpcError::Decode(error.to_string()))
    }

    /// Archived summaries are fetched separately from the live shell stream.
    pub async fn archived_shell(&self) -> Result<ShellSnapshot, RpcError> {
        let value = self.rpc.call("orchestration.getArchivedShellSnapshot", json!({})).await?;
        serde_json::from_value(value).map_err(|error| RpcError::Decode(error.to_string()))
    }

    pub async fn closed(&self) -> String {
        self.rpc.closed().await
    }

    /// Projects and thread summaries: a snapshot, then live upserts/removals.
    pub fn subscribe_shell(&self) -> Result<Subscription<ShellStreamItem>, RpcError> {
        self.rpc.subscribe(methods::SUBSCRIBE_SHELL, json!({ "requestCompletionMarker": true }))
    }

    /// One thread's detail: a snapshot, then events. `turn_limit` bounds the
    /// snapshot to the last N user-anchored turns; `None` omits `turnLimit`
    /// entirely, which the server treats as unbounded (see `turnLimit` on
    /// `orchestration.subscribeThread` in `orchestration.ts`), loading the
    /// thread's full history.
    pub fn subscribe_thread(
        &self,
        thread_id: &str,
        turn_limit: Option<u32>,
    ) -> Result<Subscription<ThreadStreamItem>, RpcError> {
        let mut params = json!({
            "threadId": thread_id,
            "reasoningMessages": true,
            "requestCompletionMarker": true,
        });
        if let Some(turn_limit) = turn_limit {
            params["turnLimit"] = json!(turn_limit);
        }
        self.rpc.subscribe(methods::SUBSCRIBE_THREAD, params)
    }

    /// Dispatch any `ClientOrchestrationCommand` (see `orchestration.ts`).
    pub async fn dispatch(&self, command: Value) -> Result<Value, RpcError> {
        self.rpc.call(methods::DISPATCH_COMMAND, command).await
    }

    /// Start a turn on an existing thread with a plain-text user message.
    pub async fn send_message(&self, thread: &ThreadShell, text: &str) -> Result<Value, RpcError> {
        self.send_message_with_attachments(thread, text, &[]).await
    }

    pub async fn send_message_with_attachments(
        &self,
        thread: &ThreadShell,
        text: &str,
        attachments: &[attachments::UploadedAttachment],
    ) -> Result<Value, RpcError> {
        self.dispatch(json!({
            "type": "thread.turn.start",
            "commandId": new_id(),
            "threadId": thread.id,
            "message": {
                "messageId": new_id(),
                "role": "user",
                "text": text,
                "attachments": attachments,
            },
            "runtimeMode": thread.runtime_mode,
            "interactionMode": thread.interaction_mode,
            "createdAt": now_iso(),
        }))
        .await
    }

    pub async fn interrupt(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Value, RpcError> {
        let mut command = json!({
            "type": "thread.turn.interrupt",
            "commandId": new_id(),
            "threadId": thread_id,
            "createdAt": now_iso(),
        });
        if let Some(turn_id) = turn_id {
            command["turnId"] = json!(turn_id);
        }
        self.dispatch(command).await
    }

    /// `project.create` (see `orchestration.ts`'s `ProjectCreateCommand`).
    /// `project_id` is generated client-side, same as the web app's
    /// `newProjectId()`; the folder is expected to already exist (picked via
    /// a native folder dialog), so `createWorkspaceRootIfMissing` is omitted.
    pub async fn create_project(
        &self,
        project_id: &str,
        title: &str,
        workspace_root: &str,
    ) -> Result<Value, RpcError> {
        self.dispatch(json!({
            "type": "project.create",
            "commandId": new_id(),
            "projectId": project_id,
            "title": title,
            "workspaceRoot": workspace_root,
            "createdAt": now_iso(),
        }))
        .await
    }

    /// `thread.create` (see `orchestration.ts`'s `ThreadCreateCommand`).
    /// Carries the selected model and modes, with no branch/worktree.
    pub async fn create_thread(
        &self,
        thread_id: &str,
        project_id: &str,
        title: &str,
        model_selection: Value,
        runtime_mode: &str,
        interaction_mode: &str,
    ) -> Result<Value, RpcError> {
        self.dispatch(json!({
            "type": "thread.create",
            "commandId": new_id(),
            "threadId": thread_id,
            "projectId": project_id,
            "title": title,
            "modelSelection": model_selection,
            "runtimeMode": runtime_mode,
            "interactionMode": interaction_mode,
            "branch": null,
            "worktreePath": null,
            "createdAt": now_iso(),
        }))
        .await
    }
}

/// A fresh id for a client-generated aggregate (project, thread, message…),
/// same shape as the web app's `newProjectId()` / `newThreadId()`.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn lifecycle_commands_follow_contract_required_fields() {
        let command = ThreadAction::Settle(false).command("thread-1");
        assert_eq!(command["type"], "thread.unsettle");
        assert_eq!(command["reason"], "user");
        assert_eq!(command["threadId"], "thread-1");
        assert!(uuid::Uuid::parse_str(command["commandId"].as_str().unwrap()).is_ok());
        assert!(ThreadAction::Archive.command("thread-1").get("reason").is_none());
        let restore = ThreadAction::Unarchive.command("thread-1");
        assert_eq!(restore["type"], "thread.unarchive");
        assert_eq!(restore["threadId"], "thread-1");
        let rename = ThreadAction::Rename("  New title  ".into()).command("thread-1");
        assert_eq!(rename["type"], "thread.meta.update");
        assert_eq!(rename["title"], "New title");
        assert!(rename.get("modelSelection").is_none());
    }

    #[test]
    fn settings_commands_keep_instance_routing_and_mode_timestamps() {
        let model = json!({ "instanceId": "custom-codex", "model": "model-1", "options": { "effort": "high" } });
        assert_eq!(ThreadAction::Model(model.clone()).command("thread-1")["modelSelection"], model);
        for action in [
            ThreadAction::RuntimeMode("approval-required".into()),
            ThreadAction::InteractionMode("plan".into()),
        ] {
            let command = action.command("thread-1");
            assert!(
                chrono::DateTime::parse_from_rfc3339(command["createdAt"].as_str().unwrap())
                    .is_ok()
            );
        }
    }

    #[test]
    fn server_models_decode_upstream_wire_names_and_optional_metadata() {
        let config: ServerConfig = serde_json::from_value(json!({ "providers": [{
            "instanceId": "my-agent", "driver": "codex", "enabled": true, "installed": true,
            "models": [{ "slug": "model-1", "name": "Model one", "isCustom": false, "capabilities": null }]
        }], "environment": { "environmentId": "env-1" } })).unwrap();
        assert_eq!(config.environment.as_ref().unwrap().environment_id, "env-1");
        assert_eq!(config.providers[0].models[0].id, "model-1");
        assert_eq!(config.providers[0].models[0].label, "Model one");
        assert!(!config.providers[0].requires_new_thread_for_model_change);
    }

    #[test]
    fn question_commands_preserve_answers_and_use_distinct_dismiss_operation() {
        let answers =
            json!({ " exact question ": [" exact value ", "B"], "text": "Written answer" });
        let command =
            ThreadAction::UserInput { request_id: "request-1".into(), answers: answers.clone() }
                .command("thread-1");
        assert_eq!(command["type"], "thread.user-input.respond");
        assert_eq!(command["requestId"], "request-1");
        assert_eq!(command["answers"], answers);
        let dismiss =
            ThreadAction::DismissUserInput { request_id: "request-1".into() }.command("thread-1");
        assert_eq!(dismiss["type"], "thread.user-input.dismiss");
        assert!(dismiss.get("answers").is_none());
    }
}
