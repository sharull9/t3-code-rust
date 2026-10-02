# Improvement audit

Reviewed the native UI modules, client projections, authentication, RPC transport,
backend lifecycle and existing parity plan. This is a source and interaction-test
review. Matching desktop screenshots and performance measurements still need a
separate acceptance pass.

## Fixed in this pass

| Problem | Change | Evidence |
| --- | --- | --- |
| Failed server switching deleted the open thread, composer drafts and question answers before authentication succeeded | Clear the environment only after successful pairing. Keep the form and error on failure, prevent duplicate pairing, and bound HTTP/handshake waits | Native recovery test preserves the open draft and other thread drafts, rejects duplicate pairing and clears state after success |
| Dropping an RPC connection left reader/writer/heartbeat tasks and sockets running | One owned transport task. Active subscriptions retain it; dropping the last owner aborts it | Local WebSocket test observes socket closure after the final subscription drops |
| Cancelled and unanswered unary RPCs retained pending entries indefinitely | Interrupt and remove cancelled requests; default 60-second deadline with a configurable override | Local WebSocket tests verify the Interrupt message and empty pending map for cancellation and timeout |
| A dead writer or silent server could leave the connection apparently live | Reader, writer and heartbeat share a lifetime. A 30-second receive deadline reports a disconnect | Disconnect regression test verifies pending callers fail and new requests are rejected. The silent-server deadline is implemented but not separately simulated |
| Shell subscriptions could stop while the UI still reported Connected | A terminated shell stream ends the backend session and starts reconnection | Source/type verification; live saved-session health check |
| Detached mutations could outlive a backend session | Track operations in a session-owned JoinSet; session exit aborts unfinished requests and releases completed tasks | Source/type verification and transport cancellation tests. Cancellation does not undo commands already accepted by the server |
| A failed thread detail stream left stale controls enabled | Disable the affected view. Selecting it again restarts its detail subscription while retaining the composer | Native recovery test verifies readiness resets, retry dispatch and retained text |
| Sending was possible before fresh detail identified pending questions or approvals | Wait for thread detail before enabling send and settings. Disconnection releases local pending-send/settings flags | Native composer test verifies loading/offline guards, duplicate-send prevention, rejected draft retention and preservation of edits made during an accepted send |
| Send/stop controls were generic clickable containers | Use native buttons with tooltips and disabled state; give the composer an accessibility label | Native click and text-input tests |
| Offline thread creation left an unresolved auto-open waiter | Emit creation results and clear the matching failed waiter | Offline command regression test |

## Remaining improvements by area

Priority 1 protects user work or fixes an incomplete basic workflow. Priority 2
closes feature parity gaps. Priority 3 needs measurement or cleanup before changing
behavior.

| Area | Priority | Findings and concrete next work |
| --- | --- | --- |
| Drafts and environment identity | 1 | Composer/question drafts now persist by normalized server URL and advertised environment ID, with debounced saves and release flushing. Environment switches restore their own drafts. Reconcile ambiguous acknowledgements after reconnect before encouraging a resend |
| Project creation and remote folders | 1 | Project creation now uses the server-side folder browser with stale-response guards. Add local shortcuts and directory creation if needed |
| Protocol consistency | 1 | Required stream decode failures are logged and skipped. Unknown event types can safely be ignored, but malformed snapshots and missed state updates should trigger an explicit resync/error. Add recorded server fixtures and generated contracts/version capability checks |
| Composer | 2 | Attachments, durable drafts and application shortcuts are implemented. Upload state/retry/removal and sent-file clearing have native tests. Slash commands and provider model options remain |
| Transcript and turn actions | 2 | Copy buttons and attachment download chips are implemented. Message edit/retry, plan interactions and inline image/document previews remain. Keep history, reasoning and tool grouping behavior; add actions tied to native message/turn IDs |
| Thread and project management | 2 | Snooze, manual ordering, archived detail previews and project editing/removal remain. The archive shelf uses explicit refresh rather than a live archived subscription. Verify model/runtime inheritance against each provider's capabilities |
| Workspace | 2 | Server files/previews, status/diffs, local branch switching and command terminals are implemented with scope/stale-result guards and streamed terminal output. Worktree/branch creation, editor integration and full ANSI/interactive terminal emulation remain |
| Providers and settings | 2 | Config updates stream live; Settings exposes provider availability, refresh and theme controls. Provider authentication, editable configuration and persistent preferences remain |
| Local backend without pasted pairing | 2 | Settings starts a selected T3 executable through private stdin bootstrap, with isolated data, owned shutdown, restart and token renewal. Executable discovery/bundling, saved startup choice, diagnostics/log display and a real-child acceptance test remain. See the local-backend plan |
| Keyboard and accessibility | 2 | Native send/stop and composer labeling improved. Audit sidebar cards, thought/tool expanders, focus restoration after modals, menus, offline controls and screen-reader labels. Add keyboard-only interaction tests |
| Visual acceptance | 2 | Full pixel equivalence has not been verified. Capture original and native desktop at matching sizes, including the 720x480 minimum; compare sidebar density, fonts, modal placement, empty/loading/failure states, long titles and scrolling |
| Performance and memory | 3 | Transcript rebuilding sorts history and derives pending requests on every applied item; the app caches a question panel per visited thread. Benchmark long streamed threads before making incremental indexes or cache eviction. Measure full-history startup, scroll latency and retained memory |
| Credentials and diagnostics | 2 | Credentials use a plain JSON file; saving failure only prints to stderr. Use an OS credential store, report persistence failures in the UI, and review URL/error/Debug output for secrets. Keep backend ownership and authentication distinct |
| Code quality and delivery | 3 | Strict Clippy fails on large error/enum variants. A secondary lint pass also reports nested conditions and an eight-argument helper. Box the large variants or introduce focused types, then add test/build/Clippy CI and desktop packaging/signing checks |

## Verification

- 82 unit/interaction tests and one documentation test pass.
- Native headless windows cover pairing recovery, thread retry, composer loading,
  archive/rename, approval/question reducers, upload retry/removal, folder selection,
  environment draft isolation, workspace scope/terminal reconnect, and keyboard
  navigation at the 720x480 minimum window size.
- Local WebSocket tests exercise real requests, interrupts, disconnects and socket
  ownership without mutating the user's server.
- Native executable build passes. Live readonly workspace calls decode folders,
  file listings/previews, Git status, branches and diffs against the connected server.
- Actual upload/send, terminal launch and managed child bootstrap still need a live
  mutation acceptance pass; current native interaction tests use isolated fixtures.
- Strict Clippy was run and its remaining findings are listed above. This pass
  does not claim a warning-free lint baseline or pixel parity.

Next priorities: visual comparisons at minimum/desktop sizes, live mutation acceptance
for attachments/terminals, then message/plan actions, worktree creation and provider
preferences. The current terminal is a command/output panel, not a terminal emulator.
