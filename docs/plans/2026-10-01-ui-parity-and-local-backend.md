# UI parity and a local backend

The goal is for the GPUI client to match T3 Code's desktop layout and basic
behavior, using the same server commands and streams. This is a continuation
plan, not a claim that the whole desktop app has been ported.

## What this pass adds

- Model menus populated from `server.getConfig`, routed by provider `instanceId`.
- Runtime and Build/Plan menus backed by the corresponding orchestration commands.
- Pin/unpin, settle/move to active, archive and rename actions in sidebar thread menus.
- A searchable archived-thread shelf fetched from `orchestration.getArchivedShellSnapshot`,
  with refresh and restore controls. Failed requests can be retried; late query results
  cannot override a newer archive query or a live restored-thread entry.
- Rename drafts retained after errors and duplicate submissions blocked.
- New threads inherit the current runtime and interaction modes. Destination-project
  model defaults take precedence, then the current model, then server defaults.
- Approval cards with provider-supplied choices and warnings, backed by
  `thread.approval.respond`. Resolved and stale requests disappear; retryable
  failures leave the request available.
- Composer and question drafts persisted across restarts by server/environment.
  Drafts clear after an accepted send and survive rejected sends. Saves are
  debounced on a dedicated disk worker and flushed when the app is released.
- File uploads with retry/removal controls, server attachment IDs on sends,
  and message download links. Newly added files survive earlier send acknowledgements.
- Server-side project folder browsing, file previews, Git status/diffs and branch
  switching, and terminal commands with streamed output.
- Live server config updates, provider availability, theme controls and app shortcuts.
- Duplicate sends blocked while a send is pending or a turn is running.
- Worktree versus local-checkout labels from server state.
- Project/thread creation controls disabled while disconnected.
- An archived or removed open thread closes its detail subscription.
- User-input forms preserve native question IDs and option values, handle
  single/multiple choices and custom replies, validate all answers before
  submission, and dismiss only asynchronous questions. Answers survive thread
  switching, reconnects and retryable errors. Replaced custom text moves into
  the composer rather than disappearing.

The existing dark surfaces, sidebar cards, settled shelf, title-bar breadcrumbs,
centered transcript, composer placement, markdown, collapsed thoughts and tool
groups remain the visual reference. New controls occupy those existing positions.
Visual equivalence still needs a side-by-side native-app check.

## Backend decision

Yes, this client can use the T3 backend directly. It already does. `t3-client`
exchanges credentials, opens an Effect RPC WebSocket, subscribes to projections,
and dispatches commands. The GPUI executable does not run agent or git logic.

Assumption: "paring" means manual pairing. If it means parsing, native Rust still
needs to decode the server's JSON protocol. Sharing the backend removes duplicate
agent/business logic; it does not remove serialization. A contract generator and
recorded protocol fixtures would reduce the maintenance of hand-ported types.

Three connection approaches:

| Approach | Manual pairing | Tradeoff |
| --- | --- | --- |
| Existing server, current credentials | Once per environment | Best for remote/shared servers; current implementation |
| GPUI-managed local child server | No pasted pairing link | Recommended local desktop path; must own startup, shutdown, version and data directory |
| Reimplement server logic or read SQLite directly | No | Reject: bypasses command validation, subscriptions, provider lifecycle and migrations |

T3's desktop protocol already supports a reusable private bootstrap credential.
The existing `crates/t3-client/examples/embedded.rs` demonstrates the launch and
authentication sequence. Settings now starts an explicitly selected executable
with the same private bootstrap, isolated data directory and owned process.
Credentials remain in memory. Startup waits for readiness, reconnects refresh
expired credentials, an exited child is restarted, and dropping the backend kills
its owned child. This path still needs executable discovery/bundling, persisted
startup preferences and a readable server-log view. Current stderr is discarded.

## Local backend implementation sequence

1. Add a connection choice: "Local server" or "Connect to server". Keep the
   current pairing flow for an independently owned server.
2. Locate an explicitly configured T3 executable first. Add a packaged executable
   once supported server versions and distribution packaging are defined. Do not
   silently download an arbitrary latest version on each launch.
3. Select an unused loopback port, generate a fresh private bootstrap token, and
   spawn the executable with `--bootstrap-fd 0`. Send the envelope through stdin:
   `mode: desktop`, `host: 127.0.0.1`, the port, `t3Home`, `noBrowser: true`, and
   `desktopBootstrapToken`. Never put the token in logs or command-line arguments.
