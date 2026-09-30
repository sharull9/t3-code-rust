//! Hand-ported subset of `packages/contracts/src/orchestration.ts`.
//!
//! Only fields the client uses are modeled; unknown fields are ignored so newer
//! servers keep decoding. Keep field names aligned with the TypeScript schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod methods {
    pub const SUBSCRIBE_SHELL: &str = "orchestration.subscribeShell";
    pub const SUBSCRIBE_THREAD: &str = "orchestration.subscribeThread";
    pub const DISPATCH_COMMAND: &str = "orchestration.dispatchCommand";
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectShell {
    pub id: String,
    pub title: String,
    pub workspace_root: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Idle,
    Starting,
    Running,
    Ready,
    Interrupted,
    Stopped,
    Error,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub status: SessionStatus,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub active_turn_id: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Session {
    pub fn is_working(&self) -> bool {
        matches!(self.status, SessionStatus::Starting | SessionStatus::Running)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadShell {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// `RuntimeMode`: "approval-required" | "auto-accept-edits" | "auto" | "full-access".
    pub runtime_mode: String,
    /// `ProviderInteractionMode`: "default" | "plan".
    #[serde(default = "default_interaction_mode")]
    pub interaction_mode: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub session: Option<Session>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub pinned_at: Option<String>,
    #[serde(default)]
    pub has_pending_approvals: bool,
    #[serde(default)]
    pub has_pending_user_input: bool,
}

fn default_interaction_mode() -> String {
    "default".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    pub snapshot_sequence: u64,
    pub projects: Vec<ProjectShell>,
    pub threads: Vec<ThreadShell>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ShellStreamItem {
    Synchronized,
    Snapshot { snapshot: ShellSnapshot },
    ProjectUpserted { sequence: u64, project: ProjectShell },
    ProjectRemoved { sequence: u64, project_id: String },
    ThreadUpserted { sequence: u64, thread: ThreadShell },
    ThreadRemoved { sequence: u64, thread_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Reasoning,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub text: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub streaming: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub branch: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub session: Option<Session>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetailSnapshot {
    pub snapshot_sequence: u64,
    pub thread: ThreadDetail,
}

/// Orchestration events stay loosely typed; reducers decode the payloads they need.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationEvent {
    pub sequence: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub aggregate_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ThreadStreamItem {
    Synchronized,
    Snapshot { snapshot: ThreadDetailSnapshot },
    Event { event: OrchestrationEvent },
}

/// Payload of `thread.message-sent`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSentPayload {
    pub message_id: String,
    pub role: MessageRole,
    pub text: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub streaming: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Payload of `thread.session-set`.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSetPayload {
    pub session: Session,
}
