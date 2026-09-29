#!/usr/bin/env bash
# Parallel development loop with spec coverage tracking.
#
# Architecture:
#   Serial on main:  auditor -> PM  (both mutate shared coverage/task files)
#   Parallel in worktrees: workers  (implement -> verify -> process-revision)
#
# The auditor updates the coverage matrix, then the PM reads it and creates
# tasks. Workers are spawned in git worktrees (one per task) and run the
# implement -> verify -> process-revision cycle independently. Completed
# worktrees are merged back to main.
#
# Prerequisites: omp CLI on PATH (agent runner). tmux optional — workers
#   fall back to background processes when tmux is unavailable.
# Usage: bash scripts/loop.sh
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

LOG=/tmp/gyre-loop.log
WORKTREE_BASE="$REPO_ROOT/worktrees/workers"
MAX_WORKERS=${GYRE_MAX_WORKERS:-6}
TMUX_SESSION=${GYRE_TMUX_SESSION:-gyre-loop}

# --- Single-instance guard ---
# Two concurrent loops spawn duplicate workers on the same worktrees, which
# corrupts in-flight agent runs. Hold a lock for the lifetime of this process.
LOCK_FILE=/tmp/gyre-loop.lock
if command -v flock >/dev/null 2>&1; then
  exec 9>"$LOCK_FILE"
  flock -n 9 || { echo "ERROR: another loop is already running (lock: $LOCK_FILE). Aborting." >&2; exit 1; }
else
  # flock-less fallback: PID lockfile with staleness check
  if [ -f "$LOCK_FILE" ]; then
    lock_pid=$(head -1 "$LOCK_FILE" 2>/dev/null)
    if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then
      echo "ERROR: another loop (pid $lock_pid) is already running (lock: $LOCK_FILE). Aborting." >&2
      exit 1
    fi
    echo "stale lock from pid $lock_pid — reclaiming" >&2
  fi
  echo $$ > "$LOCK_FILE"
  trap 'rm -f "$LOCK_FILE"' EXIT
fi

# Agent runner: omp in headless print mode, auto-approved, no session state.
# Set GYRE_MODEL to override the model (fuzzy match, e.g. "opus").
OMP=${OMP:-omp}
run_agent() {
  if [ -n "${GYRE_MODEL:-}" ]; then
    "$OMP" -p --no-session --approval-mode yolo --model "$GYRE_MODEL"
  else
    "$OMP" -p --no-session --approval-mode yolo
  fi
}

# Working-tree guard for agents that run on main (auditor, PM, pre-flight
# verifier): they commit their own edits, and agents that use `git add -A`
# sweep unrelated in-flight human/agent work into their commits (observed
# in 32edb3f1 and d7d67e22). Stash uncommitted work around the run.
wt_guard_stash() {
  WT_GUARDED=0
  if ! git diff --quiet --ignore-submodules -- || \
     ! git diff --cached --quiet --ignore-submodules -- || \
     [ -n "$(git ls-files --others --exclude-standard)" ]; then
    git stash push --include-untracked --message "loop-wt-guard" >/dev/null 2>&1 && WT_GUARDED=1
  fi
}
wt_guard_restore() {
  if [ "${WT_GUARDED:-0}" -eq 1 ]; then
    git stash pop >/dev/null 2>&1 || \
      log "!!! WARNING: failed to restore stashed WIP (loop-wt-guard) — check git stash list"
  fi
}

# Serial agents (auditor/PM): prompt = GOAL.md + their prompt file.
run_serial_agent() {
  wt_guard_stash
  cat specs/GOAL.md "$1" | run_agent 2>/dev/null
  local rc=$?
  wt_guard_restore
  return $rc
}

log() { echo "[$(date '+%H:%M:%S')] [orchestrator] $*" | tee -a "$LOG"; }

# --- Task metadata helpers ---

get_progress() {
  bash "$REPO_ROOT/scripts/task-field.sh" "$1" progress 2>/dev/null || echo ""
}

task_is_complete() {
  local f="$REPO_ROOT/specs/tasks/task-${1}.md"
  [ -f "$f" ] && [ "$(get_progress "$f")" = "complete" ]
}

deps_satisfied() {
  local task_file="$1"
  local deps
  deps=$(bash "$REPO_ROOT/scripts/task-field.sh" "$task_file" depends_on 2>/dev/null)
  [ -z "$deps" ] && return 0

  while IFS= read -r dep; do
    [ -z "$dep" ] && continue
    # Normalize: strip "task-" prefix and leading zeros
    local num
    num=$(echo "$dep" | sed 's/task-//' | sed 's/^0*//' | tr -cd '0-9')
    [ -z "$num" ] && continue
    num=$(printf "%03d" "$num")
    if ! task_is_complete "$num"; then
      return 1
    fi
  done <<< "$deps"
  return 0
}

