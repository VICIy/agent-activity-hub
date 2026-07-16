#!/bin/zsh

# End-to-end lifecycle verification against the running Agent Activity Hub.
# It uses the production Hook Helper and reads the persisted engine snapshot,
# covering IPC, reduction, persistence, leases and global priority.

set -euo pipefail

ROOT="${0:A:h:h}"
HELPER="${AGENT_ACTIVITY_HOOK:-$ROOT/target/release/bundle/macos/Agent Activity Hub.app/Contents/MacOS/agent-activity-hook}"
DB="${AGENT_ACTIVITY_DB:-$HOME/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/activity.db}"
SOCKET="${AGENT_ACTIVITY_SOCKET:-$HOME/Library/Application Support/work.Effective-Work.Agent-Activity-Hub/ipc-v1.sock}"
SNAPSHOT_QUERY='SELECT payload FROM state_snapshots WHERE singleton = 1;'
RUN="multi-e2e-$(date +%s)-$$"

die() {
  print -u2 "FAIL: $*"
  print -u2 "Current snapshot:"
  snapshot_json >&2 || true
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_command sqlite3
require_command jq
[[ -x "$HELPER" ]] || die "Hook Helper is not executable: $HELPER"
[[ -S "$SOCKET" ]] || die "Hub IPC socket is not available; start the Tauri app first"
[[ -f "$DB" ]] || die "activity database is not available: $DB"

snapshot_json() {
  local payload attempt
  for attempt in {1..20}; do
    # Read the payload directly. `sqlite3 -json` would escape the already
    # serialized engine snapshot and becomes unnecessarily expensive as the
    # bounded dedupe index grows.
    if payload="$(sqlite3 -noheader -batch "$DB" "$SNAPSHOT_QUERY" 2>/dev/null || true)" \
      && [[ -n "$payload" ]] \
      && print -r -- "$payload" | jq -e type >/dev/null 2>&1; then
      print -r -- "$payload"
      return 0
    fi
    sleep 0.1
  done
  return 1
}

status_for() {
  local provider="$1" instance="$2" session="$3"
  snapshot_json | jq -r --arg provider "$provider" --arg instance "$instance" --arg session "$session" '
    [.sessions[] | select(.key.provider == $provider and .key.instance_id == $instance and .key.session_id == $session) | .status][0] // "missing"
  '
}

reason_for() {
  local provider="$1" instance="$2" session="$3"
  snapshot_json | jq -r --arg provider "$provider" --arg instance "$instance" --arg session "$session" '
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

assert_status() {
  local label="$1" provider="$2" instance="$3" session="$4" expected="$5"
  local actual attempt
  for attempt in {1..30}; do
    actual="$(status_for "$provider" "$instance" "$session")"
    if [[ "$actual" == "$expected" ]]; then
      print "PASS $label: $provider/$instance/$session -> $actual"
      return 0
    fi
    sleep 0.1
  done
  die "$label expected $expected for $provider/$instance/$session, got $actual"
}

assert_reason_contains() {
  local label="$1" provider="$2" instance="$3" session="$4" expected="$5"
  local actual
  actual="$(reason_for "$provider" "$instance" "$session")"
  [[ "$actual" == *"$expected"* ]] || die "$label expected reason containing '$expected', got '$actual'"
  print "PASS $label: reason='$actual'"
}

assert_global() {
  local label="$1" expected="$2" actual
  actual="$(global_status)"
  [[ "$actual" == "$expected" ]] || die "$label expected global=$expected, got global=$actual"
  print "PASS $label: global -> $actual"
}

emit() {
  local provider="$1" instance="$2" session="$3" kind="$4" correlation="$5" project="$6"
  "$HELPER" emit \
    --provider "$provider" \
    --instance "$instance" \
    --session "$session" \
    --kind "$kind" \
    --correlation "$correlation" \
    --project "/tmp/$project"
}

session_id() {
  print -r -- "$RUN-$1"
}

typeset -A PROVIDER INSTANCE SESSION PROJECT
for key in codex-1 codex-2 qoder-1 qoder-2 claude-1 claude-2 xxx-1; do
  PROVIDER[$key]="${key%%-*}"
  INSTANCE[$key]="${PROVIDER[$key]}-instance-${key##*-}"
  SESSION[$key]="$(session_id "$key")"
  PROJECT[$key]="project-${key}"
done

print "Running multi-agent lifecycle test: $RUN"

# Refuse to run over an unrelated live workflow. This makes failures explicit
# instead of allowing another real session to influence the priority assertion.
active="$(snapshot_json | jq -r '[.sessions[] | select(.status | IN("working", "waiting_approval", "complete", "error"))] | length')"
[[ "$active" == "0" ]] || die "existing active sessions detected; run this test when the Hub is idle"

print "Step 1: start two sessions per named agent plus one custom provider"
for key in codex-1 codex-2 qoder-1 qoder-2 claude-1 claude-2 xxx-1; do
  emit "${PROVIDER[$key]}" "${INSTANCE[$key]}" "${SESSION[$key]}" \
    user.prompted "$RUN-$key-start" "${PROJECT[$key]}"
done
for key in codex-1 codex-2 qoder-1 qoder-2 claude-1 claude-2 xxx-1; do
  assert_status "start $key" "${PROVIDER[$key]}" "${INSTANCE[$key]}" "${SESSION[$key]}" working
done
assert_global "all sessions working" working

print "Step 2: offline -> working recovery for xxx-1"
emit "${PROVIDER[xxx-1]}" "${INSTANCE[xxx-1]}" "${SESSION[xxx-1]}" \
  session.stopped "$RUN-xxx-1-stop" "${PROJECT[xxx-1]}"
assert_status "session stopped" "${PROVIDER[xxx-1]}" "${INSTANCE[xxx-1]}" "${SESSION[xxx-1]}" offline
emit "${PROVIDER[xxx-1]}" "${INSTANCE[xxx-1]}" "${SESSION[xxx-1]}" \
  user.prompted "$RUN-xxx-1-restart" "${PROJECT[xxx-1]}"
assert_status "offline recovery" "${PROVIDER[xxx-1]}" "${INSTANCE[xxx-1]}" "${SESSION[xxx-1]}" working

print "Step 3: waiting approval outranks working; yes returns to working"
approval_yes="$RUN-codex-1-approval-yes"
emit codex "${INSTANCE[codex-1]}" "${SESSION[codex-1]}" approval.required "$approval_yes" "${PROJECT[codex-1]}"
assert_status "approval required" codex "${INSTANCE[codex-1]}" "${SESSION[codex-1]}" waiting_approval
assert_global "waiting outranks working" waiting_approval
emit codex "${INSTANCE[codex-1]}" "${SESSION[codex-1]}" approval.resolved "$approval_yes" "${PROJECT[codex-1]}"
assert_status "approval yes" codex "${INSTANCE[codex-1]}" "${SESSION[codex-1]}" working
assert_global "approval yes returns to working" working

print "Step 4: waiting approval no returns to idle, without affecting other sessions"
approval_no="$RUN-codex-2-approval-no"
emit codex "${INSTANCE[codex-2]}" "${SESSION[codex-2]}" approval.required "$approval_no" "${PROJECT[codex-2]}"
assert_global "second waiting outranks working" waiting_approval
emit codex "${INSTANCE[codex-2]}" "${SESSION[codex-2]}" run.completed "$RUN-codex-2-approval-rejected" "${PROJECT[codex-2]}"
assert_status "approval no" codex "${INSTANCE[codex-2]}" "${SESSION[codex-2]}" idle
assert_reason_contains "approval no reason" codex "${INSTANCE[codex-2]}" "${SESSION[codex-2]}" "approval rejected"
assert_global "approval no leaves other work" working

print "Step 5: complete outranks working, then expires to working"
emit qoder "${INSTANCE[qoder-2]}" "${SESSION[qoder-2]}" run.completed "$RUN-qoder-2-complete" "${PROJECT[qoder-2]}"
assert_status "qoder complete" qoder "${INSTANCE[qoder-2]}" "${SESSION[qoder-2]}" complete
assert_global "complete outranks working" complete
sleep 6
assert_status "complete lease expiry" qoder "${INSTANCE[qoder-2]}" "${SESSION[qoder-2]}" idle
assert_global "expired complete returns to working" working

print "Step 6: error outranks every non-error state and persists"
emit qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" run.failed "$RUN-qoder-1-failed" "${PROJECT[qoder-1]}"
assert_status "qoder error" qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" error
assert_global "error outranks all" error
sleep 2
assert_status "error persistence" qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" error
assert_global "error remains global" error

print "Step 7: a new prompt recovers the failed session; completion wins over work"
emit qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" user.prompted "$RUN-qoder-1-recover" "${PROJECT[qoder-1]}"
assert_status "error recovery prompt" qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" working
assert_global "recovered error returns to working" working
emit qoder "${INSTANCE[qoder-1]}" "${SESSION[qoder-1]}" run.completed "$RUN-qoder-1-complete" "${PROJECT[qoder-1]}"
emit claude "${INSTANCE[claude-1]}" "${SESSION[claude-1]}" run.completed "$RUN-claude-1-complete" "${PROJECT[claude-1]}"
emit claude "${INSTANCE[claude-2]}" "${SESSION[claude-2]}" run.completed "$RUN-claude-2-complete" "${PROJECT[claude-2]}"
emit codex "${INSTANCE[codex-1]}" "${SESSION[codex-1]}" run.completed "$RUN-codex-1-complete" "${PROJECT[codex-1]}"
emit xxx "${INSTANCE[xxx-1]}" "${SESSION[xxx-1]}" run.completed "$RUN-xxx-1-complete" "${PROJECT[xxx-1]}"
for key in qoder-1 claude-1 claude-2 codex-1 xxx-1; do
  assert_status "completion $key" "${PROVIDER[$key]}" "${INSTANCE[$key]}" "${SESSION[$key]}" complete
done
assert_global "multiple completions outrank work" complete

print "Step 8: every test session returns to idle after completion leases"
sleep 6
for key in codex-1 codex-2 qoder-1 qoder-2 claude-1 claude-2 xxx-1; do
  assert_status "final idle $key" "${PROVIDER[$key]}" "${INSTANCE[$key]}" "${SESSION[$key]}" idle
done
assert_global "all test sessions idle" idle

print "PASS: multi-agent, multi-session, multi-state lifecycle completed ($RUN)"
