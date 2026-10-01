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
# Open WITHOUT truncation: a stray `bash scripts/loop.sh` from an agent or
# user would otherwise blank the PID line the running loop wrote (flock
# correctly refuses the second instance, but `>` truncates BEFORE flock
# runs — observed in the wild). Append mode keeps the existing content.
if command -v flock >/dev/null 2>&1; then
  LOCK_FD=9
  exec 9>>"$LOCK_FILE"
  flock -n 9 || { echo "ERROR: another loop is already running (lock: $LOCK_FILE). Aborting." >&2; exit 1; }
  : >"$LOCK_FILE"   # we hold the lock — NOW it is safe to reset content
  echo $$ >&9   # PID for liveness probes (dashboard); flock ignores content
else
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
    # --include-untracked (without --all) respects .gitignore, so ignored
    # build dirs (target-task095/ et al. via /target-*/ in .gitignore) stay
    # on disk instead of being swept into the stash — observed orphaning
    # ~500 MB / 11k artifact files twice when the loop died mid-run. Do NOT
    # add a pathspec here: pathspec-mode stash includes ignored files too.
    git stash push --include-untracked --message "loop-wt-guard" >/dev/null 2>&1 && WT_GUARDED=1
  fi
}

wt_guard_restore() {
  if [ "${WT_GUARDED:-0}" -eq 1 ]; then
    if git stash pop >/dev/null 2>&1; then
      return 0
    fi


    # Failed pop is never data loss: the stash commits hold the full WIP.
    # Two failure modes, both observed in the wild:
    #  1. mid-pop merge conflict (unmerged files, stash kept)
    #  2. untracked collision: the agent (e.g. auditor cargo-building web/dist,
    #     or committing a file the WIP also added) recreates a path in the
    #     stash's untracked arm — "already exists, no checkout", stash kept,
    #     tracked part partially applied.
    # Recovery: abort any in-progress merge, then restore ONLY the paths the
    # stash recorded (tracked arm, then untracked arm), unstage, drop the
    # stash. Path-scoped, never a bare `-- .`: agents run on main between
    # stash and restore, and the stash-time snapshot contains their committed
    # work in pre-commit state — checking out the full snapshot would revert
    # those commits in the working tree (observed in the wild).
    git merge --abort >/dev/null 2>&1
    # Clear unmerged/partially-applied pop state in the index WITHOUT touching
    # the working tree, so the path checkouts below are accepted (checkout
    # refuses unmerged paths, which would leave conflict markers behind).
    git reset -q
    local _paths
    # Tracked arm: paths the stash changed vs its base. `stash show` handles
    # the stash commit's multi-parent shape; bare `diff-tree <stash>` on a
    # multi-parent commit diffs parents against each other and returns junk.
    # Modified and deleted paths are handled separately: deleted paths have
    # no entry in the stash tree, and one bad pathspec fails the whole
    # checkout, which would fall through to rm-ing the modified WIP.
    local _mods="" _dels="" _st
    while IFS=$'\t' read -r _st _paths; do
      case "$_st" in
        D) _dels="$_dels $_paths" ;;
        *) _mods="$_mods $_paths" ;;
      esac
    done <<EOF
