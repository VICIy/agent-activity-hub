---
name: agent-activity-hub-install
description: Install, update, repair, or launch the public macOS Agent Activity Hub desktop app from GitHub. Use when a user asks an AI to set up Agent Activity Hub, its Tauri traffic-light window, or the Codex/Claude Code/Qoder activity integrations on a Mac.
---

# Install Agent Activity Hub

Use the bundled `scripts/install_agent_activity_hub.sh` script to install the
public repository `https://github.com/VICIy/agent-activity-hub` on macOS. The
script prefers a published GitHub Release and falls back to a local Tauri
build when no compatible release asset is available.

## Workflow

1. Confirm that the user wants to install or update the app. Explain that a
   source fallback may download npm and Rust dependencies and may take several
   minutes.
2. Run the installer from this skill directory:

   ```bash
   ./scripts/install_agent_activity_hub.sh
   ```

   Use `--dry-run` to inspect the selected path first. Use `--no-launch` when
   the user only wants the app copied. Use `--app-path /path/to/App.app` to
   install a locally built bundle. Use `--repo` and `--ref` only when the user
   explicitly requests a fork or branch.
3. Report the final application path and whether launch succeeded. The script
   backs up an existing app before replacing it; it does not remove user data
   or provider configuration.
4. After the app opens, guide the user to the Tauri control panel's
   **Adapters** page, choose **Detect**, then install or repair the Codex,
   Claude Code, and Qoder adapters they use. The adapters write only their
   managed hook entries and preserve other settings. Do not tell the user to
   configure the obsolete HTTP ports `8765` or `8766`; the production app uses
   its bundled local IPC Hook Helper.

## Installation Modes

- **Release (default):** Download the first non-draft GitHub release asset
  ending in `.dmg`, `.app.zip`, or `.zip`, mount/extract it, and copy the app.
- **Source fallback:** Clone the requested ref, run `npm ci` in
  `apps/agent-activity-desktop`, and run
  `npm run tauri build -- --bundles app`. The resulting
  `Agent Activity Hub.app` is copied to the install directory.
- **Existing bundle:** `--app-path` skips network access and compilation.

The default destination is `/Applications`. If that location is not writable,
the script uses `~/Applications` and prints the actual path. Existing bundles
are moved to a timestamped backup under
`~/Library/Application Support/Agent Activity Hub/backups/` before replacement.

## Prerequisites for Source Fallback

The Mac must have `git`, Node.js/npm, Rust/cargo, and the Tauri macOS build
toolchain (Xcode Command Line Tools or Xcode). The installer checks these
before cloning. It never installs system packages or invokes `sudo`; report a
missing prerequisite and let the user decide how to install it.

## Troubleshooting

- If GitHub cannot be reached, retry with a local `--app-path`, or ask the user
  for a network-enabled environment. Do not silently use an untrusted mirror.
- If the app is blocked by macOS Gatekeeper, explain that the locally built
  bundle is unsigned and ask the user to open it from Finder or approve it in
  **System Settings > Privacy & Security**. Do not disable Gatekeeper.
- If the app launches but no agent activity appears, use the **Adapters** page
  to detect and install/repair hooks, then start a new provider session. The
  app's floating window is the primary status output and does not depend on a
  browser page or an HTTP status endpoint.
- Use `--no-launch` and inspect the copied `.app` when diagnosing packaging
  problems. Use `--dry-run` to verify arguments without changing the system.

## Script Contract

Supported options:

```text
--repo URL          Public Git repository (default: VICIy/agent-activity-hub)
--ref REF          Branch or tag for source fallback (default: main)
--install-dir DIR  Destination directory (default: /Applications)
--app-path PATH    Install an existing .app bundle instead of downloading/building
--skip-build       Do not build from source if no release/app bundle is found
--no-launch        Copy the app without opening it
--dry-run          Print the plan without downloading, building, or changing files
--help             Show usage
```

