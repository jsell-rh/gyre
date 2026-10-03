#!/usr/bin/env bash
# Fleet orchestrator: run the Gyre dev loop with one OpenShell sandbox per
# task. All orchestration (task selection, dispatch, merge, parallelism)
# runs locally; ALL agent work (implementation, compilation, testing) runs
# inside sandboxes via scripts/worker-sandbox.sh.
#
# This is the sandbox-mode twin of scripts/loop.sh: same task-selection
# rules (needs-revision first, then not-started with satisfied deps), but
# workers are remote sandbox processes instead of local tmux worktrees.
#
# LIVE PARALLELISM LEVER: the number of concurrent workers is re-read from
# /tmp/gyre-sandbox/parallelism on every cycle. Raise it to add workers
# mid-run; lower it to drain (running workers finish; no new spawns).
# The dashboard (loop-dashboard.mjs) exposes this as a slider.
#
# Usage: fleet-sandbox.sh
# Requires: OPENSHELL_OIDC_CLIENT_SECRET exported (see OPENSHELL-GOAL.md).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"
STATUS_DIR="${STATUS_DIR:-/tmp/gyre-sandbox/status}"
FLEET_LOG="${FLEET_LOG:-/tmp/gyre-sandbox/fleet.log}"
PARALLELISM_FILE="${PARALLELISM_FILE:-/tmp/gyre-sandbox/parallelism}"
INITIAL_PARALLELISM="${INITIAL_PARALLELISM:-2}"
MAX_PARALLELISM="${MAX_PARALLELISM:-100}"
CYCLE_SECS="${CYCLE_SECS:-60}"

mkdir -p "$STATUS_DIR"
log() { echo "[$(date '+%H:%M:%S')] [fleet] $*" | tee -a "$FLEET_LOG" >&2; }

# --- Live parallelism lever ---------------------------------------------
# The file is the source of truth; the dashboard slider writes it. Clamp
# to [1, MAX_PARALLELISM] and fall back to the last-known-good value on
# garbage/missing (never zero out the fleet on a bad write).
current_parallelism=$INITIAL_PARALLELISM
# The lever file is the source of truth; a restart must NOT clobber an
# existing value with the launch-time default.
[ -f "$PARALLELISM_FILE" ] || echo "$INITIAL_PARALLELISM" > "$PARALLELISM_FILE"
read_parallelism() {
  local v
  v=$(cat "$PARALLELISM_FILE" 2>/dev/null | tr -cd '0-9')
  if [ -n "$v" ] && [ "$v" -ge 1 ] && [ "$v" -le "$MAX_PARALLELISM" ]; then
    if [ "$v" != "$current_parallelism" ]; then
      log "    parallelism: $current_parallelism -> $v"
      current_parallelism=$v
    fi
  fi
}

# --- Task metadata (same semantics as loop.sh) ---------------------------
get_progress() {
  bash "$REPO_ROOT/scripts/task-field.sh" "$1" progress 2>/dev/null || echo ""
}

task_is_complete() {
  local f="$REPO_ROOT/specs/tasks/task-${1}.md"
  [ -f "$f" ] && [ "$(get_progress "$f")" = "complete" ]
}

deps_satisfied() {
  local task_file="$1" deps dep num
  deps=$(bash "$REPO_ROOT/scripts/task-field.sh" "$task_file" depends_on 2>/dev/null)
  [ -z "$deps" ] && return 0
  while IFS= read -r dep; do
    [ -z "$dep" ] && continue
    num=$(echo "$dep" | sed 's/task-//' | sed 's/^0*//' | tr -cd '0-9')
    [ -z "$num" ] && continue
    num=$(printf "%03d" "$num")
    task_is_complete "$num" || return 1
  done <<< "$deps"
  return 0
}

# Worker branch on origin is the remote hand-off: the sandbox worker pushes
# worker/<task> and (on completion) the task frontmatter says complete.
# IMPORTANT: the local repo only sees task-file progress once the fleet
# merges the branch to main (merge_worker). Before that, the sandbox's
# frontmatter update lives only on the remote branch — so an in-flight
# task must stay claimed, which ACTIVE_TASKS handles.
declare -A ACTIVE_TASKS=()  # task_name -> pid

find_eligible_tasks() {
  local f
  for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
    [ -f "$f" ] || continue
    [ "$(get_progress "$f")" = "needs-revision" ] && echo "$f"
  done
  for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
    [ -f "$f" ] || continue
    [ "$(get_progress "$f")" != "not-started" ] && continue
    deps_satisfied "$f" && echo "$f"
  done
}

