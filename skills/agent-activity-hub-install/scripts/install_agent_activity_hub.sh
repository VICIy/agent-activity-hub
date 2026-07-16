#!/usr/bin/env bash
# Install Agent Activity Hub from a GitHub release or by building the public source.
set -euo pipefail

REPO_URL="https://github.com/VICIy/agent-activity-hub.git"
REF="main"
INSTALL_DIR="/Applications"
APP_PATH=""
SKIP_BUILD=0
NO_LAUNCH=0
DRY_RUN=0

usage() {
  cat <<'EOF'
Install Agent Activity Hub on macOS.

Usage:
  install_agent_activity_hub.sh [options]

Options:
  --repo URL          Git repository (default: VICIy/agent-activity-hub)
  --ref REF           Branch or tag used for a source build (default: main)
  --install-dir DIR   Application directory (default: /Applications)
  --app-path PATH     Existing .app bundle; skip release lookup and build
  --skip-build        Fail instead of building if no release/app is available
  --no-launch         Install without opening the app
  --dry-run           Print the plan without changing files
  --help              Show this help
EOF
}

log() {
  printf '[agent-activity-hub] %s\n' "$*" >&2
}

warn() {
  printf '[agent-activity-hub] warning: %s\n' "$*" >&2
}

die() {
  printf '[agent-activity-hub] error: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a URL"
      REPO_URL="$2"
      shift 2
      ;;
    --ref)
      [[ $# -ge 2 ]] || die "--ref requires a branch or tag"
      REF="$2"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || die "--install-dir requires a directory"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --app-path)
      [[ $# -ge 2 ]] || die "--app-path requires an .app path"
      APP_PATH="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --no-launch)
      NO_LAUNCH=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (use --help)"
      ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "Agent Activity Hub currently supports macOS only"

if [[ "$DRY_RUN" == "1" ]]; then
  log "dry run: install directory: $INSTALL_DIR"
  if [[ -n "$APP_PATH" ]]; then
    log "dry run: install existing bundle: $APP_PATH"
  elif [[ "$SKIP_BUILD" == "1" ]]; then
    log "dry run: use a compatible GitHub release; do not build from source"
  else
    log "dry run: use a compatible GitHub release, then fall back to $REPO_URL at ref $REF"
  fi
  [[ "$NO_LAUNCH" == "1" ]] && log "dry run: do not launch the app"
  exit 0
fi

WORK_DIR=""
MOUNT_DIR=""
cleanup() {
  if [[ -n "$MOUNT_DIR" && -d "$MOUNT_DIR" ]]; then
    hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

github_repo_slug() {
  local slug
  slug="${REPO_URL#*github.com/}"
  slug="${slug%.git}"
  [[ "$slug" == */* && "$slug" != "$REPO_URL" ]] || return 1
  printf '%s' "$slug"
}

# Print two lines: asset name, then browser download URL.
find_release_asset() {
  local slug payload
  slug="$(github_repo_slug)" || return 1
  command -v curl >/dev/null 2>&1 || return 1
  command -v node >/dev/null 2>&1 || return 1
  payload="$(curl -fsSL --retry 2 -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/$slug/releases?per_page=20" 2>/dev/null)" || return 1
  printf '%s' "$payload" | node -e '
const fs = require("fs");
let input = "";
process.stdin.on("data", chunk => { input += chunk; });
process.stdin.on("end", () => {
  try {
    const releases = JSON.parse(input);
    const suffixes = [".dmg", ".app.zip", ".zip"];
    for (const release of releases) {
      if (release.draft || !Array.isArray(release.assets)) continue;
      for (const suffix of suffixes) {
        const asset = release.assets.find(item =>
          typeof item.name === "string" && item.name.toLowerCase().endsWith(suffix)
        );
        if (asset && asset.browser_download_url) {
          process.stdout.write(asset.name + "\n" + asset.browser_download_url);
          return;
        }
      }
    }
    process.exitCode = 1;
  } catch (_) {
    process.exitCode = 1;
  }
});
'
}

find_bundle_in() {
  local root="$1"
  find "$root" -type d -name '*.app' -print -quit 2>/dev/null
}

download_release_bundle() {
  local asset_name asset_url archive suffix extract_dir bundle release_dir
  if ! asset_info="$(find_release_asset)"; then
    return 1
  fi
  asset_name="${asset_info%%$'\n'*}"
  asset_url="${asset_info#*$'\n'}"
  [[ -n "$asset_name" && -n "$asset_url" ]] || return 1
  archive="$WORK_DIR/$asset_name"
  log "downloading release asset: $asset_name"
  curl -fL --retry 2 -o "$archive" "$asset_url"
  suffix="$(printf '%s' "$asset_name" | tr '[:upper:]' '[:lower:]')"
  if [[ "$suffix" == *.dmg ]]; then
    MOUNT_DIR="$WORK_DIR/mount"
    mkdir -p "$MOUNT_DIR"
    hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$archive" >/dev/null
    bundle="$(find_bundle_in "$MOUNT_DIR")"
    [[ -n "$bundle" ]] || return 1
    release_dir="$WORK_DIR/release"
    mkdir -p "$release_dir"
    ditto "$bundle" "$release_dir/Agent Activity Hub.app"
    hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
    MOUNT_DIR=""
    printf '%s' "$release_dir/Agent Activity Hub.app"
    return 0
  fi
  extract_dir="$WORK_DIR/extracted"
  mkdir -p "$extract_dir"
  ditto -x -k "$archive" "$extract_dir"
  bundle="$(find_bundle_in "$extract_dir")"
  [[ -n "$bundle" ]] || return 1
  printf '%s' "$bundle"
}

build_source_bundle() {
  local checkout bundle
  require_command git
  require_command npm
  require_command cargo
  require_command node
  require_command xcode-select
  xcode-select -p >/dev/null 2>&1 || die "Xcode Command Line Tools are required for a source build"
  node -e 'if (Number(process.versions.node.split(".")[0]) < 22) process.exit(1)' \
    || die "Node.js 22 or newer is required for a source build"
  WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/agent-activity-hub.XXXXXX")}"
  checkout="$WORK_DIR/repository"
  log "cloning $REPO_URL at ref $REF"
  git clone --depth 1 --branch "$REF" "$REPO_URL" "$checkout"
  log "installing frontend dependencies"
  (
    cd "$checkout/apps/agent-activity-desktop"
    npm ci
    log "building the Tauri macOS app"
    npm run tauri build -- --bundles app
  )
  bundle="$(find_bundle_in "$checkout/target/release/bundle/macos")"
  [[ -n "$bundle" ]] || die "source build completed without an Agent Activity Hub .app bundle"
  printf '%s' "$bundle"
}

[[ -z "$APP_PATH" || "$APP_PATH" == *.app ]] || die "--app-path must point to an .app bundle"
if [[ -n "$APP_PATH" ]]; then
  [[ -d "$APP_PATH" ]] || die "app bundle not found: $APP_PATH"
  APP_SOURCE="$APP_PATH"
  log "using existing app bundle"
else
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agent-activity-hub.XXXXXX")"
  if APP_SOURCE="$(download_release_bundle)"; then
    log "using the GitHub release bundle"
  elif [[ "$SKIP_BUILD" == "1" ]]; then
    die "no compatible GitHub release asset found and --skip-build was requested"
  else
    warn "no compatible GitHub release asset found; falling back to a source build"
    APP_SOURCE="$(build_source_bundle)"
  fi
fi

if [[ ! -d "$INSTALL_DIR" ]]; then
  mkdir -p "$INSTALL_DIR" 2>/dev/null || true
fi
if [[ ! -w "$INSTALL_DIR" ]]; then
  if [[ "$INSTALL_DIR" == "/Applications" ]]; then
    INSTALL_DIR="$HOME/Applications"
    mkdir -p "$INSTALL_DIR"
    log " /Applications is not writable; using $INSTALL_DIR"
  else
    die "install directory is not writable: $INSTALL_DIR"
  fi
fi

APP_NAME="Agent Activity Hub.app"
TARGET="$INSTALL_DIR/$APP_NAME"
if [[ -e "$TARGET" ]]; then
  BACKUP_DIR="$HOME/Library/Application Support/Agent Activity Hub/backups"
  mkdir -p "$BACKUP_DIR"
  BACKUP="$BACKUP_DIR/Agent Activity Hub $(date +%Y%m%d-%H%M%S).app"
  log "backing up existing app to $BACKUP"
  mv "$TARGET" "$BACKUP"
fi
log "installing to $TARGET"
ditto --rsrc --extattr "$APP_SOURCE" "$TARGET"

if [[ "$NO_LAUNCH" == "0" ]]; then
  log "launching Agent Activity Hub"
  open "$TARGET" || warn "the app was installed but could not be opened automatically"
fi

printf '\nInstalled: %s\n' "$TARGET"
if [[ "$NO_LAUNCH" == "0" ]]; then
  printf 'Next: open the Adapters page, Detect providers, and install/repair the hooks you use.\n'
fi
