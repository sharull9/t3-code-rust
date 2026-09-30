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

pub mod auth;
mod error;
pub mod rpc;
pub mod state;
pub mod types;

use std::sync::Arc;

use serde_json::{Value, json};

pub use reqwest;

pub use auth::{Credentials, PairingLink};
pub use error::{Error, RpcError};
pub use rpc::{RpcSession, Subscription};
pub use state::{ShellState, ThreadState, sort_settled_threads};
pub use types::*;

/// An authenticated RPC session with one environment.
#[derive(Clone)]
pub struct Connection {
    rpc: Arc<RpcSession>,
}

impl Connection {
    pub async fn connect(http: &reqwest::Client, credentials: &Credentials) -> Result<Self, Error> {
        let url = auth::websocket_url(http, credentials).await?;
        let rpc = RpcSession::connect(url.as_str()).await?;
        Ok(Self { rpc })
    }

    pub fn rpc(&self) -> &RpcSession {
        &self.rpc
    }

    pub async fn closed(&self) -> String {
        self.rpc.closed().await
    }

    /// Projects and thread summaries: a snapshot, then live upserts/removals.
    pub fn subscribe_shell(&self) -> Result<Subscription<ShellStreamItem>, RpcError> {
        self.rpc.subscribe(
            methods::SUBSCRIBE_SHELL,
            json!({ "requestCompletionMarker": true }),
        )
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
        self.dispatch(json!({
            "type": "thread.turn.start",
            "commandId": new_id(),
            "threadId": thread.id,
            "message": {
                "messageId": new_id(),
                "role": "user",
                "text": text,
                "attachments": [],
            },
            "runtimeMode": thread.runtime_mode,
            "interactionMode": thread.interaction_mode,
            "createdAt": now_iso(),
        }))
        .await
    }

    pub async fn interrupt(&self, thread_id: &str, turn_id: Option<&str>) -> Result<Value, RpcError> {
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
    /// Mirrors the defaults `ChatView.tsx` sends for a fresh thread:
    /// `DEFAULT_RUNTIME_MODE` ("full-access"), the "default" interaction
    /// mode, and no branch/worktree. `model_selection` should be the
    /// project's `defaultModelSelection` when it has one.
    pub async fn create_thread(
        &self,
        thread_id: &str,
        project_id: &str,
        title: &str,
        model_selection: Value,
    ) -> Result<Value, RpcError> {
        self.dispatch(json!({
            "type": "thread.create",
            "commandId": new_id(),
            "threadId": thread_id,
            "projectId": project_id,
            "title": title,
            "modelSelection": model_selection,
            "runtimeMode": "full-access",
            "interactionMode": "default",
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