# --- Dispatch + reap ------------------------------------------------------
spawn_sandbox_worker() {
  local task_file="$1" task_name pid
  task_name=$(basename "$task_file" .md)
  mkdir -p "$STATUS_DIR/$task_name"
  # Each worker driver is its own process; TOTAL_ROUNDS is per-task budget.
  nohup bash "$REPO_ROOT/scripts/worker-sandbox.sh" "$task_file" \
    >> "$STATUS_DIR/$task_name/driver.log" 2>&1 &
  pid=$!
  ACTIVE_TASKS[$task_name]=$pid
  log ">>> Spawned sandbox worker for $task_name (pid $pid)"
}

reap_workers() {
  # Remove finished drivers; merge completed tasks to main.
  local task_name pid task_file
  for task_name in "${!ACTIVE_TASKS[@]}"; do
    pid=${ACTIVE_TASKS[$task_name]}
    if ! kill -0 "$pid" 2>/dev/null; then
      unset "ACTIVE_TASKS[$task_name]"
      wait "$pid" 2>/dev/null
      local rc=$?
      log "<<< Driver for $task_name exited (rc=$rc)"
    fi
  done
}

merge_completed() {
  # Fetch remote worker branches; merge any whose task is complete.
  git fetch -q origin '+refs/heads/worker/*:refs/remotes/origin/worker/*' 2>/dev/null
  local task_name branch f status
  for task_name in "${!ACTIVE_TASKS[@]}"; do
    branch="origin/worker/$task_name"
    git rev-parse -q --verify "refs/remotes/$branch" >/dev/null 2>&1 || continue
    f="$REPO_ROOT/specs/tasks/$task_name.md"
    # The task file ON THE BRANCH is authoritative (sandbox updated it).
    status=$(git show "$branch:specs/tasks/$task_name.md" 2>/dev/null | \
      bash "$REPO_ROOT/scripts/task-field.sh" /dev/stdin progress 2>/dev/null || true)
    if [ "$status" = "complete" ]; then
      if git merge "$branch" --no-edit -m "merge: integrate $task_name from sandbox worker" >/dev/null 2>&1; then
        log "    Merged $task_name to main"
        git push origin main >/dev/null 2>&1 || log "    push main failed (retry next cycle)"
        git push origin --delete "worker/$task_name" >/dev/null 2>&1 || true
      else
        log "    !!! merge conflict for $task_name — branch kept for retry"
        git merge --abort 2>/dev/null || true
      fi
    fi
  done
}

# Orphan adoption: worker-sandbox.sh drivers started outside the fleet
# (manual runs, prior fleet instance) must not be double-spawned — the
# same task would get two sandboxes fighting over one branch. Register
# live drivers by scanning their command lines.
adopt_orphans() {
  local line task_name pid rest
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    pid=$(echo "$line" | awk '{print $1}')
    rest=$(echo "$line" | cut -d' ' -f2-)
    task_name=$(echo "$rest" | grep -oE 'task-[0-9]+' | head -1)
    [ -z "$task_name" ] && continue
    [ -n "${ACTIVE_TASKS[$task_name]:-}" ] && continue
    ACTIVE_TASKS[$task_name]=$pid
    log "    Adopted live driver for $task_name (pid $pid)"
  done < <(pgrep -af 'worker-sandbox.sh .*specs/tasks/task-' 2>/dev/null || true)
}

log "=== Sandbox fleet loop started (parallelism lever: $PARALLELISM_FILE) ==="
adopt_orphans

while true; do
  read_parallelism
  adopt_orphans
  reap_workers

  # Merge any completed work into local main + push.
  merge_completed

  # Rescan eligibility AFTER merges (completed deps unlock new tasks).
  # Spawn up to the live parallelism lever.
  local_active=${#ACTIVE_TASKS[@]}
  if [ "$local_active" -lt "$current_parallelism" ]; then
    slots=$((current_parallelism - local_active))
    while IFS= read -r task_file; do
      [ -z "$task_file" ] && continue
      [ "$slots" -le 0 ] && break
      task_name=$(basename "$task_file" .md)
      # Double-check liveness: adoption can miss a driver whose /proc cmdline
      # was read mid-write (partial argv). Never double-spawn a live task.
      pgrep -f "worker-sandbox\.sh .*$task_name\.md" >/dev/null 2>&1 && continue
      [ -n "${ACTIVE_TASKS[$task_name]:-}" ] && continue
      spawn_sandbox_worker "$task_file"
      slots=$((slots - 1))
    done < <(find_eligible_tasks)
  fi

  # Publish fleet status for the dashboard.
  printf '{"active":%s,"parallelism":%s,"updated":"%s"}\n' \
    "${#ACTIVE_TASKS[@]}" "$current_parallelism" "$(date -Is)" \
    > /tmp/gyre-sandbox/fleet.json 2>/dev/null || true

  sleep "$CYCLE_SECS"
done
