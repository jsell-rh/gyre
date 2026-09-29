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

# Agent runner: omp in headless print mode with JSON event stream.
# Raw events -> .agent.jsonl (dashboard reads this live);
# formatted progress -> .agent.log (human-readable, also in the pane via tee).
# GYRE_MODEL overrides the model (fuzzy match, e.g. "opus").
OMP=${OMP:-omp}
run_agent() {
  local model_args=()
  if [ -n "${GYRE_MODEL:-}" ]; then
    model_args=(--model "$GYRE_MODEL")
  fi
  "$OMP" -p --no-session --approval-mode yolo --mode json "${model_args[@]}" \
    2>/dev/null \
    | tee "$WORKTREE/.agent.jsonl" \
    | node "$(git rev-parse --show-toplevel 2>/dev/null || echo .)/scripts/fmt-omp-jsonl.mjs" \
      | tee "$WORKTREE/.agent.log"
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
      # Rebase onto the main repo HEAD to pick up merged work from other
      # workers. --autostash: workers' trees are routinely dirty (build.rs
      # regenerates web/dist with new content-hash names on every cargo run;
      # agents write .agent.jsonl/.agent.log), and rebase refuses to start on
      # a dirty tree. If the rebase stops on real conflicts, hand the
      # conflicted state to a rebase-resolver agent instead of aborting —
      # "continue on stale base" lets the worker drift further from main
      # every round (task-087: 16-hunk otlp_receiver.rs conflict, ~70
      # commits behind, wedged at max rounds).
      log "--- Rebasing onto main HEAD (round $ROUND)"
      MAIN_ROOT="$(git rev-parse --git-common-dir 2>/dev/null | sed 's|/\.git$||')"
      if [ -n "$MAIN_ROOT" ] && git fetch "$MAIN_ROOT" HEAD 2>/dev/null && git rebase --autostash FETCH_HEAD 2>/dev/null; then
        :
      elif [ -n "$MAIN_ROOT" ] && git rev-parse -q --verify REBASE_HEAD 2>/dev/null; then
        # Rebase started but stopped on conflict — resolve via agent
        log ">>> Rebase resolver (round $ROUND)"
        inject_task_prompt specs/prompts/rebase-resolver.md "$TASK_FILE" | \
          run_agent 2>/dev/null
        log "<<< Rebase resolver done (exit=$?)"
        if git rev-parse -q --verify REBASE_HEAD >/dev/null 2>&1; then
          # Agent failed to complete the rebase — abort to a safe state
          log "!!! Rebase unresolved after resolver agent — aborting to pre-rebase base"
          git rebase --abort 2>/dev/null || true
        fi
      else
        # Fetch itself failed (main worktree gone?) — nothing to rebase onto
        log "!!! Rebase failed (no fetch target — continuing on current base)"
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
