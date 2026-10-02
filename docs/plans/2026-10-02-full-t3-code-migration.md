# T3 Code feature migration to the Rust desktop client

Reviewed on 2026-10-02 against upstream commit `54084ae1e6c32809db040e4fa571c80fdf2d8ae4` and native baseline `f7c4677`. The repo-explorer cache at `C:/Users/Arafa/.explore/repos/pingdotgg__t3code` was clean and updated with `git pull --ff-only`. It advanced 46 commits from the previous cached revision, `792c7dd1`.

The target is desktop feature parity in our GPUI client. Keep the T3 server as the owner of agent processes, Git, terminals, files, credentials for providers, and shared settings. Port client behavior and UI to Rust; use the existing server APIs. Mobile-only behaviors need a desktop equivalent or an explicit disposition, listed below.

This plan supersedes the remaining-work lists in the October 1 parity and improvement plans. Those documents still explain earlier implementation decisions. Completion in this plan means verified user behavior, not merely the presence of an RPC wrapper.

## Sources and coverage

Use these pinned sources when implementing, then compare with newer upstream before each phase:

- [Settings routes and page layout](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/apps/web/src/routes/settings.tsx), [settings categories and search entries](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/apps/web/src/components/settings/settingsSearch.ts).
- [Client, server and project settings schemas](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/settings.ts).
- [RPC method registry](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts), [orchestration commands and streams](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts), [environment capabilities](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/environment.ts).
- [User guides](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/docs/README.md), `packages/client-runtime/src`, and the matching `apps/web/src/components` and `apps/desktop/src` implementations.

The [contract inventory](2026-10-02-upstream-parity-inventory.md) lists every entry in the two public RPC registries, the client/server settings fields, project overrides, storage policies and environment capabilities at this revision. Its Rust-reference column is a search aid, not proof of implementation. Provider-specific options and desktop IPC require the separate tasks below. Marketing, CI, release infrastructure and the mobile app itself are outside this desktop migration.

Recent upstream changes that the old plans missed include scratch threads without a project, creating a project from a name, a configurable Working section, updating providers across machines, restarting agent sessions after skill/plugin changes, composer undo grouping, and improved shortcut recording. Include these in their owning phases.

## Settings change implemented in this turn

- Settings occupies the main content area. The sidebar panel and its 40% height cap are removed.
- Appearance, Providers, Connections and Keyboard are separate sections. Wide windows use a category rail; narrow windows use wrapping tabs. Only existing functionality appears as working controls.
- Opening Settings preserves the current thread, drafts, attachments, Usage selection, workspace visibility preference and stream subscriptions. Workspace is hidden while Settings occupies the main area.
- Back, Escape and Ctrl+, return to the previous view. Selecting a thread or Usage leaves Settings. The settings page owns focus while open, preventing typing into a hidden composer.
- Provider refresh remains disabled offline. Cached provider rows are labeled offline. Theme selection and local-server/switch-server actions retain their existing behavior.

The Keyboard section is a reference for existing shortcuts. Editable bindings, full provider setup and scoped server settings remain migration tasks.

Verification: 57 native unit/interaction tests pass, including settings at 720x480 and 1400x900, offline refresh, retained drafts, category navigation, and return to thread, Usage, empty and pairing views. The native executable builds. These checks establish layout and interaction behavior; they do not establish screenshot equivalence with upstream.

## Feature inventory

"Partial" means a usable subset exists. "Missing" means the native client has no equivalent workflow identified in this review. Each row is assigned a phase so features do not disappear between plans.

