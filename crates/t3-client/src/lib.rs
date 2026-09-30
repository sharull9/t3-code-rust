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
pub use state::{ShellState, ThreadState};
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

    /// One thread's detail: a snapshot of the last `turn_limit` turns, then events.
    pub fn subscribe_thread(
        &self,
        thread_id: &str,
        turn_limit: u32,
    ) -> Result<Subscription<ThreadStreamItem>, RpcError> {
        self.rpc.subscribe(
            methods::SUBSCRIBE_THREAD,
            json!({
                "threadId": thread_id,
                "reasoningMessages": true,
                "requestCompletionMarker": true,
                "turnLimit": turn_limit,
            }),
        )
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
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
