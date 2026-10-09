#!/usr/bin/env bash
# Runs inside a fresh OpenShell sandbox. Args are all controller generated.
set -euo pipefail
MODE=$1 TASK=$2 ARG1=$3 ARG2=$4 ARG3=${5:-}
exec 9>/tmp/stage/attempt.lock
flock 9
if [ -f /tmp/stage/attempt.complete ]; then
  echo "sandbox attempt already completed"
  exit 0
fi
mark_complete() {
  local result=$?
  if [ "$result" -eq 0 ]; then touch /tmp/stage/attempt.complete; fi
}
trap mark_complete EXIT
mkdir -p /tmp/stage/bin
for build_command in cargo npm npx; do
  real_command=$(command -v "$build_command")
  printf -v "GYRE_DEV_REAL_${build_command^^}" '%s' "$real_command"
  export "GYRE_DEV_REAL_${build_command^^}"
  ln -sf ../dev-build-command.sh "/tmp/stage/bin/$build_command"
done
export PATH="/tmp/stage/bin:$PATH"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
mkdir -p /tmp/.omp/agent
cp /tmp/stage/models.yml /tmp/stage/config.yml /tmp/.omp/agent/
git config --global user.name gyre-dev-controller
git config --global user.email jsell-rh@users.noreply.github.com
git config --global --add safe.directory '*'
# Keep later fetches and lazy blob requests on the same transport as clone.
git config --global http.version HTTP/1.1
# shellcheck disable=SC2016 # The helper expands GITHUB_TOKEN when Git invokes it.
git config --global credential.helper '!f(){ echo username=x-access-token; echo password=$GITHUB_TOKEN; }; f'
if [ ! -d /tmp/gyre/.git ]; then
  for attempt in 1 2 3 4; do
    if git -c http.version=HTTP/1.1 clone --quiet --filter=blob:none --single-branch --branch main \
      "${GYRE_DEV_REPO_URL:-https://github.com/jsell-rh/gyre.git}" /tmp/gyre; then
      break
    fi
    rm -rf /tmp/gyre
    [ "$attempt" -lt 4 ] || { echo "clone failed after four retries in one sandbox" >&2; exit 75; }
    sleep "$((attempt * 5))"
  done
else
  echo "resuming existing sandbox checkout"
fi
cd /tmp/gyre
git fetch --quiet origin main || { echo "bootstrap fetch failed" >&2; exit 75; }
if [ "$MODE" = worker ]; then
  BRANCH=$ARG1 SEED=$ARG2
  case "$BRANCH" in devloop/"$TASK"/attempt-[0-9]*) ;; *) exit 2;; esac
  if [ "$(git branch --show-current)" = "$BRANCH" ]; then
    echo "resuming branch $BRANCH"
  elif git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null; then
    git fetch --quiet origin "$BRANCH" || exit 75
    git checkout -q -b "$BRANCH" FETCH_HEAD
  else
    if [[ "$SEED" =~ ^[a-f0-9]{40}$ ]]; then git fetch --quiet origin "$SEED" || exit 75; fi
    git checkout -q -b "$BRANCH" "$SEED"
  fi
  echo "GYRE_BOOTSTRAP_COMPLETE task=$TASK"
  if [ -f /tmp/stage/task.md ] && [ ! -f "specs/tasks/$TASK.md" ]; then
    cp /tmp/stage/task.md "specs/tasks/$TASK.md"
    git add "specs/tasks/$TASK.md"
    git commit -q -m "process: add scoped task $TASK" --no-verify
  fi
  # A completed seed rejected by integration must actually return to
  # implementation. Task frontmatter alone otherwise skips every agent round.
  if [ ! -f /tmp/stage/worker.initialized ]; then
    if [ -f /tmp/stage/repair.md ]; then
      python3 - "$TASK" <<'PY'
import pathlib, re, sys
path = pathlib.Path(f"specs/tasks/{sys.argv[1]}.md")
parts = path.read_text().split("---", 2)
parts[1], count = re.subn(r"^progress:.*$", "progress: needs-revision", parts[1], flags=re.M)
if count != 1:
    raise SystemExit("task must have exactly one progress field")
