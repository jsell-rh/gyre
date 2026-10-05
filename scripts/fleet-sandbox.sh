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

# Coverage weight of a task: how many task-assigned coverage rows name it.
# With more eligible tasks than sandbox slots, prefer the task whose
# completion flips the most coverage rows (goal: spec coverage %).
coverage_weight() {
  local task_name="$1" rows
  rows=$(grep -l "task-assigned" "$REPO_ROOT"/specs/coverage/system/*.md 2>/dev/null \
    | xargs -r grep -c "$task_name\b" 2>/dev/null | awk -F: '{s+=$2} END{print s+0}')
  echo "$rows"
}

find_eligible_tasks() {
  local f
  # needs-revision first (resume pushed branch work), then dep-satisfied
  # not-started. Within each bucket: highest coverage weight first.
  local -a rev_files=() new_files=()
  for f in "$REPO_ROOT"/specs/tasks/task-*.md; do
    [ -f "$f" ] || continue
    case "$(get_progress "$f")" in
      needs-revision) rev_files+=("$f");;
      not-started) deps_satisfied "$f" && new_files+=("$f");;
    esac
  done
  for f in "${rev_files[@]:-}" "${new_files[@]:-}"; do
    [ -n "$f" ] && echo "$(coverage_weight "$(basename "$f" .md)") $f"
  done | sort -rn | awk '{sub(/^[0-9]+ /,""); print}'
}

# --- Dispatch + reap ------------------------------------------------------
spawn_sandbox_worker() {
  local task_file="$1" task_name pid
  task_name=$(basename "$task_file" .md)
  mkdir -p "$STATUS_DIR/$task_name"
  # Each worker driver is its own process; TOTAL_ROUNDS is per-task budget.
  # setsid: the driver gets its own session, so a fleet stop/restart
  # (process-group signal) can't cascade SIGTERM into the driver — its
  # EXIT trap would delete a healthy sandbox mid-round. nohup alone only
  # shields SIGHUP; drivers must survive fleet restarts (their state is
  # on the remote worker branch, so a fleet restart must be non-event).
  setsid nohup bash "$REPO_ROOT/scripts/worker-sandbox.sh" "$task_file" \
    >> "$STATUS_DIR/$task_name/driver.log" 2>&1 &
  pid=$!
  ACTIVE_TASKS[$task_name]=$pid
  log ">>> Spawned sandbox worker for $task_name (pid $pid)"
}

reap_workers() {
  # Remove finished drivers from tracking.
  local task_name pid rc
  for task_name in "${!ACTIVE_TASKS[@]}"; do
    pid=${ACTIVE_TASKS[$task_name]}
    if ! kill -0 "$pid" 2>/dev/null; then
      unset "ACTIVE_TASKS[$task_name]"
      rc=0
      wait "$pid" 2>/dev/null || rc=$?
      # `wait` on an adopted pid (not our child) yields 127 — meaningless,
      # the driver's own rc is in its driver.log. Only report real children.
      if [ "$rc" -ne 127 ]; then
        log "<<< Driver for $task_name exited (rc=$rc)"
      else
        log "<<< Driver for $task_name exited (adopted; rc in driver.log)"
      fi
    fi
  done
}

merge_completed() {
  # Fetch remote worker branches; merge any whose task is complete.
  # Scan ALL remote worker branches — not just ACTIVE_TASKS. A branch can
  # reach `complete` from a driver the fleet no longer tracks (killed
  # driver, fleet restart, adoption gap); the branch on origin is the
  # source of truth and must merge regardless of local tracking state.
  git fetch -q origin '+refs/heads/worker/*:refs/remotes/origin/worker/*' 2>/dev/null
  local branch task_name status
  while IFS= read -r branch; do
    [ -z "$branch" ] && continue
    task_name=${branch#origin/worker/}
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
  done < <(git for-each-ref --format='%(refname:short)' refs/remotes/origin/worker/ 2>/dev/null)
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

# --- Serial auditor (coverage matrix maintainer) --------------------------
# loop.sh ran the spec-fidelity auditor as a serial agent on main; the
# sandbox fleet originally dropped that step, so coverage rows never
# flipped after merges. Runs locally (omp), one group per run, only when
# code landed on main since the last audit. Never overlaps a merge: the
# merge above completed (or found nothing) before this runs.
OMP_BIN="${OMP_BIN:-$(command -v omp || echo /home/linuxbrew/.linuxbrew/Cellar/omp/18.0.4/bin/omp)}"
run_auditor() {
  # The auditor commits into the shared checkout's HEAD. If the checkout
  # is not on main (human mid-task, leftover rewrite branch), those commits
  # strand on the wrong branch — seen once when a task-092-rewrite branch
  # was checked out during an auditor run. Refuse to run off-main instead.
  local current_branch
  current_branch=$(git branch --show-current 2>/dev/null)
  if [ "$current_branch" != "main" ]; then
    log "!!! auditor skipped: checkout on '$current_branch', not main"
    return 1
  fi
  local last_audit_sha code_changes_since
  last_audit_sha=$(git log -1 --format=%H --grep='audit(' 2>/dev/null || true)
  code_changes_since=0
  if [ -n "$last_audit_sha" ]; then
    code_changes_since=$(git log "$last_audit_sha"..HEAD --oneline \
      --invert-grep --grep='audit(\|^merge:' 2>/dev/null | wc -l)
  else
    code_changes_since=1  # no audit yet — run it
  fi
  [ "$code_changes_since" -eq 0 ] && return 0
  log ">>> Spec-Fidelity Auditor ($code_changes_since code change(s) since last audit)"
  # Working-tree guard: the auditor commits its own edits; stash unrelated
  # local dirt around the run (same pattern as loop.sh wt_guard).
  local guarded=0
  if ! git diff --quiet --ignore-submodules -- 2>/dev/null || \
     ! git diff --cached --quiet --ignore-submodules -- 2>/dev/null || \
     [ -n "$(git ls-files --others --exclude-standard)" ]; then
    git stash push --include-untracked --message "fleet-wt-guard" >/dev/null 2>&1 && guarded=1
  fi
  cat "$REPO_ROOT/specs/GOAL.md" "$REPO_ROOT/specs/prompts/spec-fidelity-auditor.md" \
    | timeout 5400 "$OMP_BIN" -p --no-session --approval-mode yolo >/dev/null 2>&1
  local rc=$?
  [ "$guarded" = 1 ] && git stash pop >/dev/null 2>&1
  if [ "$rc" -eq 0 ] && ! git diff --quiet HEAD -- specs/coverage 2>/dev/null; then
    git add specs/coverage 2>/dev/null
    # Auditor normally commits itself; safety net if it forgot.
    git commit -q -m "audit(coverage): fleet serial auditor safety-net commit" --no-verify 2>/dev/null || true
  fi
  git push origin main >/dev/null 2>&1 || true
  log "<<< Auditor done (rc=$rc)"
}

while true; do
  read_parallelism
  adopt_orphans
  reap_workers

  # Merge any completed work into local main + push.
  merge_completed

  # Audit coverage after merges (serial, on main, only when code landed).
  run_auditor

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