$(git stash show --name-status "stash@{0}")
EOF
    [ -n "${_mods# }" ] && git checkout "stash@{0}" -- $_mods >/dev/null 2>&1
    [ -n "${_dels# }" ] && git rm -q --ignore-unmatch -- $_dels >/dev/null 2>&1
    # Untracked arm (third parent): restore untracked WIP files.
    if git rev-parse -q --verify "stash@{0}^3" >/dev/null 2>&1; then
      _paths="$(git ls-tree -r --name-only "stash@{0}^3")"
      [ -n "$_paths" ] && git checkout "stash@{0}^3" -- $_paths >/dev/null 2>&1
    fi
    # Unstage: restore the WIP to its original unstaged state.
    git reset -q
    git stash drop >/dev/null 2>&1
    log "WIP restore failed mid-pop (loop-wt-guard); recovered stash content by checkout"
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
  if [ "${DISK_GUARD_SOFT:-0}" -eq 1 ]; then
    log "    Disk guard: deferring spawn of $task_name (low disk)"
    return 1
  fi
  local worktree="$WORKTREE_BASE/$task_name"
  # Orphan adoption: a worker from a previous loop instance may still be
  # running in this worktree (loop restarts don't kill workers, and the
  # loop's own EXIT trap only detaches worktrees that are registered in
  # ACTIVE_WORKERS). Never steal or remove its worktree — register it so
  # the normal merge/monitor cycle picks it up when it finishes.
  if pgrep -f "$worktree" >/dev/null 2>&1; then
    log "    Worker for $task_name already running (orphan from prior loop) — adopting"
    ACTIVE_WORKERS[$task_name]="$worktree"
    return 0
  fi

  # Drop registrations for worktree directories that no longer exist on
  # disk (e.g. after a crash or manual cleanup). Without this, a prunable
  # registration blocks both the branch delete below and worktree add —
  # the task is then wedged forever, failing every cycle.
  git worktree prune

  # Clean up a stale worktree directory if it exists (orphaned by a crash
  # or a previous loop's cleanup). Commits live on the branch, not the dir.
  if [ -d "$worktree" ]; then
    git worktree remove "$worktree" --force 2>/dev/null || rm -rf "$worktree"
  fi

  # If a previous run's branch carries unmerged commits, keep them: create
  # the worktree on that branch and let the worker's rebase round (with the
  # rebase-resolver agent) reconcile it against main HEAD. Only branches
  # with no unique commits are discarded. This makes loop restarts safe —
  # in-flight work resumes instead of being deleted by `git branch -D`.
  if git rev-parse -q --verify "refs/heads/worker/$task_name" >/dev/null 2>&1 && \
     git cherry HEAD "worker/$task_name" 2>/dev/null | grep -q '^+'; then
    log "    Reusing branch worker/$task_name (unmerged commits from a prior run)"
    if ! git worktree add "$worktree" "worker/$task_name" 2>/dev/null; then
      log "!!! Failed to create worktree for $task_name"
      return 1
    fi
  else
    git branch -D "worker/$task_name" 2>/dev/null
    if ! git worktree add "$worktree" -b "worker/$task_name" HEAD 2>/dev/null; then
      log "!!! Failed to create worktree for $task_name"
      return 1
    fi
  fi

  # Execute a frozen COPY of the worker script, not
  # $REPO_ROOT/scripts/worker.sh. Bash pre-parses the whole `while…done`
  # loop at parse time but reads the epilogue (after `done`) from a byte
  # offset — if the main script is rewritten mid-run (e.g. committed fixes
  # while workers are in-flight), bash re-reads from a stale offset in the
  # now-longer file and the worker dies silently at loop exit (no epilogue
  # log, no .done, tmux window just vanishes). The copy is written once at
  # spawn time under /tmp (outside the worktree so worker agents can't
  # `git add -A` it into a commit) and never rewritten, so workers are
  # immune to mid-run edits of the main script.
  local worker_copy="/tmp/gyre-worker-$task_name.sh"
  cp "$REPO_ROOT/scripts/worker.sh" "$worker_copy" && chmod +x "$worker_copy"

  log ">>> Spawning worker: $task_name (worktree: $worktree)"

  # Ensure the loop's tmux session exists (new-window needs a running server)
  if command -v tmux >/dev/null 2>&1; then
    tmux has-session -t "$TMUX_SESSION" 2>/dev/null || \
      tmux new-session -d -s "$TMUX_SESSION" "exec sleep infinity"
  fi
  if command -v tmux >/dev/null 2>&1 && \
     tmux new-window -t "$TMUX_SESSION" -n "$task_name" \
       "bash '/tmp/gyre-worker-$task_name.sh' '$task_file' '$worktree'; echo 'Worker $task_name exited'; sleep 5"; then
    :
  else
    # No tmux (or window spawn failed) — run the worker as a background
    nohup bash "/tmp/gyre-worker-$task_name.sh" "$task_file" "$worktree" \
      > "$worktree/.spawn.log" 2>&1 &
    log "    (tmux unavailable — backgrounding worker)"
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
      # Keep the branch: the respawn path in spawn_worker reuses branches
      # with unmerged commits ("Reusing branch worker/$task_name"), so the
      # next cycle's worker re-rebases onto the new main and retries the
      # merge. Deleting the branch here would lose all the work.
      log "    Merge aborted for $task_name — branch kept for retry via respawn"
      git worktree remove "$worktree" --force 2>/dev/null
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

