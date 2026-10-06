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
mkdir -p /tmp/.omp/agent
cp /tmp/stage/models.yml /tmp/stage/config.yml /tmp/.omp/agent/
git config --global user.name gyre-dev-controller
git config --global user.email jsell-rh@users.noreply.github.com
git config --global --add safe.directory '*'
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
  rounds="${GYRE_DEV_WORKER_ROUNDS:-6}"
  [[ "$rounds" =~ ^[1-9][0-9]*$ ]] || exit 2
  for round in $(seq 1 "$rounds"); do
    set +e
    bash /tmp/stage/dev-round.sh "$TASK"
    worker_rc=$?
    set -e
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
    node - "specs/tasks/$TASK.md" <<'JS'
const fs = require('fs');
const { execFileSync } = require('child_process');
const [file] = process.argv.slice(2);
const text = fs.readFileSync(file, 'utf8');
const parts = text.split('---');
if (parts.length < 3) throw new Error('missing task frontmatter');
const lines = parts[1].split('\n');
let start = lines.findIndex(line => /^commits:/.test(line));
if (start < 0) { start = lines.length; lines.push('commits: []'); }
let end = start + 1;
while (end < lines.length && !/^[a-z_][\w-]*:/.test(lines[end])) end++;
const hashes = [...lines.slice(start, end).join('\n').matchAll(/\b[0-9a-f]{7,40}\b/g)].map(m => m[0]);
const branch = execFileSync('git', ['log', '--no-merges', '--abbrev=8', '--format=%h', 'origin/main..HEAD'], {encoding:'utf8'}).trim().split('\n').filter(Boolean);
for (const sha of branch) if (!hashes.includes(sha)) hashes.push(sha);
lines.splice(start, end - start, `commits: [${hashes.map(h => JSON.stringify(h)).join(', ')}]`);
parts[1] = lines.join('\n');
if (!parts[1].endsWith('\n')) parts[1] += '\n';
fs.writeFileSync(file, parts.join('---'));
JS
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
    progress=$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)
    echo "round=$round worker_exit=$worker_rc progress=$progress pushed=$(git rev-parse HEAD)"
    [ "$progress" = complete ] && break
  done
  # A successful push preserves partial work; the controller decides whether
  # this is a candidate or a seed for the next attempt.
elif [ "$MODE" = check ]; then
  CANDIDATE=$ARG1 BASE=$ARG2
  [[ "$CANDIDATE" =~ ^[a-f0-9]{40}$ && "$BASE" =~ ^[a-f0-9]{40}$ ]] || exit 2
  [[ "$ARG3" =~ ^[a-f0-9]{16}$ ]] || exit 2
  git fetch --quiet origin "$CANDIDATE" "$BASE"
  git checkout -q --detach "$BASE"
  git merge --no-ff --no-edit "$CANDIDATE"
  echo "GYRE_BOOTSTRAP_COMPLETE task=$TASK"
  INTEGRATION=$(git rev-parse HEAD)
  [ "$(git rev-parse HEAD^1)" = "$BASE" ]
  git merge-base --is-ancestor "$CANDIDATE" HEAD
  [ "$(bash scripts/task-field.sh "specs/tasks/$TASK.md" progress)" = complete ]
  bash /tmp/stage/dev-check.sh
  # A separate agent reviews the exact integration tree after deterministic gates.
  verdict_file=/tmp/stage/integration-verdict.txt
  { cat /tmp/stage/dev-integration-review.md; printf '\nTask: %s\nBase SHA: %s\nCandidate SHA: %s\n' "$TASK" "$BASE" "$CANDIDATE"; } \
    | omp -p --no-session --mode=json --approval-mode yolo \
    | node /tmp/stage/dev-stream.mjs integration-review "$verdict_file"
  [ "$(tail -n 1 "$verdict_file")" = 'VERDICT: PASS' ]
  [ "$(git rev-parse HEAD)" = "$INTEGRATION" ]
  [ "$(git rev-parse HEAD^1)" = "$BASE" ]
  git diff --quiet HEAD
  [ -z "$(git ls-files --others --exclude-standard)" ]
  # Push a ref named for this exact merge SHA; controller validates both parents.
  MERGE=$(git rev-parse HEAD)
  git push origin "$MERGE:refs/heads/devloop/verified/$ARG3"
  echo "verified integration $MERGE"
else
  exit 2
fi