find_eligible_tasks() {
  # Priority 1: needs-revision tasks (return to worker for fixes)
  for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
    [ -f "$f" ] || continue
    [ "$(get_progress "$f")" = "needs-revision" ] && echo "$f"
  done
  # Priority 2: not-started tasks with satisfied deps
  for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
    [ -f "$f" ] || continue
    [ "$(get_progress "$f")" != "not-started" ] && continue
    deps_satisfied "$f" && echo "$f"
  done
}

# --- Worker management ---

declare -A ACTIVE_WORKERS=()  # task_name -> worktree_path

spawn_worker() {
  local task_file="$1"
  local task_name
  task_name=$(basename "$task_file" .md)
  local worktree="$WORKTREE_BASE/$task_name"

  # Drop registrations for worktree directories that no longer exist on
  # disk (e.g. after a crash or manual cleanup). Without this, a prunable
  # registration blocks both the branch delete below and worktree add —
  # the task is then wedged forever, failing every cycle.
  git worktree prune

  # Clean up stale branch and stale worktree directory if it exists
  git branch -D "worker/$task_name" 2>/dev/null

  # Create worktree
  if ! git worktree add "$worktree" -b "worker/$task_name" HEAD 2>/dev/null; then
    log "!!! Failed to create worktree for $task_name"
    return 1
  fi

  log ">>> Spawning worker: $task_name (worktree: $worktree)"

  # Ensure the loop's tmux session exists (new-window needs a running server)
  if command -v tmux >/dev/null 2>&1; then
    tmux has-session -t "$TMUX_SESSION" 2>/dev/null || \
      tmux new-session -d -s "$TMUX_SESSION" "exec sleep infinity"
  fi

  if command -v tmux >/dev/null 2>&1 && \
     tmux new-window -t "$TMUX_SESSION" -n "$task_name" \
       "bash '$REPO_ROOT/scripts/worker.sh' '$task_file' '$worktree'; echo 'Worker $task_name exited'; sleep 5"; then
    :
  else
    # No tmux (or window spawn failed) — run the worker as a background
    # process. Output goes to the worker's own .worker.log and
    # /tmp/gyre-loop.log (see worker.sh log()).
    log "    (tmux unavailable — backgrounding worker)"
    nohup bash "$REPO_ROOT/scripts/worker.sh" "$task_file" "$worktree" \
      > "$worktree/.spawn.log" 2>&1 &
  fi

  ACTIVE_WORKERS[$task_name]="$worktree"
  return 0
}

check_worker_done() {
  local task_name="$1"
  local worktree="${ACTIVE_WORKERS[$task_name]}"
  [ -f "$worktree/.done" ]
}

merge_worker() {
  local task_name="$1"
  local worktree="${ACTIVE_WORKERS[$task_name]}"
  local branch="worker/$task_name"

  log "<<< Merging $task_name back to main"

  if git merge "$branch" --no-edit -m "merge: integrate $task_name from parallel worker" 2>/dev/null; then
    log "    Merge successful"
  else
    # Merge conflict — take theirs for task/review files, auto-resolve rest
    log "    Merge conflict on $task_name — attempting resolution"
    git checkout --theirs specs/tasks/ specs/reviews/ 2>/dev/null
    git add specs/tasks/ specs/reviews/ 2>/dev/null

    local conflicts
    conflicts=$(git diff --name-only --diff-filter=U 2>/dev/null)
    if [ -n "$conflicts" ]; then
      log "    !!! Unresolvable conflicts in: $conflicts"
      git merge --abort 2>/dev/null
      log "    Merge aborted for $task_name — will retry next cycle"
      git worktree remove "$worktree" --force 2>/dev/null
      git branch -D "$branch" 2>/dev/null
      unset "ACTIVE_WORKERS[$task_name]"
      return 1
    fi
    git commit --no-edit 2>/dev/null
    log "    Conflict auto-resolved"
  fi

  # Clean up
  git worktree remove "$worktree" --force 2>/dev/null
  git branch -d "$branch" 2>/dev/null
  unset "ACTIVE_WORKERS[$task_name]"
  log "    Cleaned up worktree for $task_name"
  return 0
}

cleanup_all() {
  log "Loop exiting — detaching worktrees (branches preserved for recovery)"
  for task_name in "${!ACTIVE_WORKERS[@]}"; do
    local worktree="${ACTIVE_WORKERS[$task_name]}"
    git worktree remove "$worktree" --force 2>/dev/null
    # Intentionally NOT deleting worker branches — commits are preserved
    log "    Detached worktree for $task_name (branch worker/$task_name intact)"
  done
  rm -rf "$WORKTREE_BASE" 2>/dev/null
}
trap cleanup_all EXIT

# --- Main loop ---

mkdir -p "$WORKTREE_BASE"
> "$LOG"
log "=== Parallel Dev Loop Started (max $MAX_WORKERS workers) ==="

