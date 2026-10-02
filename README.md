# Rust code

A native desktop client for [T3 Code](https://github.com/pingdotgg/t3code), built with
[GPUI](https://gpui.rs) and [GPUI Kit](https://gpui-kit.com). The T3 server (`npx t3`) stays the
backend: it owns the agents, git, terminals and state. This app is another client of it, like the
web, desktop and mobile apps.

```
crates/
├── t3-client/   Rust client for the T3 server: pairing, Effect RPC over WebSocket, typed
│                shell/thread streams and reducers. No UI dependencies.
└── t3-gpui/     The GPUI Kit app: sidebar of projects/threads, streaming transcript, composer.
```

## Prerequisites

- Rust, latest stable (GPUI tracks new releases): <https://rustup.rs>
- Windows: Visual Studio Build Tools with the "Desktop development with C++" workload
  (MSVC + Windows SDK). macOS: Xcode. Linux: see the GPUI Kit getting-started page.
- Node 22+ for the T3 server.

## Run

1. Start a T3 server and get a pairing link:

   ```sh
   npx t3 serve                     # prints http://<host>:3773/pair#token=...
   # or, with a server already running:
   npx t3 auth pairing create
   ```

2. Start the app and paste either the pairing link or just the token:

   ```sh
   cargo run -p t3-gpui
   ```

   A bare token (what `npx t3 auth pairing create` prints) connects to `http://localhost:3773`. For
   another server, enter `<server url> <token>`, for example `http://192.168.1.20:3773 7NB9KZSDLQLW`.
   Credentials are saved under your config directory (`t3-gpui/credentials.json`), so later
   launches reconnect without pairing again. Use **Switch server** in the sidebar to pair with a
   different server.

For the smoothest scrolling and animation, run an optimized build:

```sh
cargo run --release -p t3-gpui
```

`cargo run` builds dependencies (GPUI) with `opt-level = 2` so debug builds stay usable, but
only `--release` optimizes the app itself and enables thin LTO.

Headless check of the protocol layer, with no UI:

```sh
cargo run -p t3-client --example shell -- "http://localhost:3773/pair#token=..."
cargo test -p t3-client
# Check an existing saved session without pairing or changing server state:
cargo run -p t3-client --example session -- <path-to-credentials.json>
```

## How it talks to the server

| Step                                | Endpoint                                                                           |
| ----------------------------------- | ---------------------------------------------------------------------------------- |
| Pair (one-time link → bearer token) | `POST /oauth/token` (token exchange, form-encoded)                                 |
| Socket ticket                       | `POST /api/auth/websocket-ticket`                                                  |
| RPC                                 | `GET /ws?wsTicket=…`, Effect RPC with JSON messages                                |
| Projects/threads                    | `orchestration.subscribeShell` (stream)                                            |
| Archived threads                    | `orchestration.getArchivedShellSnapshot` (on demand)                               |
| Thread detail                       | `orchestration.subscribeThread` (stream, full history)                             |
| Send / stop                         | `orchestration.dispatchCommand` with `thread.turn.start` / `thread.turn.interrupt` |

The types in `t3-client/src/types.rs` are a hand-ported subset of
`packages/contracts/src/orchestration.ts`. Unknown fields are ignored, and stream items that fail
to decode are skipped, so newer servers keep working until a change touches a field we use.

## Native features

The app supports model selection, runtime/Build/Plan modes, project/thread creation,
approval responses, user-input questions, and pin/settle/archive/restore/rename actions.
Archived threads have a searchable shelf with refresh and restore controls. New
threads inherit current modes and use destination-project model defaults first.

Composer text and question answers persist across restarts under the server URL
and environment ID. File attachments upload through signed server URLs, with
retry/removal controls and per-message download links. Selected local attachment
files are held in memory and must be selected again after restarting the app.

Open **Workspace** beside Settings or press Ctrl+J for server-side files, readonly
previews, Git status/diffs, local branch switching, and a command terminal with
streamed output. Project creation browses folders on the server. Ctrl+N creates a
thread, Ctrl+B toggles the sidebar, Ctrl+L focuses the composer, and Ctrl+, opens
Settings. Question cards support Ctrl+1 through Ctrl+9 for choices.

The sidebar's **Usage** button opens Cost, Tokens and Limits tabs. Cost and Tokens
offer 7/30/90-day history. Limits shows pooled subscription quota, account bars,
reset countdowns, pace and reported reset credits from provider instances and
configured quota sources. It refreshes every five minutes while visible. Failed
refreshes retain the last report; reset-credit redemption and multi-environment
usage remain pending.

The app embeds Geist for interface text, Geist Mono for code and terminals, and
SVG provider logos. No system font installation is required. Asset sources and
font licensing are in [assets/README.md](crates/t3-gpui/assets/README.md).

Settings shows provider availability and model counts, refreshes provider status,
and switches dark/light appearance. **Local server** lets you select a compatible
T3 server executable: the app starts it with a private stdin bootstrap, a separate
`t3-gpui/server` data directory, and in-memory credentials. No pasted pairing link
is needed for that session. Server discovery, bundling and automatic local startup
on the next app launch are still pending.

The compact desktop UI groups tool calls between messages, with expandable per-call
details and icon-only copy controls. Settings opens as a full page in the main area,
with Appearance, Providers, Connections and Keyboard sections. Back, Escape or
Ctrl+, returns to the previous view and preserves the current draft. Categories
use a rail on wide windows and tabs on narrow windows.
See the [compact UI changes](docs/plans/2026-10-01-compact-rust-code-ui.md).

## Remaining parity work

The [full migration plan](docs/plans/2026-10-02-full-t3-code-migration.md) compares
the native app with upstream T3 Code at `54084ae1` and orders the remaining work
into ten phases. Its [contract inventory](docs/plans/2026-10-02-upstream-parity-inventory.md)
tracks all 153 public RPC methods and the settings/capability fields at that revision.

Slash commands, provider-specific model options, message edit/retry and plan actions,
worktree creation, full terminal emulation, provider authentication/preferences,
multiple simultaneous environments, DPoP tokens, and relay/T3 Connect remain.
Pixel equivalence with the original desktop has not been verified.

See the [UI parity and local-backend plan](docs/plans/2026-10-01-ui-parity-and-local-backend.md)
for implementation details and the remaining local-server work.

The [improvement audit](docs/plans/2026-10-01-improvement-audit.md) records the remaining
work across UI, workspace, connections, performance and delivery. Failed pairing
keeps the current thread and drafts; sends wait for fresh thread details.
