//! Typed RPC helpers for the native workspace panels.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Connection, RpcError, Subscription};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub path: String,
    pub kind: String,
    #[serde(default)]
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectory {
    pub entries: Vec<WorkspaceEntry>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub relative_path: String,
    pub contents: String,
    pub byte_length: u64,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatusFile {
    pub path: String,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitStatus {
    pub is_repo: bool,
    pub has_working_tree_changes: bool,
    pub ref_name: Option<String>,
    pub working_tree: WorkspaceWorkingTree,
    #[serde(default)]
    pub is_default_ref: bool,
    #[serde(default)]
    pub has_upstream: bool,
    #[serde(default)]
    pub ahead_count: u32,
    #[serde(default)]
    pub behind_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWorkingTree {
    pub files: Vec<WorkspaceStatusFile>,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRefs {
    pub refs: Vec<WorkspaceRef>,
    pub is_repo: bool,
    pub has_primary_remote: bool,
    pub next_cursor: Option<u32>,
    pub total_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRef {
    pub name: String,
    #[serde(default)]
    pub is_remote: bool,
    pub current: bool,
    pub is_default: bool,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffPreview {
    pub cwd: String,
    #[serde(default)]
    pub sources: Vec<WorkspaceDiffSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffSource {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub diff: String,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub files: Vec<WorkspaceDiffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffFile {
    pub path: String,
    #[serde(default)]
    pub previous_path: Option<String>,
    pub additions: i32,
    pub deletions: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTerminal {
    pub thread_id: String,
    pub terminal_id: String,
    pub cwd: String,
    pub status: String,
    pub history: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WorkspaceTerminalEvent {
    Snapshot {
        snapshot: WorkspaceTerminal,
    },
    Output {
        thread_id: String,
        terminal_id: String,
        data: String,
    },
    Exited {
        thread_id: String,
        terminal_id: String,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
    },
    Closed {
        thread_id: String,
        terminal_id: String,
    },
    Error {
        thread_id: String,
        terminal_id: String,
        message: String,
    },
    Cleared {
        thread_id: String,
        terminal_id: String,
    },
    Restarted {
        snapshot: WorkspaceTerminal,
    },
    Activity {
        thread_id: String,
        terminal_id: String,
        has_running_subprocess: bool,
        label: String,
    },
}

impl Connection {
    /// Attaches to the selected remote terminal and yields its snapshot and live output events.
    pub fn subscribe_terminal(
        &self,
        thread_id: &str,
        terminal_id: &str,
        cwd: Option<&str>,
    ) -> Result<Subscription<WorkspaceTerminalEvent>, RpcError> {
        let mut payload = json!({ "threadId": thread_id, "terminalId": terminal_id });
        if let Some(cwd) = cwd {
            payload["cwd"] = json!(cwd);
        }
        self.rpc().subscribe("terminal.attach", payload)
    }

    /// Runs a project's setup script (`ProjectScript::run_on_worktree_create`)
    /// in a thread's terminal, the way the server does for threads it puts
    /// in a worktree: in the worktree, with `T3CODE_PROJECT_ROOT` and
    /// `T3CODE_WORKTREE_PATH` set. Returns once the command is typed in,
    /// or with `wait`, once it stops running (see [`wait_for_idle`]).
    pub async fn run_setup_script(
        &self,
        thread_id: &str,
        terminal_id: &str,
        project_root: &str,
        worktree_path: &str,
        command: &str,
        wait: bool,
    ) -> Result<(), RpcError> {
        self.rpc()
            .call::<Value>(
                "terminal.open",
                json!({
                    "threadId": thread_id,
                    "terminalId": terminal_id,
                    "cwd": worktree_path,
                    "worktreePath": worktree_path,
                    "env": {
                        "T3CODE_PROJECT_ROOT": project_root,
                        "T3CODE_WORKTREE_PATH": worktree_path,
                    },
                }),
            )
            .await?;
        // Attach before typing so the script's first activity isn't missed.
        let stream = if wait {
            Some(self.subscribe_terminal(thread_id, terminal_id, Some(worktree_path))?)
        } else {
            None
        };
        self.rpc()
            .call::<Value>(
                "terminal.write",
                json!({ "threadId": thread_id, "terminalId": terminal_id, "data": format!("{command}\r") }),
            )
            .await?;
        if let Some(stream) = stream {
            wait_for_idle(stream).await;
        }
        Ok(())
    }
}

/// Waits for a terminal's command to finish: its shell reports a running
/// subprocess, then none. A command that never shows as running within
/// [`SETUP_START_TIMEOUT`] counts as done, and none is waited on longer than
/// [`SETUP_RUN_TIMEOUT`], so a stuck script can't hold the thread forever.
async fn wait_for_idle(mut stream: Subscription<WorkspaceTerminalEvent>) {
    let mut running = false;
    let deadline = tokio::time::Instant::now() + SETUP_RUN_TIMEOUT;
    loop {
        let limit = if running {
            deadline
        } else {
            deadline.min(tokio::time::Instant::now() + SETUP_START_TIMEOUT)
        };
        let Ok(Some(Ok(event))) = tokio::time::timeout_at(limit, stream.next()).await else {
            return;
        };
        match event {
            WorkspaceTerminalEvent::Activity { has_running_subprocess, .. } => {
                if running && !has_running_subprocess {
                    return;
                }
                running |= has_running_subprocess;
            }
            WorkspaceTerminalEvent::Exited { .. }
            | WorkspaceTerminalEvent::Closed { .. }
            | WorkspaceTerminalEvent::Error { .. } => return,
            _ => {}
        }
    }
}

const SETUP_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const SETUP_RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBrowseResult {
    pub parent_path: String,
    pub entries: Vec<WorkspaceBrowseEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBrowseEntry {
    pub name: String,
    pub full_path: String,
}

/// `git.runStackedAction`'s `GitStackedAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitAction {
    Commit,
    Push,
    CreatePr,
    CommitPush,
}

impl GitAction {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Push => "push",
            Self::CreatePr => "create_pr",
            Self::CommitPush => "commit_push",
        }
    }
}

/// What a finished git action reports: the server's toast text and, for a
/// created pull request, its URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitActionOutcome {
    pub title: String,
    pub description: Option<String>,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRequest {
    ListDirectory { cwd: String, directory_path: Option<String> },
    ReadFile { cwd: String, relative_path: String },
    GitStatus { cwd: String },
    ListRefs { cwd: String },
    SwitchRef { cwd: String, ref_name: String },
    DiffPreview { cwd: String },
    DiffFile { cwd: String, path: String },
    BrowseDirectories { partial_path: String, cwd: Option<String> },
    OpenTerminal { thread_id: String, terminal_id: String, cwd: String },
    RestartTerminal { thread_id: String, terminal_id: String, cwd: String },
    WriteTerminal { thread_id: String, terminal_id: String, data: String },
    ResizeTerminal { thread_id: String, terminal_id: String, cols: u16, rows: u16 },
    CloseTerminal { thread_id: String, terminal_id: Option<String> },
    OpenInEditor { cwd: String, editor: String },
    /// Runs to completion: the RPC streams progress and ends with
    /// `action_finished` or `action_failed`.
    RunGitAction { cwd: String, action: GitAction, thread_id: Option<String> },
    /// Replaces the project's whole action list (`project.meta.update`).
    SetProjectScripts { project_id: String, scripts: Vec<crate::ProjectScript> },
    /// `projects.searchEntries`: fuzzy file search for `@` mentions. An empty
    /// query returns recently used entries.
    SearchEntries { cwd: String, query: String, limit: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceResponse {
    Directory(WorkspaceDirectory),
    File(WorkspaceFile),
    GitStatus(WorkspaceGitStatus),
    Refs(WorkspaceRefs),
    DiffPreview(WorkspaceDiffPreview),
    BrowseDirectories(WorkspaceBrowseResult),
    Terminal(WorkspaceTerminal),
    GitAction(GitActionOutcome),
    Entries(WorkspaceDirectory),
    Ack,
}

impl WorkspaceRequest {
    /// Executes one server RPC using the public connection session. The response shape is
    /// decoded here so UI and backend code can stay independent of JSON details.
    pub async fn execute(&self, connection: &Connection) -> Result<WorkspaceResponse, RpcError> {
        let (method, payload, response) = match self {
            Self::RunGitAction { cwd, action, thread_id } => {
                return run_git_action(connection, cwd, *action, thread_id.as_deref())
                    .await
                    .map(WorkspaceResponse::GitAction);
            }
            Self::SetProjectScripts { project_id, scripts } => {
                connection
                    .mutate_project(json!({
                        "type": "project.update",
                        "commandId": crate::new_id(),
                        "projectId": project_id,
                        "scripts": scripts,
                    }))
                    .await?;
                return Ok(WorkspaceResponse::Ack);
            }
            Self::OpenInEditor { cwd, editor } => (
                "shell.openInEditor",
                json!({ "cwd": cwd, "editor": editor }),
                ResponseKind::Ack,
            ),
            Self::ListDirectory { cwd, directory_path } => (
                "projects.listEntries",
                list_directory_payload(cwd, directory_path.as_deref()),
                ResponseKind::Directory,
            ),
            Self::ReadFile { cwd, relative_path } => (
                "projects.readFile",
                json!({ "cwd": cwd, "relativePath": relative_path }),
                ResponseKind::File,
            ),
            Self::GitStatus { cwd } => {
                ("vcs.refreshStatus", json!({ "cwd": cwd }), ResponseKind::GitStatus)
            }
            Self::ListRefs { cwd } => (
                "vcs.listRefs",
                json!({ "cwd": cwd, "refKind": "local", "limit": 200 }),
                ResponseKind::Refs,
            ),
            Self::SwitchRef { cwd, ref_name } => {
                ("vcs.switchRef", json!({ "cwd": cwd, "refName": ref_name }), ResponseKind::Ack)
            }
            Self::DiffPreview { cwd } => (
                "review.getDiffPreview",
                json!({ "cwd": cwd, "ignoreWhitespace": false }),
                ResponseKind::DiffPreview,
            ),
            Self::DiffFile { cwd, path } => (
                "review.getDiffPreview",
                json!({ "cwd": cwd, "ignoreWhitespace": false, "file": { "path": path, "previousPath": null, "sourceKind": "working-tree" } }),
                ResponseKind::DiffPreview,
            ),
            Self::BrowseDirectories { partial_path, cwd } => (
                "filesystem.browse",
                browse_payload(partial_path, cwd.as_deref()),
                ResponseKind::BrowseDirectories,
            ),
            Self::OpenTerminal { thread_id, terminal_id, cwd } => (
                "terminal.open",
                json!({ "threadId": thread_id, "terminalId": terminal_id, "cwd": cwd }),
                ResponseKind::Terminal,
            ),
            Self::RestartTerminal { thread_id, terminal_id, cwd } => (
                "terminal.restart",
                json!({ "threadId": thread_id, "terminalId": terminal_id, "cwd": cwd, "cols": 80, "rows": 24 }),
                ResponseKind::Terminal,
            ),
            Self::WriteTerminal { thread_id, terminal_id, data } => (
                "terminal.write",
                json!({ "threadId": thread_id, "terminalId": terminal_id, "data": data }),
                ResponseKind::Ack,
            ),
            Self::ResizeTerminal { thread_id, terminal_id, cols, rows } => (
                "terminal.resize",
                json!({ "threadId": thread_id, "terminalId": terminal_id, "cols": cols, "rows": rows }),
                ResponseKind::Ack,
            ),
            Self::CloseTerminal { thread_id, terminal_id } => (
                "terminal.close",
                close_terminal_payload(thread_id, terminal_id.as_deref()),
                ResponseKind::Ack,
            ),
            Self::SearchEntries { cwd, query, limit } => (
                "projects.searchEntries",
                json!({ "cwd": cwd, "query": query.trim(), "limit": limit }),
                ResponseKind::Entries,
            ),
        };
        let value: Value = connection.rpc().call(method, payload).await?;
        match response {
            ResponseKind::Directory => decode(value).map(WorkspaceResponse::Directory),
            ResponseKind::File => decode(value).map(WorkspaceResponse::File),
            ResponseKind::GitStatus => decode(value).map(WorkspaceResponse::GitStatus),
            ResponseKind::Refs => decode(value).map(WorkspaceResponse::Refs),
            ResponseKind::DiffPreview => decode(value).map(WorkspaceResponse::DiffPreview),
            ResponseKind::BrowseDirectories => {
                decode(value).map(WorkspaceResponse::BrowseDirectories)
            }
            ResponseKind::Terminal => decode(value).map(WorkspaceResponse::Terminal),
            ResponseKind::Entries => decode(value).map(WorkspaceResponse::Entries),
            ResponseKind::Ack => Ok(WorkspaceResponse::Ack),
        }
    }
}

async fn run_git_action(
    connection: &Connection,
    cwd: &str,
    action: GitAction,
    thread_id: Option<&str>,
) -> Result<GitActionOutcome, RpcError> {
    let mut payload =
        json!({ "actionId": crate::new_id(), "cwd": cwd, "action": action.wire() });
    if let Some(thread_id) = thread_id {
        payload["threadId"] = json!(thread_id);
    }
    let mut progress = connection.rpc().subscribe::<Value>("git.runStackedAction", payload)?;
    while let Some(event) = progress.next().await {
        if let Some(outcome) = git_action_outcome(event?)? {
            return Ok(outcome);
        }
    }
    Err(RpcError::Decode("git action ended without a result".into()))
}

/// `Some` once a progress event finishes the action; failures become errors.
fn git_action_outcome(event: Value) -> Result<Option<GitActionOutcome>, RpcError> {
    match event["kind"].as_str() {
        Some("action_finished") => {
            let result = &event["result"];
            let text = |value: &Value| value.as_str().map(str::to_owned);
            Ok(Some(GitActionOutcome {
                title: text(&result["toast"]["title"]).unwrap_or_else(|| "Done".into()),
                description: text(&result["toast"]["description"]),
                pr_url: text(&result["pr"]["url"]),
            }))
        }
        Some("action_failed") => Err(RpcError::Failure(json!({
            "_tag": "GitActionFailed",
            "message": event["message"].as_str().unwrap_or("Git action failed"),
        }))),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Copy)]
enum ResponseKind {
    Directory,
    File,
    GitStatus,
    Refs,
    DiffPreview,
    BrowseDirectories,
    Terminal,
    Entries,
    Ack,
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RpcError> {
    serde_json::from_value(value).map_err(|error| RpcError::Decode(error.to_string()))
}

fn browse_payload(partial_path: &str, cwd: Option<&str>) -> Value {
    let partial_path = partial_path.trim();
    let mut payload = json!({
        // `~` is a server-supported home-directory browse token. `.` is
        // explicitly relative and requires a selected project cwd.
        "partialPath": if partial_path.is_empty() { "~" } else { partial_path }
    });
    if let Some(cwd) = cwd {
        payload["cwd"] = json!(cwd);
    }
    payload
}

fn list_directory_payload(cwd: &str, directory_path: Option<&str>) -> Value {
    let mut payload = json!({ "cwd": cwd });
    if let Some(directory_path) = directory_path {
        payload["directoryPath"] = json!(directory_path);
    }
    payload
}

fn close_terminal_payload(thread_id: &str, terminal_id: Option<&str>) -> Value {
    let mut payload = json!({ "threadId": thread_id });
    if let Some(terminal_id) = terminal_id {
        payload["terminalId"] = json!(terminal_id);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_workspace_shapes_from_contracts() {
        let listing: WorkspaceDirectory = serde_json::from_value(json!({
            "entries": [{ "path": "src/main.rs", "kind": "file", "ignored": false }],
            "truncated": false
        }))
        .unwrap();
        assert_eq!(listing.entries[0].path, "src/main.rs");

        let status: WorkspaceGitStatus = serde_json::from_value(json!({
            "isRepo": true,
            "hasPrimaryRemote": true,
            "isDefaultRef": true,
            "refName": "main",
            "hasWorkingTreeChanges": true,
            "workingTree": { "files": [{ "path": "src/main.rs", "insertions": 2, "deletions": 1 }], "insertions": 2, "deletions": 1 },
            "hasUpstream": true,
            "aheadCount": 0,
            "behindCount": 0,
            "pr": null
        })).unwrap();
        assert_eq!(status.working_tree.files[0].deletions, 1);

        let diff: WorkspaceDiffPreview = serde_json::from_value(json!({
            "cwd": "/workspace",
            "generatedAt": "2026-10-01T00:00:00.000Z",
            "sources": [{
                "id": "working-tree",
                "kind": "working-tree",
                "title": "Working tree",
                "baseRef": null,
                "headRef": null,
                "diff": "--- a/src/main.rs\n+++ b/src/main.rs\n",
                "diffHash": "abc123",
                "truncated": false
            }]
        }))
        .unwrap();
        assert_eq!(diff.sources[0].diff, "--- a/src/main.rs\n+++ b/src/main.rs\n");
    }

    #[test]
    fn git_action_stream_ends_on_finish_or_failure() {
        assert_eq!(git_action_outcome(json!({ "kind": "phase_started", "phase": "commit" })).unwrap(), None);
        let finished = git_action_outcome(json!({
            "kind": "action_finished",
            "result": {
                "toast": { "title": "Pushed main", "cta": { "kind": "none" } },
                "pr": { "status": "created", "url": "https://github.com/o/r/pull/1" }
            }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(finished.title, "Pushed main");
        assert_eq!(finished.pr_url.as_deref(), Some("https://github.com/o/r/pull/1"));
        assert!(matches!(
            git_action_outcome(json!({ "kind": "action_failed", "message": "nothing to commit" })),
            Err(RpcError::Failure(value)) if value["message"] == "nothing to commit"
        ));
    }

    #[test]
    fn omits_optional_inputs_instead_of_sending_null() {
        assert_eq!(browse_payload("~", None), json!({ "partialPath": "~" }));
        assert_eq!(browse_payload("", None), json!({ "partialPath": "~" }));
        assert_eq!(
            browse_payload(".", Some("/workspace")),
            json!({
                "partialPath": ".",
                "cwd": "/workspace"
            })
        );
        assert_eq!(
            browse_payload("  ", Some("/workspace")),
            json!({
                "partialPath": "~",
                "cwd": "/workspace"
            })
        );
        assert_eq!(list_directory_payload("/workspace", None), json!({ "cwd": "/workspace" }));
        assert_eq!(close_terminal_payload("thread-1", None), json!({ "threadId": "thread-1" }));
    }
}
