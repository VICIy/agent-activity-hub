# Implementation status

This file tracks the repository against the phases in `多智能体桌面状态中心一体化实施方案.md`. It is intentionally evidence-based: source availability is not treated as platform or hardware verification.

| Phase | Status | Delivered | Remaining external or follow-on work |
|---|---|---|---|
| 0 Compatibility spike | Partial | Provider matrix, Codex and Qoder fixtures, macOS/Windows IPC source, live macOS Tauri window/tray verification | Real Claude fixtures, clean Windows host verification, and optional hardware evidence |
| 1 Protocol and state core | Implemented for first slice | Versioned event/command schemas, strict validation, instance-scoped bounded dedupe, sequence/time ordering guards, per-session reducer, global arbiter, scheduled leases, SQLite event ring and restart-safe snapshots | Richer cross-source reconciliation and 24-hour soak metrics |
| 2 IPC, helper and Codex | Implemented for first slice | Unix socket/Named Pipe handshake, business acknowledgement checks, strict helper timeouts, bounded and periodically drained spool, Codex mapping, session-log rejection compensation, managed Hook configuration | Broader host-version compatibility and long-running fault-injection evidence |
| 3 Desktop | Implemented for first slice | Tauri 2 shell, tray, close-to-hide, hidden background/autostart mode, separate transparent light window, overview/sessions/adapters/diagnostics/settings views, configurable light mapping, and dismissible terminal sessions | Multi-display position persistence, OS notifications, and signed bundle smoke tests |
| 4 Effective Work control plane | Not started | Command types and JSON Schema only | Requires the Effective Work backend repository, identity model, database and SSE infrastructure |
| 5 Claude and Qoder | Implemented for first slice | Shared Hook Helper mappings, managed install/repair/uninstall UI, Claude/Qoder provider isolation, rejection compensation, Qoder legacy-wrapper migration, compact/notification/subagent event coverage, and redacted Qoder fixtures | Real Claude fixture capture and clean Windows host verification |
| 6 Devices | Optional follow-on | Desktop traffic light and saved LED effects work without external hardware | Direct Serial/BLE ownership, reconnect testing, and hardware evidence if physical output is reintroduced |
| 7 Distribution | Partial | Target-aware Hook Helper sidecar, successful macOS `.app` bundle, dynamic development ports, and dual-platform CI source | Developer ID/Windows signing credentials, notarization, clean Windows bundle smoke test, and 24-hour trial |

## First vertical slice definition

- Standard events are validated before reduction.
- Approval state is isolated by provider, instance, session and correlation.
- Older sequence/timestamp events cannot roll a session back, and a lower-confidence recovery event cannot clear a higher-confidence approval wait.
- A working event in another session cannot overwrite a waiting session.
- Matching approval/tool completion clears the wait immediately.
- Duplicate inputs do not produce another state revision or output event.
- Unverified approval state is cleared on restart rather than restoring a false yellow light.
- The Hook Helper always exits successfully and spools when IPC is unavailable.
- An event and its engine snapshot commit in one SQLite transaction; rejected deliveries remain eligible for spool retry.
- Desktop and floating outputs consume only `StateSnapshot`.
