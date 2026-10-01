# T3 Code GPUI

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

| Step | Endpoint |
| ---- | -------- |
| Pair (one-time link → bearer token) | `POST /oauth/token` (token exchange, form-encoded) |
| Socket ticket | `POST /api/auth/websocket-ticket` |
| RPC | `GET /ws?wsTicket=…`, Effect RPC with JSON messages |
| Projects/threads | `orchestration.subscribeShell` (stream) |
| Thread detail | `orchestration.subscribeThread` (stream, full history) |
| Send / stop | `orchestration.dispatchCommand` with `thread.turn.start` / `thread.turn.interrupt` |

The types in `t3-client/src/types.rs` are a hand-ported subset of
`packages/contracts/src/orchestration.ts`. Unknown fields are ignored, and stream items that fail
to decode are skipped, so newer servers keep working until a change touches a field we use.

## Not yet supported

Diffs, terminals, attachments, model options, multiple environments,
DPoP-bound tokens, and relay/T3 Connect. Model selection, runtime/Build/Plan modes,
project/thread creation, approval responses, user-input questions, and pin/settle/archive actions are supported.
Drafts survive thread switching and rejected sends within the current connection;
they are not yet saved across app restarts.
Question forms support single/multiple choices, custom answers and dismissal for
asynchronous questions. Answers survive rejected submissions, thread switching
and reconnects within the current connection.

See the [UI parity and local-backend plan](docs/plans/2026-10-01-ui-parity-and-local-backend.md)
for the remaining work and a GPUI-managed backend that avoids manual pairing.
