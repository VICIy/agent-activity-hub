# Agent Activity Hub

[English](README.md) | [简体中文](README-cn.md)

Agent Activity Hub is a local-first Tauri desktop application that combines
Codex, Claude Code, Qoder, and custom agent activity into a session-aware
traffic light. The floating light is the primary output, so the application
works without an external LED device.

```text
Agent hooks and session logs
  -> bundled Rust Hook Helper
  -> Unix Socket (macOS/Linux) or Named Pipe (Windows)
  -> per-session state reducer
  -> global priority arbiter
  -> Tauri control panel and floating traffic light
```

Production activity delivery does not require an HTTP port. The historical
Hook Hub on ports such as `8765` or `8766` is separate from the Tauri state
path and is not required by this application.

## Features

- Isolates sessions by `provider + instance_id + session_id`.
- Handles concurrent agents, projects, sessions, and custom providers.
- Shows Agent name, project name, status, and session details in the control
  panel and expandable floating panel.
- Uses the global priority
  `error > waiting approval > complete > working > idle`.
- Keeps approval and error states visible until a real provider event or an
  explicit dismiss action changes them.
- Expires only `complete` automatically, then returns that session to
  `idle`.
- Treats an empty or all-offline session set as global `idle`, so clearing
  the session list leaves every lamp off.
- Lets dismissed error, idle, or offline sessions reappear when a newer event
  arrives for the same session.
- Supports English and Simplified Chinese.
- Hides the main window on close and keeps the floating light available from
  the macOS tray menu.

## Traffic light

The lamp order is green, yellow, red. The default mapping is:

| State | Default effect | Automatic transition |
|---|---|---|
| Idle | All lamps off | None |
| Working | Green solid | Provider event controls the next state |
| Waiting approval | Yellow blink, 500 ms phase | No automatic idle transition |
| Complete | Green blink, 500 ms phase | Returns to idle after the completion lease |
| Error | Red blink, 500 ms phase | No automatic idle transition |
| Offline / sleeping | All lamps off | Kept only as per-session diagnostic state |

The Settings page can change:

- vertical or horizontal floating-light orientation;
- active lamp mask for each state;
- blink on/off and phase interval;
- global brightness;
- launch at login;
- interface language.

The floating panel uses a compact responsive layout. Active Agent chips wrap
onto additional rows, while expanded session cards show Agent and project
names on separate lines. Error and idle entries can be dismissed from the
floating panel; the control panel's full session list also allows offline
entries to be removed. Dismissal does not suppress future activity from that
session.

## Provider adapters

Open **Adapters** in the Tauri control panel to detect, install, repair, or
uninstall managed hooks.

| Provider | Configuration | Input path |
|---|---|---|
| Codex | `~/.codex/hooks.json` | Native hooks plus structured session-log compensation |
| Claude Code | `~/.claude/settings.json` | Native hooks plus structured session-log compensation |
| Qoder | `~/.qoder/settings.json` | Native hooks; repair removes the legacy `flash4-light.sh` wrapper |

Managed entries are marked with
`work.effective.agent-activity-hub/v1`. Installation preserves unrelated
hooks and top-level settings and writes a backup before replacing a provider
configuration. Codex matchers use the regular expression `.*`, allowing
permission requests and all other managed lifecycle events to reach Tauri.

The Hook Helper is bundled inside the application. End users do not need this
repository, Python, a private shell wrapper, or a fixed HTTP service.

Repository diagnostics are also available:

```bash
node tools/codex_hooks.mjs doctor
node tools/claude_hooks.mjs doctor
node tools/qoder_hooks.mjs doctor
```

## Development

Prerequisites:

- Rust 1.77 or newer;
- Node.js 22 or newer;
- npm 10 or newer;
- the platform prerequisites for Tauri 2.

```bash
cd apps/agent-activity-desktop
npm install
npm run tauri dev
```

Development starts searching for an available Vite address at
`127.0.0.1:1420`. When that port is occupied, the launcher selects the next
available port and passes the same URL to both Vite and Tauri.

## Production build

```bash
cd apps/agent-activity-desktop
npm run tauri build -- --bundles app
```

The build compiles the Rust Hook Helper for the active target, copies it into
Tauri's sidecar layout, builds the React frontend, and packages the desktop
application. The macOS application is created at:

```text
target/release/bundle/macos/Agent Activity Hub.app
```

Launch the packaged application with:

```bash
open -n "target/release/bundle/macos/Agent Activity Hub.app"
```

The macOS bundle includes the rounded application icon and the bundled
`agent-activity-hook` executable.

## Verification

Run the Rust and frontend test suites:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd apps/agent-activity-desktop
npm run test -- --run
npm run build
```

With the production Tauri app running, the lifecycle smoke tests send events
through the bundled Hook Helper and inspect the persisted Tauri state. They
cover multiple providers, projects, sessions, serial transitions, concurrent
status arbitration, approval yes/no, persistent errors, completion leases,
offline recovery, and final idle convergence.

```bash
tools/verify_multi_agent_lifecycle.zsh
tools/verify_concurrent_multistate.zsh
```

The scripts require `sqlite3` and `jq`. They refuse to overwrite unrelated
active workflows.

## Repository layout

```text
apps/agent-activity-desktop/       React UI and Tauri shell
native/agent-activity/             protocol, reducer, IPC, storage, Hook Helper
sdk/protocol-schema/               public JSON schemas
fixtures/agent_activity/           redacted provider payload fixtures
tools/                              launchers, hook maintenance, and verification
docs/                               provider and implementation status
```

Runtime data is stored in the platform-specific application data directory.
On macOS:

```text
~/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/
```

The directory contains the SQLite event/state store and the local IPC socket.
Provider payloads are normalized and sensitive tool input is not persisted.

## Generated-file cleanup

The repository ignores `target/`, `dist/`, `node_modules/`, local
databases, and logs. Use the protected cleanup command instead of deleting the
Rust target directory directly:

```bash
node tools/clean_generated.mjs --dry-run
node tools/clean_generated.mjs
```

The cleanup preserves the release application, bundled Hook Helper, sidecar,
and compatibility executables referenced by currently installed provider
hooks.

Further implementation details are available in
[docs/implementation-status.md](docs/implementation-status.md) and
[docs/provider-support.md](docs/provider-support.md).
