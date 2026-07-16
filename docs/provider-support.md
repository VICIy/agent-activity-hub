# Provider support matrix

Status as of 2026-07-16. A capability is not promoted above the evidence available in this repository.

| Provider | macOS | Windows | Highest verified input | Approval certainty | Control |
|---|---|---|---|---|---|
| Codex | Implemented and locally exercised | Source is cross-platform; CI coverage enabled | Native Hook plus session-log compensation | Explicit | Detect, install, repair, uninstall, observe |
| Claude Code | Hook and session-log adapters implemented | Cross-platform Hook source; host verification pending | Native Hook source and local CLI configuration | Explicit Hook events plus structured rejection compensation | Detect, install, repair, uninstall, observe |
| Qoder | Hook configuration and legacy migration locally verified | Cross-platform source; host verification pending | Native Hook source and redacted fixtures | Explicit only for received Hook payloads | Detect, install, repair, uninstall, observe |

Process-only detection never produces approval events. Approval state requires an
explicit Hook payload, while structured session-log compensation is limited to
observable rejection/abort outcomes. Windows bundle behavior remains source/CI
evidence until it is exercised on a clean host.
