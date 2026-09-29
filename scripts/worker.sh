#!/usr/bin/env bash
# Worker: implement -> verify -> process-revision for one task in a worktree.
# Runs up to MAX_ROUNDS cycles until the task reaches `complete`.
# Signals completion by touching $WORKTREE/.done
# Usage: worker.sh <task-file> <worktree-path>
set -uo pipefail

TASK_FILE="$1"
WORKTREE="$2"
TASK_NAME=$(basename "$TASK_FILE" .md)

cd "$WORKTREE"

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

log() {
  echo "[$(date '+%H:%M:%S')] [$TASK_NAME] $*" >> "$WORKTREE/.worker.log"
  echo "[$(date '+%H:%M:%S')] [$TASK_NAME] $*" >> /tmp/gyre-loop.log
}

get_status() {
  bash scripts/task-field.sh "$TASK_FILE" progress 2>/dev/null || echo "unknown"
}

inject_task_prompt() {
  local prompt_file="$1" task_file="$2"
  cat specs/GOAL.md "$prompt_file"
  printf '\n---\n\n## Pre-computed Target\n\n'
  printf 'Your target task file is: `%s` (%s).\n' "$task_file" "$TASK_NAME"
  printf 'Read this file first. Do not scan other task files to find work.\n'
}

MAX_ROUNDS=6
ROUND=0

while [ $ROUND -lt $MAX_ROUNDS ]; do
  ROUND=$((ROUND + 1))
  status=$(get_status)

  case "$status" in
    not-started|needs-revision)
      # Rebase onto the main repo HEAD to pick up merged work from other workers
      log "--- Rebasing onto main HEAD (round $ROUND)"
      MAIN_ROOT="$(git rev-parse --git-common-dir 2>/dev/null | sed 's|/\.git$||')"
      if [ -n "$MAIN_ROOT" ] && git fetch "$MAIN_ROOT" HEAD 2>/dev/null && git rebase FETCH_HEAD 2>/dev/null; then
        :
      else
        log "!!! Rebase failed (may need manual resolution)"
      fi

      log ">>> Implementation (round $ROUND, status=$status)"
      inject_task_prompt specs/prompts/implementation.md "$TASK_FILE" | \
        run_agent 2>/dev/null
      log "<<< Implementation done (exit=$?)"
      log "    Last commit: $(git log --oneline -1 2>/dev/null)"
      ;;

    ready-for-review)
      log ">>> Verifier (round $ROUND)"
      inject_task_prompt specs/prompts/verifier.md "$TASK_FILE" | \
        run_agent 2>/dev/null
      log "<<< Verifier done (exit=$?)"
      log "    Last commit: $(git log --oneline -1 2>/dev/null)"

      # If verifier found issues, run process revision
      new_status=$(get_status)
      if [ "$new_status" = "needs-revision" ]; then
        log ">>> Process Revision (round $ROUND)"
        cat specs/GOAL.md specs/prompts/process-revision.md | \
          run_agent 2>/dev/null
        log "<<< Process Revision done"
      fi
      ;;

    complete)
      log "=== Task $TASK_NAME complete ==="
      touch "$WORKTREE/.done"
      exit 0
      ;;

    *)
      log "!!! Unknown status: $status — aborting"
      touch "$WORKTREE/.done"
      exit 1
      ;;
  esac
done

log "!!! Max rounds ($MAX_ROUNDS) reached without completion for $TASK_NAME"
touch "$WORKTREE/.done"
exit 1