path.write_text("---".join(parts))
PY
      rm -f /tmp/stage/review-approved
    elif [ "$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)" = complete ]; then
      # A complete checkpoint still needs a fresh, successful review in this
      # worker before it can be nominated as a new candidate.
      sed -i 's/^progress: complete$/progress: ready-for-review/' "specs/tasks/$TASK.md"
    fi
    touch /tmp/stage/worker.initialized
  fi
  rounds="${GYRE_DEV_WORKER_ROUNDS:-6}"
  [[ "$rounds" =~ ^[1-9][0-9]*$ ]] || exit 2
  inference_failures=0
  for round in $(seq 1 "$rounds"); do
    set +e
    bash /tmp/stage/dev-round.sh "$TASK"
    worker_rc=$?
    set -e
    [ "$worker_rc" -ne 80 ] || { echo 'audit generation changed; requesting a fresh assignment' >&2; exit 80; }
    # A cancelled baseline probe may leave production edits hidden in a stash.
    # Restore only this task branch's work before reading progress/checkpointing.
    python3 /tmp/stage/dev-checkpoint.py "$BRANCH"
    if [ "$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)" = complete ] &&
       { [ "$worker_rc" -ne 0 ] || [ ! -f /tmp/stage/review-approved ]; }; then
      echo "completion has no successful review; returning to review" >&2
      sed -i 's/^progress: complete$/progress: ready-for-review/' "specs/tasks/$TASK.md"
    fi
    # build.rs regenerates committed web/dist during Rust tests. It is a build
    # artifact, not task work, and must not be swept into checkpoint commits.
    git restore --worktree -- web/dist
    git clean -fd -- web/dist
    if ! git diff --quiet HEAD || [ -n "$(git ls-files --others --exclude-standard)" ]; then
      git add -A
      git commit -q -m "wip($TASK): preserve sandbox attempt" --no-verify
    fi
    # Rebasing rewrites task commit IDs. Rebuild attribution from the exact
    # branch range after each round, including commits the agent made itself.
    python3 /tmp/stage/dev-attribution.py "$TASK"
    if ! git diff --quiet -- "specs/tasks/$TASK.md"; then
      git add "specs/tasks/$TASK.md"
      git commit -q -m "process: record $TASK branch commits" --no-verify
    fi
    # A plain --force-with-lease compares against the local remote-tracking
    # ref, which a push does not reliably refresh. The first unchanged rounds
    # can appear to work, then the first real commit is rejected as stale.
    # Pin the lease to the server's exact ref and retry transport failures in
    # this sandbox. Never overwrite a branch that changed under us.
    pushed=0
    expected_remote='unset'
    for push_try in 1 2 3 4; do
      remote_line=$(git ls-remote --heads origin "refs/heads/$BRANCH") || { sleep "$((push_try * 3))"; continue; }
      remote_sha=${remote_line%%[[:space:]]*}
      [ -n "$remote_line" ] || remote_sha=
      if [ "$remote_sha" = "$(git rev-parse HEAD)" ]; then pushed=1; break; fi
      if [ "$expected_remote" = unset ]; then
        expected_remote=$remote_sha
        printf '%s\n' "$remote_sha" > /tmp/stage/push-expected
      elif [ "$remote_sha" != "$expected_remote" ]; then
        echo "branch changed during push; refusing to overwrite $BRANCH" >&2
        exit 76
      fi
      if git push --force-with-lease="refs/heads/$BRANCH:$remote_sha" origin "HEAD:refs/heads/$BRANCH"; then
        pushed=1
        break
      fi
      sleep "$((push_try * 3))"
    done
    [ "$pushed" -eq 1 ] || { echo "branch push failed after four retries; preserving sandbox work" >&2; exit 76; }
    git rev-parse HEAD > /tmp/stage/push-expected
    progress=$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)
    echo "round=$round worker_exit=$worker_rc progress=$progress pushed=$(git rev-parse HEAD)"
    [ "$progress" = complete ] && break
    if [ "$worker_rc" -eq 82 ]; then
      inference_failures=$((inference_failures + 1))
      delay=$((5 * 2 ** (inference_failures < 5 ? inference_failures - 1 : 4)))
      [ "$delay" -le 60 ] || delay=60
      if [ "$round" -lt "$rounds" ]; then
        echo "inference unavailable; retrying saved session in this sandbox after ${delay}s" >&2
        sleep "$delay"
      fi
    else
      inference_failures=0
    fi
  done
  [ "$progress" = complete ] || [ "$worker_rc" -ne 82 ] || exit 82
  # A successful push preserves partial work; the controller decides whether
  # this is a candidate or a seed for the next attempt.