| Feature group | Native state after this turn | Remaining work | Phase |
| --- | --- | --- | --- |
| Settings navigation | Full page implemented | Search, deep links, scoped targets, restore defaults, remaining categories | P2 |
| Protocol and live state | Partial | Full environment descriptors, capability guards, explicit resync on malformed required data, config event coverage | P1 |
| General preferences | Mostly missing | Default models/permissions/workspaces, streaming, follow-up behavior, confirmations, time format, restart continuation, background activity | P2 |
| Project settings and inheritance | Partial model defaults | Project/environment overrides, inherited and mixed values, reset overrides, grouped checkouts, project actions | P2, P6 |
| Appearance | Dark/light and favorites persist | System mode, themes, custom/VS Code import/export, environment themes, contrast, fonts, width, wrapping, diff colors, motion, glass | P2 |
| Keyboard, command palette and search | Fixed shortcuts exist | Editable bindings and conflict recording; thread/message/file/settings search; palette actions and navigation history | P3 |
| Thread organization | Partial | Snooze/wake, manual pinned/active order, grouping and sort, Working preference, auto-settle opt-out, undo, title regeneration | P3 |
| Archive | Searchable shelf and restore | Full archive page, history preview, delete/restore error recovery and cross-environment search | P3 |
| Provider setup | Readonly availability and model list | Install/update/remove, sign-in/out/cancel, accounts and instance configuration, custom models, health refresh | P4 |
| Provider choices | Model picker and favorite models | Advertised reasoning/service-tier options, remembered model traits, project defaults, account switching | P4 |
| Skills and provider commands | Missing | Slash/$ menus by provider and workspace, discovery refresh, compaction, restart session, loaded-session import | P4, P5 |
| Chat and composer | Partial | Queue/steer, queued-message controls, prompt recall/stash, undo grouping, rich context chips, large-paste handling | P5 |
| Transcript and turn actions | Streaming, markdown, reasoning and grouped tools exist | Edit/retry, conversation/file rewind, checkpoints, plan actions, context meter, citations, rich media, agent/subagent inspection | P5 |
| Approvals and questions | Basic approval/choice/free-text workflows exist | Question attachments, async timing, per-provider capabilities and pending-state recovery | P5 |
| Attachments | Signed uploads, retry/remove and download exist | Drag/drop/paste, validation/limits, conversion, inline previews, cross-environment references and expiry | P5 |
| Project creation | Server folder picker and basic creation | Name-only creation, scratch threads, clone/import/init/publish, clone progress/cancel/retry, icons, rename/remove | P6 |
| Files and editor integration | Server file tree and readonly previews | File/content search, write/save, rich previews, external files, tabs, editor launch, file/review context | P6 |
| Workspaces and worktrees | Status/diff and local branch switch exist | Required worktree bootstrap, create/remove refs/worktrees, setup actions, cancellation, origin/submodules, cleanup | P6 |
| Terminal | Command input and streamed output | ANSI terminal, input/resize/attach, alternate screen, selections/context, multiple terminals, history and clear/restart | P6 |
| Source control | Status, branches and text diffs | Pull/fetch/commit/push, generated commit/PR text, provider discovery, credentials, writer preferences, diff review | P7 |
| Pull requests and stacks | Missing | List/search/detail/checks/comments/reviews, linked threads, stacks, merge/rebase, viewed files, reviewer/label management | P7 |
| Usage | Single-server cost/token charts and breakdowns | Limits/accounts/reset credits, rate overrides, refresh pricing, pooled and multi-environment summaries, hub sources | P7 |
| Browser preview | Missing | Embedded browser, tabs/profiles, navigation/resize, automation host, annotations/element context, screenshots/recording, floating view | P8 |
| Device panel | Missing | Device hub onboarding/hosts, list/open/control/stream, tools, multiple/floating device tabs, supported 3D/fold controls | P8 |
| Connections | Pairing, reconnect and one active server exist | Connection catalog, concurrent environments, credentials, pairing/access revocation, local discovery, scopes and routing | P9 |
| Remote access | Direct server URL exists | DPoP, T3 Connect/relay, LAN exposure/Tailscale, SSH, WSL, load balancing, GitHub access sharing | P9 |
| Local backend | Selected executable starts with isolated data | Discovery/bundling, saved startup choice, autostart/service integration, lifecycle/logs, packaged runner | P9, P10 |
| Notifications and app lifecycle | Missing native notification workflow | Completion/question/approval notifications, badge, thread navigation, focus/background/power reporting and policies | P10 |
| SnapShots | Missing | Global capture shortcut, screenshots plus accessible app text, permissions, preview and attach, sound/flash preferences | P10 |
| Diagnostics, updates and licenses | Errors reach UI; no full diagnostics pages | Process/resource traces, logs, safe diagnostics export, provider/server/app update flows, channels and license page | P10 |
| Desktop delivery | Source builds | Installers/signing, app menus, URL/file handling, OS credential storage, release/update recovery, visual/accessibility/performance acceptance | P10 |

