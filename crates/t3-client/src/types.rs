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
    /// The project's saved model, used to seed a new thread. `None` when the
    /// project has never had one set; kept opaque (raw JSON) since this
    /// client neither lists providers nor edits the selection itself.
    #[serde(default)]
    pub default_model_selection: Option<Value>,
    /// Project actions (`ProjectScript`), run from the info panel and
    /// optionally on worktree creation.
    #[serde(default)]
    pub scripts: Vec<ProjectScript>,
}

/// One project action (see `orchestration.ts`'s `ProjectScript`). Written
/// back whole through `project.meta.update`, so every field round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScript {
    pub id: String,
    pub name: String,
    pub command: String,
    /// `play`, `test`, `lint`, `configure`, `build` or `debug`.
    pub icon: String,
    pub run_on_worktree_create: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#async: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_open_preview: Option<bool>,
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
    #[serde(default)]
    pub model_selection: Option<Value>,
    #[serde(default)]
    pub worktree_path: Option<String>,
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
    pub created_at: String,
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
    /// "settled" | "active" | absent. Set only by explicit `thread.settle` /
    /// `thread.unsettle` (or server auto-settle) — see
    /// `packages/client-runtime/src/state/threadSettled.ts` and the
    /// classification in `apps/web/src/components/Sidebar.tsx` ("settled"
    /// iff this is exactly `Some("settled")"; never inferred from turn state).
    #[serde(default)]
    pub settled_override: Option<String>,
    /// When the thread last settled. Sorts the Settled shelf (most recent
    /// first); falls back to message/turn timestamps, see
    /// [`ThreadShell::settled_timestamp`].
    #[serde(default)]
    pub settled_at: Option<String>,
    #[serde(default)]
    pub latest_user_message_at: Option<String>,
    #[serde(default)]
    pub latest_turn: Option<LatestTurn>,
}

/// Configured instances and model IDs come from the connected server.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(default)]
    pub environment: Option<ServerEnvironment>,
    #[serde(default)]
    pub providers: Vec<ServerProvider>,
    #[serde(default)]
    pub usage_limit_sources: Vec<crate::quotas::LimitSource>,
    /// Editor IDs (`editor.ts`'s `EditorId`) installed on the server machine.
    #[serde(default)]
    pub available_editors: Vec<String>,
    /// Effective keybindings (defaults merged with the user's overrides).
    #[serde(default, deserialize_with = "crate::quotas::compatible_array")]
    pub keybindings: Vec<crate::keybindings::ResolvedKeybinding>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEnvironment {
    pub environment_id: String,
    #[serde(default)]
    pub capabilities: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProvider {
    pub instance_id: String,
    pub driver: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub enabled: bool,
    pub installed: bool,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub requires_new_thread_for_model_change: bool,
    #[serde(default)]
    pub models: Vec<ServerModel>,
    #[serde(default)]
    pub auth: crate::quotas::ProviderAuth,
    #[serde(default)]
    pub usage_limits: Option<crate::quotas::UsageLimits>,
    #[serde(default)]
    pub skills: Vec<ProviderSkill>,
    /// Skills discovered per workspace, in addition to the global `skills`.
    #[serde(default)]
    pub workspace_snapshots: Vec<ProviderWorkspaceSnapshot>,
}

impl ServerProvider {
    /// The account the instance is signed in to: its email, else the plan
    /// label ("ChatGPT Plus Subscription").
    pub fn account(&self) -> Option<&str> {
        [self.auth.email.as_deref(), self.auth.label.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|text| !text.is_empty())
    }

    /// Skills a user can start from the composer in `cwd`: workspace skills
    /// first, then global ones, deduplicated by name.
    pub fn invocable_skills(&self, cwd: Option<&str>) -> Vec<&ProviderSkill> {
        let workspace = self
            .workspace_snapshots
            .iter()
            .filter(|snapshot| cwd.is_some_and(|cwd| same_path(&snapshot.cwd, cwd)))
            .flat_map(|snapshot| &snapshot.skills);
        let mut seen = std::collections::HashSet::new();
        workspace
            .chain(&self.skills)
            .filter(|skill| skill.enabled && skill.user_invocable != Some(false))
            .filter(|skill| seen.insert(skill.name.trim().to_lowercase()))
            .collect()
    }
}

fn same_path(a: &str, b: &str) -> bool {
    let normalize =
        |path: &str| path.trim_end_matches(['/', '\\']).replace('\\', "/").to_lowercase();
    normalize(a) == normalize(b)
}

/// Port of `ServerProviderSkill`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSkill {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub short_description: Option<String>,
    #[serde(default)]
    pub user_invocable: Option<bool>,
}

fn enabled_default() -> bool {
    true
}