# --- Disk guard ---
# The 2026-09-30 crash: six worker cargo target/ dirs (88G) + caches filled
# the root fs to 100%; the auditor omp died on SIGBUS (mmap write, ENOSPC),
# taking the loop shell with it. Prevent recurrence:
#   - DISK_SOFT_FREE_PCT (default 5 = 95% used): stop spawning new
#     workers; sweep target dirs of NON-running workers (rm -rf is safe:
#     cargo rebuilds them).
#   - DISK_HARD_FREE_PCT (default 3 = 97% used): sleep the loop (no agents
#     at all) until space recovers — agents on a full disk die mid-write.
# Both are advisory gates ahead of ENOSPC, checked every cycle.
DISK_SOFT_FREE_PCT=${GYRE_DISK_SOFT_FREE_PCT:-5}
DISK_HARD_FREE_PCT=${GYRE_DISK_HARD_FREE_PCT:-3}
disk_avail_pct() {
  # Print free-space percentage of the filesystem holding the repo (integer).
  df -P "$REPO_ROOT" | awk 'NR==2 { gsub("%","",$5); print 100-$5 }'
}
sweep_dead_target_dirs() {
  # rm -rf cargo target dirs of workers with no live process in their
  # worktree. NEVER touch a dir belonging to a running worker — rm -rf on
  # a live agent's build dir kills its cargo mid-write.
  local wt pct_used avail_after
  for wt in "$WORKTREE_BASE"/task-*/; do
    [ -d "$wt/target" ] || continue
    if pgrep -f "$wt" >/dev/null 2>&1; then
      log "    Disk sweep: skipping ${wt}target (worker live)"
      continue
    fi
    rm -rf "$wt/target" 2>/dev/null \
      && log "    Disk sweep: removed ${wt}target"
  done
  # Stray /target-* dirs at repo root (orphaned CARGO_TARGET_DIR from dead
  # workers; /target-*/ is gitignored).
  for wt in "$REPO_ROOT"/target-*/; do
    [ -d "$wt" ] || continue
    rm -rf "$wt" 2>/dev/null \
      && log "    Disk sweep: removed stray $wt"
  done
}
disk_guard() {
  local avail
  avail=$(disk_avail_pct)
  if [ "$avail" -lt "$DISK_HARD_FREE_PCT" ]; then
    log "!!! Disk guard: only ${avail}% free (< $DISK_HARD_FREE_PCT%) — pausing loop until space recovers"
    sweep_dead_target_dirs
    while [ "$(disk_avail_pct)" -lt "$DISK_HARD_FREE_PCT" ]; do
      sleep 60
    done
    log "    Disk guard: recovered, resuming"
    return 0
  fi
  if [ "$avail" -lt "$DISK_SOFT_FREE_PCT" ]; then
    log "!!! Disk guard: only ${avail}% free (< $DISK_SOFT_FREE_PCT%) — sweeping dead target dirs, no new spawns this cycle"
    sweep_dead_target_dirs
    return 1
  fi
  return 0
}

cleanup_all() {
  log "Loop exiting — detaching worktrees (branches preserved for recovery)"
  local task_name worktree live
  for task_name in "${!ACTIVE_WORKERS[@]}"; do
    worktree="${ACTIVE_WORKERS[$task_name]}"
    # Never delete a worktree that still has a live worker process in it —
    # rm -rf on a live agent's cwd kills it mid-run (observed: task-106's
    # round-5 verifier had its worktree deleted under it by this path).
    # Orphan the worker instead; a restarted loop adopts it via the
    # orphan-adoption guard in spawn_worker.
    live=$(pgrep -f "$worktree" | head -1)
    if [ -n "$live" ]; then
      log "    Worker for $task_name still live (pid $live) — orphaning, NOT deleting its worktree"
      unset "ACTIVE_WORKERS[$task_name]"
      continue
    fi
    git worktree remove "$worktree" --force 2>/dev/null
    # Intentionally NOT deleting worker branches — commits are preserved
    log "    Detached worktree for $task_name (branch worker/$task_name intact)"
  done
  # Only sweep the base dir if no worker process remains anywhere under it
  if ! pgrep -f "$WORKTREE_BASE" >/dev/null 2>&1; then
    rm -rf "$WORKTREE_BASE" 2>/dev/null
  fi
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

# Pre-flight verdicts and process revisions commit directly to main —
# publish them so remote is not stale while workers churn.
git push origin main >/dev/null 2>&1 \
  && log "Pre-flight: pushed main to origin" \
  || log "Pre-flight: push to origin failed (will retry)"


ITERATION=0
while true; do
  ITERATION=$((ITERATION + 1))
  log "--- Orchestrator cycle $ITERATION (${#ACTIVE_WORKERS[@]} active workers) ---"

  # Disk guard: hard pause when nearly full; soft gate stops new spawns.
  # Live workers continue their rounds either way (their commits are on
  # branches; only NEW agents are gated).
  DISK_GUARD_SOFT=0
  disk_guard || DISK_GUARD_SOFT=1

  # Self-heal the lock PID: agents occasionally truncate /tmp/gyre-loop.lock
  # (or /tmp cleanups wipe it). Rewrite our PID each cycle so the dashboard's
  # liveness probe recovers instead of reading a blank file forever.
  [ -n "${LOCK_FD:-}" ] && echo $$ >&"$LOCK_FD"

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

  # 5. Publish: one push per cycle covers auditor/PM/merge/pre-flight commits
  git push origin main >/dev/null 2>&1 \
    && log "Cycle: pushed main to origin" \
    || log "Cycle: push to origin failed (will retry next cycle)"

  # 6. Status report
  active=${#ACTIVE_WORKERS[@]}
  log "    Active workers: $active"
  for task_name in "${!ACTIVE_WORKERS[@]}"; do
    log "      - $task_name"
  done

  # 7. Check convergence
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
