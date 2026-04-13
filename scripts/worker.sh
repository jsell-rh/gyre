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

log() {
  echo "[$(date '+%H:%M:%S')] [$TASK_NAME] $*" >> "$WORKTREE/.worker.log"
  echo "[$(date '+%H:%M:%S')] [$TASK_NAME] $*" >> /tmp/gyre-loop.log
}

get_status() {
  bash scripts/task-field.sh "$TASK_FILE" progress 2>/dev/null || echo "unknown"
}

inject_task_prompt() {
  local prompt_file="$1" task_file="$2"
  cat "$prompt_file"
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
      # Rebase onto the explorer worktree root to pick up merged work from other workers
      log "--- Rebasing onto explorer HEAD (round $ROUND)"
      EXPLORER_ROOT="$(cd "$WORKTREE/../.." && pwd)"
      git fetch "$EXPLORER_ROOT" HEAD 2>/dev/null && git rebase FETCH_HEAD 2>/dev/null || \
        log "!!! Rebase failed (may need manual resolution)"

      log ">>> Implementation (round $ROUND, status=$status)"
      inject_task_prompt specs/prompts/implementation.md "$TASK_FILE" | \
        claude --model opus[1m] --dangerously-skip-permissions 2>/dev/null
      log "<<< Implementation done (exit=$?)"
      log "    Last commit: $(git log --oneline -1 2>/dev/null)"
      ;;

    ready-for-review)
      log ">>> Verifier (round $ROUND)"
      inject_task_prompt specs/prompts/verifier.md "$TASK_FILE" | \
        claude --model opus[1m] --dangerously-skip-permissions 2>/dev/null
      log "<<< Verifier done (exit=$?)"
      log "    Last commit: $(git log --oneline -1 2>/dev/null)"

      # If verifier found issues, run process revision
      new_status=$(get_status)
      if [ "$new_status" = "needs-revision" ]; then
        log ">>> Process Revision (round $ROUND)"
        claude --model opus[1m] --dangerously-skip-permissions \
          < specs/prompts/process-revision.md 2>/dev/null
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