for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
  [ -f "$f" ] || continue
  status=$(get_progress "$f")
  [ "$status" = "ready-for-review" ] || continue
  task_name=$(basename "$f" .md)
  log ">>> Pre-flight: verifying $task_name on main"
  wt_guard_stash
  {
    cat specs/GOAL.md specs/prompts/verifier.md
    printf '\n---\n\n## Pre-computed Target\n\nYour target task file is: `%s` (%s).\nRead this file first. Do not scan other task files to find work.\n' "$f" "$task_name"
  } | run_agent 2>/dev/null
  wt_guard_restore
  log "<<< Pre-flight verifier done for $task_name"

  new_status=$(get_progress "$f")
  if [ "$new_status" = "needs-revision" ]; then
    log ">>> Pre-flight: process revision for $task_name"
    wt_guard_stash
    cat specs/GOAL.md specs/prompts/process-revision.md | \
      run_agent 2>/dev/null
    wt_guard_restore
    log "<<< Pre-flight process revision done"
  fi
done

ITERATION=0
while true; do
  ITERATION=$((ITERATION + 1))
  log "--- Orchestrator cycle $ITERATION (${#ACTIVE_WORKERS[@]} active workers) ---"

  # 1. SERIAL: Spec-fidelity auditor (updates coverage matrix on main)
  #    Skip if no code changes since last audit — avoids burning Opus tokens
  #    re-confirming identical classifications.
  if [ -d specs/coverage ]; then
    last_audit_sha=$(git log -1 --format=%H --grep='audit(coverage)' 2>/dev/null || true)
    code_changes_since=0
    if [ -n "$last_audit_sha" ]; then
      code_changes_since=$(git log "$last_audit_sha"..HEAD --oneline \
        --invert-grep --grep='audit(coverage)\|^merge:' 2>/dev/null | wc -l)
    else
      code_changes_since=1  # no audit yet — run the auditor
    fi
    if [ "$code_changes_since" -gt 0 ]; then
      log ">>> Spec-Fidelity Auditor ($code_changes_since code change(s) since last audit)"
      run_serial_agent specs/prompts/spec-fidelity-auditor.md
      log "<<< Auditor done"
    else
      log "    Auditor skipped — no code changes since last audit"
    fi
  fi

  # 2. SERIAL: Project manager (reads matrix, creates tasks on main)
  #    Skip if no not-started sections remain — nothing to decompose.
  # Count table rows with "| not-started |" (excludes header summary text)
  not_started_count=$(grep -c '| not-started |' specs/coverage/system/*.md 2>/dev/null \
    | awk -F: '{s+=$2} END{print s+0}')

  if [ "$not_started_count" -gt 0 ]; then
    log ">>> Project Manager ($not_started_count not-started section(s) to decompose)"
    run_serial_agent specs/prompts/project-manager.md
    log "<<< Project Manager done"
  else
    log "    PM skipped — no not-started coverage sections"
  fi

  # 3. Merge completed workers back to main
  for task_name in "${!ACTIVE_WORKERS[@]}"; do
    if check_worker_done "$task_name"; then
      merge_worker "$task_name"
    fi
  done

  # 4. Spawn new workers for eligible tasks up to MAX_WORKERS
  active=${#ACTIVE_WORKERS[@]}
  if [ "$active" -lt "$MAX_WORKERS" ]; then
    slots=$((MAX_WORKERS - active))
    eligible=$(find_eligible_tasks | head -"$slots")

    for task_file in $eligible; do
      task_name=$(basename "$task_file" .md)
      # Don't spawn if already active
      [ -n "${ACTIVE_WORKERS[$task_name]:-}" ] && continue
      spawn_worker "$task_file" || true
    done
  fi

  # 5. Status report
  active=${#ACTIVE_WORKERS[@]}
  log "    Active workers: $active"
  for task_name in "${!ACTIVE_WORKERS[@]}"; do
    log "      - $task_name"
  done

  # 6. Check convergence
  if [ "$active" -eq 0 ]; then
    remaining=$(find_eligible_tasks | wc -l)
    if [ "$remaining" -eq 0 ]; then
      # Check coverage matrix — are we actually done? (specs/GOAL.md)
      # Done = zero not-started AND zero task-assigned sections AND no
      # incomplete tasks. Partial-implemented sections stay open — the
      # auditor re-splits them into tasks.
      not_started=$(grep -c '| not-started |' specs/coverage/system/*.md 2>/dev/null \
        | awk -F: '{s+=$2} END{print s+0}')
      task_assigned=$(grep -c '| task-assigned |' specs/coverage/system/*.md 2>/dev/null \
        | awk -F: '{s+=$2} END{print s+0}')
      incomplete_tasks=$(grep -rl 'progress: \(not-started\|in-progress\|ready-for-review\|needs-revision\)' \
        specs/tasks/ 2>/dev/null | wc -l)
      if [ "$not_started" -eq 0 ] && [ "$task_assigned" -eq 0 ] && [ "$incomplete_tasks" -eq 0 ]; then
        log "=== All specs covered, all tasks complete (goal: specs/GOAL.md) ==="
        break
      else
        log "    $not_started not-started, $task_assigned task-assigned coverage sections, $incomplete_tasks incomplete tasks remain"
      fi
    fi
  fi

  # Poll every 30 seconds
  sleep 30
done

log "=== Loop complete after $ITERATION iterations ==="