elif [ "$MODE" = check ]; then
  CANDIDATE=$ARG1 BASE=$ARG2
  [[ "$CANDIDATE" =~ ^[a-f0-9]{40}$ && "$BASE" =~ ^[a-f0-9]{40}$ ]] || exit 2
  [[ "$ARG3" =~ ^[a-f0-9]{16}$ ]] || exit 2
  git fetch --quiet origin "$CANDIDATE" "$BASE"
  git checkout -q --detach "$BASE"
  echo "GYRE_BOOTSTRAP_COMPLETE task=$TASK"
  if [ -f /tmp/stage/audit-contract.json ]; then
    python3 /tmp/stage/dev-audit-check.py /tmp/stage/audit-contract.json --generation-only || exit 80
  fi
  git merge --no-ff --no-commit "$CANDIDATE"
  if [ -n "$(git diff --cached --name-only "$BASE" -- specs/coverage)" ]; then
    python3 /tmp/stage/dev-coverage.py
    git add specs/coverage/SUMMARY.md
  fi
  python3 /tmp/stage/dev-merge-message.py "$TASK" "$CANDIDATE" > /tmp/stage/merge-message.txt
  git commit -F /tmp/stage/merge-message.txt
  echo "GYRE_BOOTSTRAP_COMPLETE task=$TASK"
  INTEGRATION=$(git rev-parse HEAD)
  [ "$(git rev-parse HEAD^1)" = "$BASE" ]
  git merge-base --is-ancestor "$CANDIDATE" HEAD
  [ "$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)" = complete ]
  if [ -f /tmp/stage/audit-contract.json ]; then
    python3 /tmp/stage/dev-audit-check.py /tmp/stage/audit-contract.json --base "$BASE" --replay
  fi
  bash /tmp/stage/dev-check.sh
  # A separate agent reviews the exact integration tree after deterministic gates.
  verdict_file=/tmp/stage/integration-verdict.txt
  { cat /tmp/stage/dev-integration-review.md; if [ -f /tmp/stage/audit-contract.json ]; then printf '\nThis is a metadata fidelity audit: verify evidence and follow-up tasks within its contract; product gaps must remain open.\n'; cat /tmp/stage/audit-contract.json; fi; printf '\nTask: %s\nBase SHA: %s\nCandidate SHA: %s\n' "$TASK" "$BASE" "$CANDIDATE"; } \
    | timeout --signal=INT --kill-after=30s "${GYRE_DEV_ROUND_TIMEOUT:-1800}" \
      omp -p --model "${GYRE_DEV_MODEL:-enmaas-glm-5-3/rits/zai-org/glm-5-3}" --no-session --mode=json --approval-mode yolo \
    | node /tmp/stage/dev-stream.mjs integration-review "$verdict_file"
  [ "$(tail -n 1 "$verdict_file")" = 'VERDICT: PASS' ] || {
    echo "integration review did not pass" >&2; exit 1;
  }
  [ "$(git rev-parse HEAD)" = "$INTEGRATION" ]
  [ "$(git rev-parse HEAD^1)" = "$BASE" ]
  # Reviewers may run Rust tests; build.rs regenerates the committed web/dist
  # bundle. Remove that build output before asserting the reviewed tree stayed
  # unchanged. Keep any other changes visible as a hard failure.
  git restore --worktree -- web/dist
  git clean -fd -- web/dist
  if ! git diff --quiet HEAD || [ -n "$(git ls-files --others --exclude-standard)" ]; then
    git status --short >&2
    echo "review changed the integration tree" >&2
    exit 1
  fi
  # Push a ref named for this exact merge SHA; controller validates both parents.
  MERGE=$(git rev-parse HEAD)
  git push origin "$MERGE:refs/heads/devloop/verified/$ARG3" || exit 76
  echo "verified integration $MERGE"
else
  exit 2
fi