## Architecture and implementation rules

`t3-client` remains UI-independent. Add typed contract models and grouped API modules there. Keep orchestration projections, connection lifetimes, cancellation and reconnect handling testable without GPUI. Reuse server behavior rather than invoking local Git or provider CLIs for remote projects.

`backend.rs` currently routes a single session. Extend it to a connection catalog with environment-keyed commands/events before exposing concurrent environments. A response must carry the environment, project/thread scope and request ID needed to reject stale results. Drafts, uploads, terminal subscriptions and pending mutations must stay associated with their originating environment.

Keep device preferences in `prefs.rs`. Save server settings with `server.updateSettings`, and refresh from authoritative config events. Preserve absent versus explicit null versus a stored value for project inheritance. Show pending and failed saves; do not imply that disconnected environments received bulk changes. Keep server-owned provider secrets on the server.

Give pages explicit navigation state as their number grows. At present, Settings temporarily covers the remembered thread/Usage view. Add typed destinations and back history when search, project pages and PR pages arrive. Do not replace retained thread entities merely to visit a settings category.

Translate web/desktop semantics to native controls. An Electron IPC call is not an RPC endpoint: browser embedding, capture, OS login callbacks, notifications, SSH/WSL management and app updates need native implementations or an owned helper. Preserve platform support limits in the UI.

## Ordered implementation phases

Phases describe a recommended delivery order, not estimated calendar dates. Each task below should become a small implementation change with an observable acceptance case. P8 can proceed after P1 and P5; P9 should land before cross-environment variants of P2, P3, P4 and P7 are declared complete.

### P1. Contract compatibility

1. Decode environment version/capabilities, complete provider/model metadata and all config event variants. Add per-feature guards using advertised capabilities rather than client version checks.
2. Separate unknown additive events from malformed required snapshots or sequence gaps. Resubscribe/resync explicitly; retain drafts and pending work while recovering.
3. Build sanitized fixtures from this upstream revision and one older supported revision. Cover shell/thread snapshots, config changes, authorization failures and subscription cancellation.

Acceptance: newer unknown fields remain readable; malformed required state cannot leave silently stale controls; unsupported actions stay disabled. Existing transport and draft recovery tests still pass.

### P2. Scoped settings and appearance

1. Add settings models, get/update wrappers, pending/save/error state and a device-preference migration. Expand the full page with General, Project, Appearance, Providers, Integrations, Source Control, Storage, Connections, Archive, Keybindings and SnapShots as their functionality lands. Diagnostics and licenses are secondary pages.
2. Add settings search and remembered project/environment/checkout targets. Implement built-in/environment/project/`t3.json` inheritance, reset, mixed values and partial bulk-save results. Initially support the active environment; complete multi-environment selection after P9.
3. Implement General and appearance fields from the contract inventory. Connect each control to actual behavior, including system theme changes, fonts, wrapping, streaming preferences, confirmations and motion. Add import/export/custom themes and environment themes.

Acceptance: device preferences survive relaunch without writing server settings. Project overrides survive reconnect and reset correctly. Unknown settings are preserved when editing known fields. Restore defaults affects the chosen scope. Every inventory field has an implemented control or documented platform disposition.

### P3. Navigation, keyboard and thread management

1. Add a command registry, searchable palette and configurable bindings with shortcut recording, conflict reporting and platform modifier conventions. Use the same commands for menus and keyboard actions.
2. Add server thread/message search and project file search, scoped navigation history, archived history and reference copying. Avoid downloading every transcript to search.
3. Add snooze/unsnooze, pinned/active ordering, configurable Working shelf, grouping/sorting, auto-settle opt-out and title regeneration. Add reversible undo notifications for thread state actions, including prior order and reopening archived work.

Acceptance: keyboard-only navigation works at 720x480; settings and modals restore appropriate focus. Snoozed and ordered threads match a second upstream client. Search results open the correct environment/thread and preserve the draft.

### P4. Provider setup and model capabilities

