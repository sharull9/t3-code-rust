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
pub mod keybindings;
pub mod pending;
pub mod provider_config;
pub mod quotas;
pub mod rpc;
pub mod settings;
pub mod state;
pub mod turn_items;
pub mod types;
pub mod usage;
pub mod workspace;

use std::sync::Arc;

use serde_json::{Value, json};

pub use reqwest;

pub use auth::{Credentials, PairingLink};
pub use error::{Error, RpcError};
pub use keybindings::{KeybindingOp, ResolvedKeybinding};
pub use rpc::{RpcSession, Subscription};
pub use settings::ServerSettings;
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
            Self::Rename(_) => "thread.metadata.update",
            Self::RuntimeMode(_) => "thread.runtime-mode.set",
            Self::InteractionMode(_) => "thread.interaction-mode.set",
            Self::Model(_) => "thread.model-selection.set",
            Self::Approval { .. } | Self::UserInput { .. } => "runtime-request.respond",
            Self::DismissUserInput { .. } => "thread.user-input.dismiss",
        };
        let mut command = json!({ "type": kind, "commandId": new_id(), "threadId": thread_id });
        match self {
            Self::Settle(false) => command["reason"] = json!("user"),
            Self::RuntimeMode(mode) => command["runtimeMode"] = json!(mode),
            Self::InteractionMode(mode) => command["interactionMode"] = json!(mode),
            Self::Model(model) => command["modelSelection"] = model.clone(),
            Self::Rename(title) => command["title"] = json!(title.trim()),
            Self::Approval { request_id, decision } => {
                command["requestId"] = json!(request_id);
                command["decision"] = json!(decision);
            }
            Self::UserInput { request_id, answers } => {
                command["requestId"] = json!(request_id);
                command["answers"] = answers.clone();
            }
            Self::DismissUserInput { request_id } => command["requestId"] = json!(request_id),
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

    /// Dispatch any `OrchestrationV2Command` (see `orchestration.ts`).
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
        // `deliveryIntent: "auto"` lets the server steer or queue behind a
        // running turn instead of the client guessing from stale state.
        self.dispatch(json!({
            "type": "message.dispatch",
            "commandId": new_id(),
            "threadId": thread.id,
            "messageId": new_id(),
            "text": text,
            "attachments": attachments,
            "deliveryIntent": "auto",
            "dispatchMode": { "type": "start_immediately" },
            "createdBy": CREATED_BY,
            "creationSource": CREATION_SOURCE,
        }))
        .await
    }

    /// `run.interrupt`: stops the thread's active run.
    pub async fn interrupt(&self, thread_id: &str, run_id: &str) -> Result<Value, RpcError> {
        self.dispatch(json!({
            "type": "run.interrupt",
            "commandId": new_id(),
            "threadId": thread_id,
            "runId": run_id,
        }))
        .await
    }

    /// `projects.mutate`: create, update or delete a project
    /// (`ProjectMutation` in `project.ts`).
    pub async fn mutate_project(&self, mutation: Value) -> Result<Value, RpcError> {
        self.rpc.call("projects.mutate", mutation).await
    }

    /// `provider.consumeResetCredit`: spends one banked limit reset. `input`
    /// is a [`quotas::LimitAccount::reset_target`]. Returns the outcome
    /// ("reset", "nothingToReset", "noCredit" or "alreadyRedeemed") and any
    /// warning about a follow-up step that failed after the reset.
    pub async fn consume_reset_credit(
        &self,
        input: Value,
    ) -> Result<(String, Option<String>), RpcError> {
        let value: Value = self.rpc.call("provider.consumeResetCredit", input).await?;
        let outcome = value["outcome"]
            .as_str()
            .ok_or_else(|| RpcError::Decode(format!("unexpected reset result: {value}")))?;
        Ok((outcome.to_owned(), value["warning"].as_str().map(str::to_owned)))
    }

    /// `project.create` (see `project.ts`'s `ProjectMutation`).
    /// `project_id` is generated client-side, same as the web app's
    /// `newProjectId()`; the folder is expected to already exist (picked via
    /// a native folder dialog), so `createWorkspaceRootIfMissing` is omitted.
    pub async fn create_project(
        &self,
        project_id: &str,
        title: &str,
        workspace_root: &str,
    ) -> Result<Value, RpcError> {
        self.mutate_project(json!({
            "type": "project.create",
            "commandId": new_id(),
            "projectId": project_id,
            "title": title,
            "workspaceRoot": workspace_root,
        }))
        .await
    }

    /// `thread.create` (see `orchestration.ts`'s `ThreadCreateCommand`).
    /// Carries the selected model and modes; `worktree` is `(branch, path)`
    /// for a thread that runs in its own worktree, `None` for the checkout.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_thread(
        &self,
        thread_id: &str,
        project_id: &str,
        title: &str,
        model_selection: Value,
        runtime_mode: &str,
        interaction_mode: &str,
        worktree: Option<(&str, &str)>,
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
            "branch": worktree.map(|(branch, _)| branch),
            "worktreePath": worktree.map(|(_, path)| path),
            "createdBy": CREATED_BY,
            "creationSource": CREATION_SOURCE,
        }))
        .await
    }

    /// `vcs.createWorktree`: a new branch `new_ref_name` off `base_ref_name`,
    /// checked out at a server-chosen path. Returns `(branch, path)`.
    pub async fn create_worktree(
        &self,
        cwd: &str,
        base_ref_name: &str,
        new_ref_name: &str,
    ) -> Result<(String, String), RpcError> {
        let value: Value = self
            .rpc()
            .call(
                "vcs.createWorktree",
                json!({
                    "cwd": cwd,
                    "refName": base_ref_name,
                    "newRefName": new_ref_name,
                    "baseRefName": base_ref_name,
                    "path": null,
                }),
            )
            .await?;
        let worktree = &value["worktree"];
        match (worktree["refName"].as_str(), worktree["path"].as_str()) {
            (Some(branch), Some(path)) => Ok((branch.to_owned(), path.to_owned())),
            _ => Err(RpcError::Decode(format!("unexpected worktree result: {value}"))),
        }
    }
}

/// A fresh id for a client-generated aggregate (project, thread, message…),
/// same shape as the web app's `newProjectId()` / `newThreadId()`.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `OrchestrationV2CreationFields` for anything the user starts here: the
/// server has no native-desktop source, and "web" is the closest
/// interactive client.
const CREATED_BY: &str = "user";
const CREATION_SOURCE: &str = "web";

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
        assert_eq!(rename["type"], "thread.metadata.update");
        assert_eq!(rename["title"], "New title");
        assert!(rename.get("modelSelection").is_none());
    }

    #[test]
    fn settings_commands_keep_instance_routing() {
        let model = json!({ "instanceId": "custom-codex", "model": "model-1", "options": { "effort": "high" } });
        let command = ThreadAction::Model(model.clone()).command("thread-1");
        assert_eq!(command["type"], "thread.model-selection.set");
        assert_eq!(command["modelSelection"], model);
        let mode = ThreadAction::RuntimeMode("approval-required".into()).command("thread-1");
        assert_eq!(mode["type"], "thread.runtime-mode.set");
        assert_eq!(mode["runtimeMode"], "approval-required");
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
        assert_eq!(command["type"], "runtime-request.respond");
        assert_eq!(command["requestId"], "request-1");
        assert_eq!(command["answers"], answers);
        let dismiss =
            ThreadAction::DismissUserInput { request_id: "request-1".into() }.command("thread-1");
        assert_eq!(dismiss["type"], "thread.user-input.dismiss");
        assert!(dismiss.get("answers").is_none());
    }
}
