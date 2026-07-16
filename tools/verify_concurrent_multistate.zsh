#!/bin/zsh

# Production lifecycle test for the Tauri Activity Hub.
# It drives the real Hook Helper over IPC and reads the same persisted engine
# snapshot used by the floating traffic light. Test sessions are isolated by a
# unique run id; the pre-test snapshot is restored when the script exits.

set -euo pipefail

ROOT="${0:A:h:h}"
APP="${AGENT_ACTIVITY_APP_PATH:-$ROOT/target/release/bundle/macos/Agent Activity Hub.app}"
APP_BIN="$APP/Contents/MacOS/agent-activity"
HELPER="${AGENT_ACTIVITY_HOOK:-$APP/Contents/MacOS/agent-activity-hook}"
DB="${AGENT_ACTIVITY_DB:-$HOME/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/activity.db}"
SOCKET="${AGENT_ACTIVITY_SOCKET:-$HOME/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/ipc-v1.sock}"
RUN="multistate-e2e-$(date +%s)-$$"
BASELINE="$(mktemp -t agent-activity-baseline.XXXXXX)"
WAS_RUNNING=1
APP_PID="$(ps -axo pid=,command= 2>/dev/null | awk -v path="$APP_BIN" '$0 ~ path {print $1; exit}' || true)"

typeset -A PROVIDER INSTANCE SESSION PROJECT
for key in codex-1 codex-2 qoder-1 qoder-2 claude-1 claude-2 xxx-1 xxx-2; do
  PROVIDER[$key]="${key%%-*}"
  INSTANCE[$key]="${PROVIDER[$key]}-${RUN}-${key##*-}"
  SESSION[$key]="${RUN}-${key}"
  PROJECT[$key]="${RUN}-${key}"
done

die() {
  print -u2 "FAIL: $*"
  print -u2 "Test snapshot:"
  snapshot_json >&2 || true
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

snapshot_json() {
  local payload
  payload="$(sqlite3 -noheader -batch "$DB" 'SELECT payload FROM state_snapshots WHERE singleton = 1;' 2>/dev/null || true)"
  [[ -n "$payload" ]] || return 1
  print -r -- "$payload" | jq -e type >/dev/null 2>&1 || return 1
  print -r -- "$payload"
}

status_priority() {
  case "$1" in
    error) print 500 ;;
    waiting_approval) print 400 ;;
    complete) print 350 ;;
    working) print 300 ;;
    idle) print 100 ;;
    offline|sleeping) print 0 ;;
    *) print 0 ;;
  esac
}

status_for() {
  local key="$1"
  snapshot_json | jq -r --arg provider "${PROVIDER[$key]}" --arg instance "${INSTANCE[$key]}" --arg session "${SESSION[$key]}" '
    [.sessions[] | select(.key.provider == $provider and .key.instance_id == $instance and .key.session_id == $session) | .status][0] // "missing"
  '
}

reason_for() {
  local key="$1"
  snapshot_json | jq -r --arg provider "${PROVIDER[$key]}" --arg instance "${INSTANCE[$key]}" --arg session "${SESSION[$key]}" '
    [.sessions[] | select(.key.provider == $provider and .key.instance_id == $instance and .key.session_id == $session) | .reason][0] // "missing"
  '
}

global_status() {
  snapshot_json | jq -r '
    def priority:
      if . == "error" then 500
      elif . == "waiting_approval" then 400
      elif . == "complete" then 350
      elif . == "working" then 300
      elif . == "idle" then 100
      else 0 end;
    if (.sessions | length) == 0 then "idle"
    else (.sessions | max_by(.status | priority) | .status)
    end
  '
}

global_rank() {
  snapshot_json | jq -r '
    def priority:
      if . == "error" then 500
      elif . == "waiting_approval" then 400
      elif . == "complete" then 350
      elif . == "working" then 300
      elif . == "idle" then 100
      else 0 end;
    if (.sessions | length) == 0 then 100
    else ([.sessions[].status | priority] | max)
    end
  '
}

test_rank() {
  snapshot_json | jq -r --arg run "$RUN" '
    def priority:
      if . == "error" then 500
      elif . == "waiting_approval" then 400
      elif . == "complete" then 350
      elif . == "working" then 300
      elif . == "idle" then 100
      else 0 end;
    ([.sessions[] | select(.key.session_id | startswith($run)) | .status | priority] | max) // 100
  '
}