1. Add provider instance forms for Codex, Claude, Cursor, Grok, OpenCode and Antigravity using their settings schemas. Support enabled state, binary/home paths, launch options, custom models and provider-specific configuration.
2. Implement install/update/remove and streaming sign-in flows with respond/complete/cancel/logout. Include managed ChatGPT setup, multiple Codex accounts, import/reconnect profiles and account switching where advertised.
3. Port model option selectors and remembered choices. Add per-workspace commands/skills discovery, explicit refresh, session restart and agent-session scan/import. Keep plugin/MCP discovery provider-owned; do not invent a separate plugin store.

Acceptance: authenticate on the environment that owns the provider. Cancellation stops the owned setup flow. Models/options reflect advertised capabilities, and restarting loads changed skills without deleting conversation history.

### P5. Complete the conversation workflow

1. Add queue/steer behavior, send-queued-now/cancel, prompt recall/stash, provider slash commands, compaction and provider-aware follow-up controls. Port rich context serialization and message/paste limits.
2. Add edit/retry and checkpoint/conversation rewind using native message and turn IDs. Offer file restore only when the server permits it. Preserve unsent drafts and recover rejected rewinds.
3. Add plan cards and follow-ups, context usage, citations, richer attachment/media previews, drag/drop/paste, async questions with attachments and agent/subagent inspection. Preserve virtualization and independent tool expanders.

Acceptance: queued messages belong to their originating thread; stop returns queued work to the composer; stale responses cannot erase newer drafts. Rewind refusal leaves files/conversation intact. Provider support differences are tested explicitly.

### P6. Projects, files, worktrees and terminal

1. Add name-only project creation, scratch threads, existing-session/project import, clone/init/publish workflows and clone progress/cancel/retry. Add metadata/icons/checkouts, rename/remove and project actions.
2. Implement required worktrees, branch/ref creation/removal, setup scripts, origin selection and submodule policy. Subscribe to setup progress and allow cancellation. Expose safe worktree/storage cleanup through server settings.
3. Add file/content search, save/write, preview tabs and media/document viewers, external file handling and remote editor launch. Use server URLs and RPC rather than local filesystem assumptions.
4. Replace command output with a terminal emulator backed by a Rust terminal parser. Support attach/write/resize/clear/restart, ANSI colors, alternate screen, multiple terminals, history and terminal selection context.

Acceptance: workflows operate against a remote environment whose paths do not exist locally. A required worktree failure does not fall back to editing the project checkout. An interactive shell and full-screen terminal program survive resize and reconnect without duplicate input.

### P7. Source control, pull requests and usage

1. Complete Git workflow actions, generated commit/PR text and source-control settings. Port split/stacked diffs, whitespace filtering, file/turn diff review and review context.
2. Add PR workspace and linked-review panels, checks, comments/replies/resolution/reactions, reviews, reviewers/labels, viewed-file tracking and host-supported actions. Support GitHub, GitLab, Forgejo/Gitea, Bitbucket and Azure DevOps according to server capability.
3. Implement multi-link and stack state with the upstream compatibility rules: multi-link when `threadPullRequests` is true; legacy metadata linking when only `threadPullRequestLinking` is true; no linking action when neither is advertised. Add merge/rebase confirmations and uncertain-result recovery.
4. Complete Usage rates/overrides, reset-credit redemption and quota-source editing. The native Cost/Tokens/Limits tabs now show subscription limits, deduplicated native/CLIProxyAPI accounts, pooled remaining quotas, reset countdowns, pace, reset-credit balances and probe errors. Complete pooled history and multi-environment summaries after P9.

Acceptance: the server owns host credentials and mutations. Never retry a mutation on another environment automatically when its result is uncertain. New and old linking formats work with their corresponding fixtures. Usage rates and failed partial saves agree with upstream behavior.

### P8. Browser and device integrations

1. Prove embedded-browser hosting in a small native implementation before committing to a library. Compare WebView2 on Windows and platform equivalents against required navigation, profile isolation, automation, screenshots/recording and remote control. A page displayed in a webview alone is insufficient.
2. Implement preview tabs, profiles/import, viewport/appearance, annotations and element context, recording, floating view and the `previewAutomation` request/response host. Apply browser-access preferences only at their documented scope.
3. Implement device hub configuration/host checks, list/open/close/shutdown/actions, visible-stream ownership, interactive tools and floating/multiple tabs. Add 3D/fold controls where supported after basic viewing/control works.

