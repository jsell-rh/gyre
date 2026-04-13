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
# Prerequisites: tmux session named "gyre-loop" (or set TMUX_SESSION).
# Usage: bash scripts/loop.sh
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

LOG=/tmp/gyre-loop.log
WORKTREE_BASE="$REPO_ROOT/worktrees/workers"
MAX_WORKERS=${GYRE_MAX_WORKERS:-6}
TMUX_SESSION=${GYRE_TMUX_SESSION:-gyre-loop}

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
    num=$(echo "$dep" | sed 's/task-//' | sed 's/^0*//')
    [ -z "$num" ] && continue
    num=$(printf "%03d" "$((10#$num))")
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

  # Clean up stale branch if it exists
  git branch -D "worker/$task_name" 2>/dev/null

  # Create worktree
  if ! git worktree add "$worktree" -b "worker/$task_name" HEAD 2>/dev/null; then
    log "!!! Failed to create worktree for $task_name"
    return 1
  fi

  log ">>> Spawning worker: $task_name (worktree: $worktree)"

  if ! tmux new-window -t "$TMUX_SESSION" -n "$task_name" \
    "bash '$REPO_ROOT/scripts/worker.sh' '$task_file' '$worktree'; echo 'Worker $task_name exited'; sleep 5"; then
    log "!!! Failed to spawn tmux window for $task_name (session '$TMUX_SESSION' exists?)"
    git worktree remove "$worktree" --force 2>/dev/null
    git branch -D "worker/$task_name" 2>/dev/null
    return 1
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

# Pre-flight: handle any ready-for-review tasks on main before going parallel
for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
  [ -f "$f" ] || continue
  status=$(get_progress "$f")
  [ "$status" = "ready-for-review" ] || continue
  task_name=$(basename "$f" .md)
  log ">>> Pre-flight: verifying $task_name on main"
  {
    cat specs/prompts/verifier.md
    printf '\n---\n\n## Pre-computed Target\n\nYour target task file is: `%s` (%s).\nRead this file first. Do not scan other task files to find work.\n' "$f" "$task_name"
  } | claude --model opus[1m] --dangerously-skip-permissions 2>/dev/null
  log "<<< Pre-flight verifier done for $task_name"

  new_status=$(get_progress "$f")
  if [ "$new_status" = "needs-revision" ]; then
    log ">>> Pre-flight: process revision for $task_name"
    claude --model opus[1m] --dangerously-skip-permissions < specs/prompts/process-revision.md 2>/dev/null
    log "<<< Pre-flight process revision done"
  fi
done

ITERATION=0
while true; do
  ITERATION=$((ITERATION + 1))
  log "--- Orchestrator cycle $ITERATION (${#ACTIVE_WORKERS[@]} active workers) ---"

  # 1. SERIAL: Spec-fidelity auditor (updates coverage matrix on main)
  if [ -d specs/coverage ]; then
    log ">>> Spec-Fidelity Auditor"
    claude --model opus[1m] --dangerously-skip-permissions \
      < specs/prompts/spec-fidelity-auditor.md 2>/dev/null
    log "<<< Auditor done"
  fi

  # 2. SERIAL: Project manager (reads matrix, creates tasks on main)
  log ">>> Project Manager"
  claude --model opus[1m] --dangerously-skip-permissions \
    < specs/prompts/project-manager.md 2>/dev/null
  log "<<< Project Manager done"

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
      # Check coverage matrix — are we actually done?
      not_started=$(grep -c 'not-started' specs/coverage/system/*.md 2>/dev/null || echo 0)
      if [ "$not_started" -eq 0 ]; then
        log "=== All specs covered, all tasks complete ==="
        break
      else
        log "    $not_started coverage gaps remain — next auditor cycle will surface them"
      fi
    fi
  fi

  # Poll every 30 seconds
  sleep 30
done

log "=== Loop complete after $ITERATION iterations ==="