baseline_rank() {
  snapshot_json | jq -r --arg run "$RUN" '
    def priority:
      if . == "error" then 500
      elif . == "waiting_approval" then 400
      elif . == "complete" then 350
      elif . == "working" then 300
      elif . == "idle" then 100
      else 0 end;
    ([.sessions[] | select((.key.session_id | startswith($run)) | not) | .status | priority] | max) // 100
  '
}

wait_status() {
  local key="$1" expected="$2" actual="missing"
  for attempt in {1..50}; do
    actual="$(status_for "$key")"
    [[ "$actual" == "$expected" ]] && return 0
    sleep 0.1
  done
  die "$key expected status=$expected, got $actual"
}

assert_reason_contains() {
  local key="$1" expected="$2" actual
  actual="$(reason_for "$key")"
  [[ "$actual" == *"$expected"* ]] || die "$key expected reason containing '$expected', got '$actual'"
}

assert_target_rank() {
  local label="$1" expected="$2" actual
  actual="$(test_rank)"
  local expected_rank
  expected_rank="$(status_priority "$expected")"
  [[ "$actual" == "$expected_rank" ]] || die "$label expected test priority=$expected_rank, got $actual"
}

assert_overall_priority() {
  local label="$1" expected="$2"
  local test expected_rank base actual
  test="$(status_priority "$expected")"
  base="$(baseline_rank)"
  if (( test > base )); then
    expected_rank="$test"
  else
    expected_rank="$base"
  fi
  actual="$(global_rank)"
  [[ "$actual" == "$expected_rank" ]] || die "$label expected overall priority=$expected_rank (baseline=$base, test=$test), got $actual / $(global_status)"
  print "PASS $label: overall=$(global_status), test=$expected, baseline_rank=$base"
}

assert_all() {
  local expected="$1"
  for key in "${(@k)SESSION}"; do
    wait_status "$key" "$expected"
  done
}

emit() {
  local key="$1" kind="$2" correlation="$3"
  "$HELPER" emit \
    --provider "${PROVIDER[$key]}" \
    --instance "${INSTANCE[$key]}" \
    --session "${SESSION[$key]}" \
    --kind "$kind" \
    --correlation "$correlation" \
    --project "/tmp/${PROJECT[$key]}" >/dev/null
}

deduplicated_events() {
  snapshot_json | jq -r '.deduplicated_events'
}

cleanup() {
  local code=$?
  if [[ "${AGENT_ACTIVITY_TEST_KEEP:-0}" != "1" && -s "$BASELINE" ]]; then
    # Stop the Tauri owner before restoring its serialized engine; otherwise a
    # maintenance tick could write the temporary test sessions back.
    if [[ -n "$APP_PID" ]]; then
      kill -TERM "$APP_PID" >/dev/null 2>&1 || true
    else
      osascript -e 'tell application "Agent Activity Hub" to quit' >/dev/null 2>&1 || true
    fi
    sleep 1
    sqlite3 "$DB" "PRAGMA busy_timeout=5000; UPDATE state_snapshots SET payload=CAST(readfile('$BASELINE') AS TEXT), updated_at=datetime('now') WHERE singleton=1;" >/dev/null
    if (( WAS_RUNNING )); then
      open "$APP" >/dev/null 2>&1 || true
    fi
  fi
  rm -f "$BASELINE"
  exit "$code"
}
trap cleanup EXIT INT TERM

require_command sqlite3
require_command jq
require_command osascript
require_command open
[[ -x "$HELPER" ]] || die "Hook Helper is not executable: $HELPER"
[[ -f "$DB" ]] || die "activity database is not available: $DB"
[[ -S "$SOCKET" ]] || die "Tauri IPC socket is not available: $SOCKET"

leftover="$(snapshot_json | jq '[.sessions[] | select(.key.session_id | startswith("multistate-e2e-"))] | length')"
[[ "$leftover" == "0" ]] || die "found $leftover leftover multistate-e2e sessions; clean them before running"
snapshot_json > "$BASELINE" || die "could not capture baseline snapshot"
print "Running concurrent multi-agent lifecycle test: $RUN"
print "Baseline global: $(global_status)"

print "Step 1: create eight sessions and verify idle -> working"
for key in "${(@k)SESSION}"; do
  emit "$key" session.started "$RUN-$key-start"