Acceptance: user and agent control the same preview/device. Background tabs stop unnecessary streaming. Closing a device tab stops watching without silently shutting down the device. On Windows, iOS requires a supported macOS host; unsupported local platforms get an accurate explanation.

### P9. Concurrent and remote environments

1. Add connection catalog and independent session ownership, OS-backed credential storage, server discovery, access/pairing/revocation and protocol-specific DPoP/refresh handling. Migrate existing saved credentials without losing drafts.
2. Add local runner discovery/bundling/autostart, LAN and Tailscale exposure, desktop-managed SSH/WSL, T3 Connect/relay and reconnect diagnostics. Reuse upstream auth/connection rules rather than changing the server protocol.
3. Add project grouping across checkouts, all-environment settings/search/usage/provider updates, new-thread load balancing and opt-in GitHub routing. Route by explicit environment identity and capability.

Acceptance: two environments with identical thread IDs never share drafts, uploads or pending requests. One offline server does not block another. Endpoint replacement invalidates trust as upstream requires. Failed bulk changes can retry only failed destinations.

### P10. Native desktop integration and release acceptance

1. Add notifications/badges and deep-link navigation, activity/power reporting, background-policy controls, app menus and quit handling.
2. Add SnapShots through platform capture/accessibility APIs with explicit permission state, global shortcuts and attachment previews. Port feedback/export and diagnostics/process/resource views with appropriate access checks.
3. Implement server/provider/app update flows, progress/restart/channel handling, startup/log recovery, packaging/signing and license notices. Respect server-advertised update methods; desktop-managed updates need our own native supervisor.
4. Run an integrated acceptance pass against the pinned upstream at desktop and minimum sizes, light/dark themes, slow/disconnected environments and long histories. Measure startup, stream rendering, scroll latency, idle CPU and retained memory. Add focused CI checks for supported platforms.

Acceptance: installed builds launch a compatible backend, reconnect after authorized updates and retain work. Exported diagnostics omit credentials. Compare screenshots and keyboard/screen-reader behavior before claiming visual or accessibility parity.

## Platform-specific disposition

| Upstream behavior | Rust desktop treatment |
| --- | --- |
| Electron IPC and renderer | Implement equivalent native services and GPUI UI; retain the server contract |
| iPhone on-device voice input | Mobile implementation remains outside scope; desktop dictation needs its own OS-specific task if included |
| Mobile offline attachment copies/queue | Preserve desktop upstream semantics first; durable offline desktop queues require a separate deliberate design |
| iOS Live Activities and Android home widgets | Keep mobile behavior in upstream mobile; provide desktop notifications/badges rather than claiming widget parity |
| Material You and mobile navigation | Native desktop appearance/navigation equivalents; no Android-specific layout port |
| Browser cookie import | Match supported OS/browser combinations and keyring permission flows; do not promise import for unsupported Windows app-bound encryption |
| SnapShots and device platform requirements | Implement per-platform support and permission recovery; retain supported remote-host paths |
| CLI/server installation, service and relay infrastructure | Reuse the T3 runtime; expose supported setup/control paths in the desktop app |
| Legacy feature switches | Track schema entries; migrate their behavior when meaningful, document switches whose old web implementation has no native equivalent |

## Completion tracking

Only the settings shell described above is delivered in this turn. P1 through P10 remain planned. Start with P1 and P2, then P3/P4/P5/P6. Browser hosting and remote authentication are the largest integration uncertainties; resolve their feasibility before promising dates.

For every inventory row, record the implementing change, verified behavior and any platform limitation. Require a focused contract/reducer test for stateful behavior and an integrated native interaction check for user-facing workflows. Do not add tests that merely repeat props or method names.

Refresh upstream in the repo-explorer cache before each phase, pin its new commit, and compare changed contracts/routes against the inventory. Add newly discovered features to an owning phase. Desktop parity is complete only when the feature rows and contract inventory have dispositions, older supported servers still work, and integrated acceptance passes.