4. Use a dedicated GPUI-managed T3 data directory by default. Display that choice
   in connection settings. Do not start a second server against a running desktop
   server's database. Existing T3 history remains available through the remote/
   existing-server option. Sharing an existing server requires its normal grant.
5. Poll `/.well-known/t3/environment` with a bounded startup deadline while also
   checking child exit. Retain stderr in a local log with a readable failure view.
6. Exchange the bootstrap token at `/oauth/token`, obtain a socket ticket, and use
   the existing `Connection`, shell reducer, and thread subscriptions unchanged.
   The token exchange still happens automatically; authentication is not disabled.
7. On expiry or a managed-server restart, exchange the reusable bootstrap token
   again. Keep managed credentials in memory rather than writing them into the
   saved credentials for an independently owned server.
8. Keep the child handle for the application's lifetime, stop only that child on
   quit or connection-mode changes, and use bounded restart backoff on crashes.
   Avoid orphan children; do not terminate independently owned T3 servers.
9. Check server version/capabilities before enabling newer commands. Report
   unsupported features without dropping the whole connection.

Validate fresh local startup without a pairing link, child startup failure, port
conflicts, token expiry, process crash/restart, application quit, and switching
between managed and independently owned servers. Use isolated test data.

## Remaining parity work, in order

1. Question-form polish: attachments, option number shortcuts, collapse/expand,
   and the web client's delayed automatic advance for single choices. Core
   answer submission, navigation, Enter-to-advance and dismissal are implemented.
2. Thread management: snooze, manual ordering and archived-thread detail previews.
   Rename, archived-thread browsing/restore and model/mode inheritance are implemented.
   Verify inherited modes against provider-specific capabilities.
3. Composer: provider model options and slash commands remain. Uploads, persisted
   environment-scoped drafts and shortcuts are implemented. Verify live upload/send
   acceptance and recovery after ambiguous acknowledgements.
4. Workspace UI: worktree/branch creation, editor integration and full terminal
   emulation remain. Server-side files/folders, diffs, branch switching and command
   terminals are implemented with scoped results and terminal streams.
5. Settings and connections: provider authentication, editable preferences,
   simultaneous environments and persisted startup/theme settings remain. Config
   updates, provider status and theme controls are implemented.
6. Visual acceptance: capture the original desktop and GPUI app at matching sizes,
   compare spacing/typography, and verify resize, focus, menus, scrolling and
   keyboard behavior. Exercise working, idle, failed, approval and empty states.

## Source references

Inspected the upstream checkout at `792c7dd1`, dated 2026-09-30. Relevant sources:

- [Orchestration contracts](https://github.com/pingdotgg/t3code/blob/792c7dd1/packages/contracts/src/orchestration.ts)
- [Server/provider model contracts](https://github.com/pingdotgg/t3code/blob/792c7dd1/packages/contracts/src/server.ts)
- [Pending request reducer](https://github.com/pingdotgg/t3code/blob/792c7dd1/packages/client-runtime/src/pendingRequests.ts)
- [Environment authentication](https://github.com/pingdotgg/t3code/blob/792c7dd1/docs/internals/environment-auth.md)
- [Desktop backend configuration](https://github.com/pingdotgg/t3code/blob/792c7dd1/apps/desktop/src/backend/DesktopBackendConfiguration.ts)

## Validation for this pass

- Workspace build and type check passed.
- 82 unit/interaction tests and one documentation test passed. They include
  approval/question ordering, upload retry/removal, stale folder/workspace responses,
  terminal command/stream/reconnect behavior, disk drafts and environment isolation.
- A 720x480 native headless window verifies Ctrl+J collapses the sidebar, Ctrl+,
  opens Settings, Escape closes it, and typing returns to the composer.
- The read-only `session` example authenticated with the existing saved session,
  decoded 9 providers and 56 models, and synchronized 21 projects and 313 threads.
  The archive query decoded 1 project and 1 archived thread. Optional workspace
  checks decoded server folder browsing, file listing/read, Git status, branch
  listing and diff preview. It did not send turns, switch branches, launch terminals,
  upload files, or print credentials/message content.
- GPUI headless windows verify native clicks, typing, multi-question navigation,
  duplicate-submit prevention, retry, dismissal and text preservation during
  reconnect, plus rename menu navigation, error retry, offline guards, stale archive
  queries and duplicate restore prevention. These tests caught a same-update input callback race, fixed by
  reading controls before rebuilding the form.
- Screen-level visual inspection was unavailable because the Orca desktop
  runtime was not running. Pixel-level equivalence remains unverified.

See the [improvement audit](2026-10-01-improvement-audit.md) for reliability fixes,
verification and remaining improvements across the app.