done
assert_all idle
assert_target_rank "all sessions idle" idle
assert_overall_priority "empty work layer" idle
for key in "${(@k)SESSION}"; do
  emit "$key" user.prompted "$RUN-$key-prompt"
done
assert_all working
assert_target_rank "all sessions working" working
assert_overall_priority "all sessions working" working

print "Step 2: two concurrent tools in one Codex session"
emit codex-1 tool.started "$RUN-codex-1-tool-a"
emit codex-1 tool.started "$RUN-codex-1-tool-b"
emit codex-1 tool.finished "$RUN-codex-1-tool-a"
wait_status codex-1 working
emit codex-1 tool.finished "$RUN-codex-1-tool-b"
wait_status codex-1 working
print "PASS concurrent tool correlations stay working until both finish"

print "Step 3: approval yes and approval no are isolated per session"
emit codex-1 approval.required "$RUN-codex-1-approval-yes"
wait_status codex-1 waiting_approval
assert_target_rank "approval outranks work" waiting_approval
assert_overall_priority "approval outranks work" waiting_approval
emit codex-1 approval.resolved "$RUN-codex-1-approval-yes"
wait_status codex-1 working
emit codex-2 approval.required "$RUN-codex-2-approval-no"
wait_status codex-2 waiting_approval
emit codex-2 run.aborted "$RUN-codex-2-approval-no"
wait_status codex-2 idle
assert_reason_contains codex-2 "approval rejected"
wait_status codex-1 working
print "PASS approval yes -> working and approval no -> idle"

print "Step 4: concurrent priority error > approval > complete > working"
emit qoder-1 run.completed "$RUN-qoder-1-complete"
wait_status qoder-1 complete
assert_target_rank "complete outranks work" complete
assert_overall_priority "complete outranks work" complete
emit codex-1 approval.required "$RUN-codex-1-approval-priority"
wait_status codex-1 waiting_approval
assert_target_rank "approval outranks complete" waiting_approval
assert_overall_priority "approval outranks complete" waiting_approval
emit claude-1 run.failed "$RUN-claude-1-error"
wait_status claude-1 error
assert_target_rank "error outranks approval" error
assert_overall_priority "error outranks approval" error
sleep 2
wait_status claude-1 error
print "PASS error persists while other sessions change"

print "Step 5: recover error, resolve approval, and let complete lease expire"
emit claude-1 user.prompted "$RUN-claude-1-recover"
wait_status claude-1 working
emit codex-1 approval.resolved "$RUN-codex-1-approval-priority"
wait_status codex-1 working
sleep 6
wait_status qoder-1 idle
assert_target_rank "complete lease returns to work/idle" working
assert_overall_priority "complete lease expires" working

print "Step 6: simultaneous completions, abort, offline recovery, and duplicate event"
emit codex-1 run.completed "$RUN-codex-1-complete"
emit qoder-2 run.completed "$RUN-qoder-2-complete"
wait_status codex-1 complete
wait_status qoder-2 complete
assert_target_rank "multiple completions" complete
assert_overall_priority "multiple completions" complete
emit xxx-1 approval.required "$RUN-xxx-1-abort"
wait_status xxx-1 waiting_approval
emit xxx-1 run.aborted "$RUN-xxx-1-abort"
wait_status xxx-1 idle
assert_reason_contains xxx-1 "approval rejected"
emit xxx-2 session.stopped "$RUN-xxx-2-offline"
wait_status xxx-2 offline
emit xxx-2 model.working "$RUN-xxx-2-recover"
wait_status xxx-2 working
local_dedup_before="$(deduplicated_events)"
emit xxx-2 user.prompted "$RUN-xxx-2-duplicate"
emit xxx-2 user.prompted "$RUN-xxx-2-duplicate"
wait_status xxx-2 working
local_dedup_after="$(deduplicated_events)"
(( local_dedup_after > local_dedup_before )) || die "duplicate event was not counted as deduplicated"
print "PASS abort -> idle, offline -> working, and duplicate suppression"

print "Step 7: every test session returns to idle after terminal cleanup"
for key in "${(@k)SESSION}"; do
  emit "$key" run.completed "$RUN-$key-cleanup"
done
sleep 6
for key in "${(@k)SESSION}"; do
  wait_status "$key" idle
done
assert_target_rank "all test sessions idle" idle
print "PASS multi-agent, multi-session, multi-state lifecycle completed"