impl ProviderSkill {
    /// Mirrors `formatProviderSkillDisplayName`: the display name, else the
    /// name in title case ("repo-explorer" → "Repo Explorer").
    pub fn label(&self) -> String {
        if let Some(name) = self.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            return name.to_owned();
        }
        self.name
            .split(['-', '_', ' ', ':'])
            .filter(|word| !word.is_empty())
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn summary(&self) -> Option<&str> {
        self.short_description.as_deref().or(self.description.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderWorkspaceSnapshot {
    pub cwd: String,
    #[serde(default)]
    pub skills: Vec<ProviderSkill>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerModel {
    #[serde(rename = "slug")]
    pub id: String,
    #[serde(rename = "name")]
    pub label: String,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
}

fn default_interaction_mode() -> String {
    "default".into()
}

/// Timing subset of `OrchestrationLatestTurn`, enough to resolve a settled
/// thread's sort timestamp.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestTurn {
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

impl ThreadShell {
    /// Mirrors `Sidebar.tsx`'s classification: settled is an explicit,
    /// user-driven (or server auto-settle) state, never derived from turn
    /// status. A thread with no `settledOverride` is active.
    pub fn is_settled(&self) -> bool {
        self.settled_override.as_deref() == Some("settled")
    }

    /// Whether a turn has ever been requested. A started thread is bound to
    /// its provider instance; changing provider means a new thread.
    pub fn is_started(&self) -> bool {
        self.latest_turn.is_some() || self.latest_user_message_at.is_some()
    }

    /// Port of `resolveSettledThreadTimestamp` in
    /// `packages/client-runtime/src/state/threadSort.ts`: `settledAt` when
    /// stamped, otherwise the latest of the user message / turn timestamps,
    /// then `updatedAt`.
    pub fn settled_timestamp(&self) -> Option<&str> {
        if self.settled_at.is_some() {
            return self.settled_at.as_deref();
        }
        let mut latest: Option<&str> = None;
        for candidate in [
            self.latest_user_message_at.as_deref(),
            self.latest_turn.as_ref().and_then(|t| t.requested_at.as_deref()),
            self.latest_turn.as_ref().and_then(|t| t.started_at.as_deref()),
            self.latest_turn.as_ref().and_then(|t| t.completed_at.as_deref()),
        ] {
            let Some(candidate) = candidate else { continue };
            if latest.is_none_or(|current| candidate > current) {
                latest = Some(candidate);
            }
        }
        latest.or(if self.updated_at.is_empty() { None } else { Some(self.updated_at.as_str()) })
    }
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
    Snapshot {
        snapshot: ShellSnapshot,
    },
    #[serde(alias = "project.updated")]
    ProjectUpserted {
        sequence: u64,
        project: ProjectShell,
    },
    #[serde(alias = "project.removed")]
    ProjectRemoved {
        sequence: u64,
        project_id: String,
    },
    #[serde(alias = "thread.updated")]
    ThreadUpserted {
        sequence: u64,
        thread: ThreadShell,
    },
    #[serde(alias = "thread.removed")]
    ThreadRemoved {
        sequence: u64,
        thread_id: String,
    },
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
    #[serde(default)]
    pub attachments: Vec<crate::attachments::UploadedAttachment>,
    pub role: MessageRole,
    pub text: String,
    #[serde(default, alias = "runId")]
    pub turn_id: Option<String>,
    pub streaming: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// `OrchestrationThreadActivityTone`: how a `thread.activity.append`d item
/// should read. `Tool` is what a provider's tool calls arrive as; the
/// transcript groups consecutive `Tool` activities into one collapsible row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityTone {
    Info,
    Tool,
    Approval,
    Error,
    #[serde(other)]
    Unknown,
}

/// `OrchestrationThreadActivity`. `kind` and `payload` stay loosely typed on
/// the wire (new tool lifecycle kinds must keep decoding); this client only
/// renders `summary` and reads approval details from the opaque `payload`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub tone: ActivityTone,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
    /// Tool calls and other non-message activity, interleaved into the
    /// transcript by `created_at`. Optional on the wire so snapshots from
    /// older servers still decode.
    #[serde(default)]
    pub activities: Vec<Activity>,
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
    #[serde(alias = "threadId")]
    pub aggregate_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub enum ThreadStreamItem {
    Synchronized,
    Snapshot { snapshot: ThreadDetailSnapshot },
    Event { event: OrchestrationEvent },
}

impl<'de> Deserialize<'de> for ThreadStreamItem {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut value = Value::deserialize(deserializer)?;
        match value.get("kind").and_then(Value::as_str) {
            Some("synchronized") => Ok(Self::Synchronized),
            Some("snapshot") => {
                if let Some(projection) = value.get("projection") {
                    // V2 puts the sequence and projection directly on the frame.
                    let mut thread = projection["thread"].clone();
                    thread["messages"] = projection["messages"].clone();
                    let snapshot = serde_json::from_value(serde_json::json!({
                        "snapshotSequence": value["snapshotSequence"],
                        "thread": thread,
                    }))
                    .map_err(D::Error::custom)?;
                    Ok(Self::Snapshot { snapshot })
                } else {
                    let snapshot = serde_json::from_value(value["snapshot"].take())
                        .map_err(D::Error::custom)?;
                    Ok(Self::Snapshot { snapshot })
                }
            }
            Some("event") => {
                // V1 stores the sequence on the event; V2 stores it on the frame.
                if let Some(sequence) = value.get("sequence").cloned() {
                    value["event"]["sequence"] = sequence;
                }
                let event =
                    serde_json::from_value(value["event"].take()).map_err(D::Error::custom)?;
                Ok(Self::Event { event })
            }
            _ => Err(D::Error::custom("unknown thread stream item kind")),
        }
    }
}

/// Payload of `thread.message-sent`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSentPayload {
    pub message_id: String,
    #[serde(default)]
    pub attachments: Vec<crate::attachments::UploadedAttachment>,
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

/// Payload of `thread.activity-appended`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityAppendedPayload {
    pub activity: Activity,
}
