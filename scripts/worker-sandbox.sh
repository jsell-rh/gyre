#!/usr/bin/env bash
# Sandbox worker: dispatch one Gyre task into an OpenShell sandbox.
# Creates a sandbox from the gyre-worker image, clones the repo, runs the
# standard worker loop (scripts/worker.sh) inside it via `openshell sandbox
# exec`, then pushes the worker branch and opens a PR.
#
# Usage: worker-sandbox.sh <task-file>
#
# Requires: openshell 0.1.2, gateway `gyre-gyre` registered, providers
# gyre-pricetag + gyre-github-rw on the gateway (see OPENSHELL-GOAL.md).
set -uo pipefail

TASK_FILE="$1"
TASK_REL=$(basename "$TASK_FILE")
TASK_NAME="${TASK_REL%.md}"
SANDBOX="${SANDBOX:-gyre-wk-${TASK_NAME}}"
IMAGE="${IMAGE:-ghcr.io/jsell-rh/gyre-worker:latest}"
REPO_URL="${REPO_URL:-https://github.com/jsell-rh/gyre.git}"
BASE_BRANCH="${BASE_BRANCH:-main}"

export OPENSHELL_GATEWAY_INSECURE=true OPENSHELL_WORKSPACE=default
OPENSHELL_BIN="${OPENSHELL:-openshell}"

log() {
  echo "[$(date '+%H:%M:%S')] [$TASK_NAME/sandbox] $*" | tee -a /tmp/gyre-sandbox-loop.log >&2
  # Also append to the per-task status dir the dashboard reads
  # (sandbox agents can't write to the local FS; the driver mirrors).
  mkdir -p "${STATUS_DIR:-/tmp/gyre-sandbox/status}/$TASK_NAME" 2>/dev/null
  echo "[$(date '+%H:%M:%S')] $*" >> "${STATUS_DIR:-/tmp/gyre-sandbox/status}/$TASK_NAME/driver.log" 2>/dev/null
}

# publish_status <state> — write a small JSON status file for the dashboard.
# Best-effort: status mirroring must never fail the worker.
publish_status() {
  local state="$1" extra="${2:-}"
  local dir="${STATUS_DIR:-/tmp/gyre-sandbox/status}/$TASK_NAME"
  mkdir -p "$dir" 2>/dev/null || return 0
  printf '{"task":"%s","sandbox":"%s","state":"%s","round":%s,"total_rounds":%s,"updated":"%s","extra":"%s"}\n' \
    "$TASK_NAME" "$SANDBOX" "$state" "${ROUND:-0}" "${TOTAL_ROUNDS:-6}" \
    "$(date -Is)" "$extra" > "$dir/status.json" 2>/dev/null || true
}

# Refresh the 5-minute SA token before each CLI burst.
reauth() {
  OPENSHELL_OIDC_CLIENT_SECRET="${OPENSHELL_OIDC_CLIENT_SECRET:?set OPENSHELL_OIDC_CLIENT_SECRET}" \
    OPENSHELL_NO_BROWSER=1 "$OPENSHELL_BIN" gateway login gyre-gyre >/dev/null 2>&1 || {
    log "!!! gateway login failed"; return 1; }
}

# osexec <script-file>  — run a staged script file in the sandbox.
# Script files avoid nested-quoting bugs in inline `bash -c` payloads.
osexec() {
  reauth
  timeout "${EXEC_TIMEOUT:-7200}" "$OPENSHELL_BIN" -g gyre-gyre sandbox exec \
    -n "$SANDBOX" --no-login-shell --workdir /tmp \
    --env HOME=/tmp \
    --env RUSTUP_HOME=/usr/local/rustup \
    --env CARGO_HOME=/usr/local/cargo \
    -- bash "/tmp/stage/$(basename "$1")"
}

# stage <local-file> [remote-name] — copy a file into the sandbox /tmp/stage.
stage() {
  local src="$1" dst="${2:-$(basename "$1")}"
  reauth
  cat "$src" | timeout 120 "$OPENSHELL_BIN" -g gyre-gyre sandbox exec \
    -n "$SANDBOX" --no-login-shell --workdir /tmp \
    -- bash -c "mkdir -p /tmp/stage && cat > /tmp/stage/$dst" >/dev/null 2>&1
}

log "=== Creating sandbox $SANDBOX (image $IMAGE)"
reauth
timeout 600 "$OPENSHELL_BIN" -g gyre-gyre sandbox create \
  --name "$SANDBOX" \
  --from "$IMAGE" \
  --provider gyre-pricetag \
  --provider gyre-github-rw \
  --policy /tmp/gyre-sandbox/policy.yaml \
  --detach -- bash -c 'while true; do sleep 3600; done' 2>&1 | tail -3

log ">>> Waiting for sandbox Ready"
reauth
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12; do
  phase=$(timeout 60 "$OPENSHELL_BIN" -g gyre-gyre sandbox get "$SANDBOX" 2>/dev/null | grep -m1 'Phase:' || true)
  case "$phase" in *Ready*) break;; esac
  log "    phase: $phase (attempt $attempt)"
  sleep 15
  reauth
done

cleanup() {
  # Delete the sandbox ONLY when the driver finished cleanly (complete, PR
  # opened, or genuinely done). On transient failures (flaky egress clone,
  # relay timeouts) the sandbox is healthy — deleting it forces a 1.6 GB
  # image re-pull per fleet respawn cycle, which is exactly the churn that
  # burned 60+ spawns per task. Let the next driver reuse it (clone resuming
  # the pushed worker branch makes the reuse cheap).
  if [ "$KEEP_SANDBOX_ON_FAILURE" = 1 ] && [ "$?" != 0 ]; then
    log "=== Driver failed — keeping sandbox $SANDBOX for reuse"
    return 0
  fi
  reauth
  "$OPENSHELL_BIN" -g gyre-gyre sandbox delete "$SANDBOX" >/dev/null 2>&1
  log "=== Sandbox $SANDBOX deleted"
}
trap cleanup EXIT
KEEP_SANDBOX_ON_FAILURE=1

# ---- Stage: repo clone, omp config, worker script ----
WORKDIR=$(mktemp -d /tmp/sbxwk-$TASK_NAME-XXXX)

cat > "$WORKDIR/10-clone.sh" <<EOF
set -e
git config --global user.name "gyre-sandbox-agent"
git config --global user.email "jsell-rh@users.noreply.github.com"
git config --global --add safe.directory "*"
git config --global credential.helper '!f(){ echo username=x-access-token; echo password=\$GITHUB_TOKEN; }; f'
rm -rf /tmp/gyre
git clone --quiet $REPO_URL /tmp/gyre
cd /tmp/gyre
git checkout -q worker/$TASK_NAME 2>/dev/null || git checkout -q -b worker/$TASK_NAME origin/worker/$TASK_NAME 2>/dev/null || git checkout -q -b worker/$TASK_NAME origin/$BASE_BRANCH
git log --oneline -1
EOF

# omp config: HOME=/tmp, config at /tmp/.omp/agent (see OPENSHELL-GOAL.md
# gotchas: /opt blocked by default policy; env: not resolved by omp; the
# egress proxy swaps only the profile-declared header, hence X-Api-Key).
MODELS_YML="${MODELS_YML:-/tmp/sbx-models.yml}"
CONFIG_YML="${CONFIG_YML:-$HOME/.pi/agent/config.yml}"
[ -f "$MODELS_YML" ] || { log "!!! $MODELS_YML missing (see OPENSHELL-GOAL.md)"; exit 1; }
[ -f "$CONFIG_YML" ] || { log "!!! $CONFIG_YML missing"; exit 1; }
[ -f "$WORKDIR/10-clone.sh" ] && stage "$WORKDIR/10-clone.sh"

log ">>> Cloning repo into sandbox"
# Egress connections on freshly-provisioned sandboxes intermittently hang
# (observed curl 56 getpeername errno 95; recovers within a minute). A single
# clone attempt would kill the driver — retry with backoff instead.
clone_ok=""
for clone_try in 1 2 3 4 5; do
  osexec "$WORKDIR/10-clone.sh" && { clone_ok=1; break; }
  log "    clone attempt $clone_try failed — retrying in 30s"
  sleep 30
  reauth
  stage "$WORKDIR/10-clone.sh"
done
[ -n "$clone_ok" ] || { log "!!! clone/branch failed after 5 attempts"; exit 1; }

log ">>> Staging omp config + worker scripts"
stage "$MODELS_YML" models.yml
stage "$CONFIG_YML" config.yml
stage scripts/worker.sh worker.sh
stage scripts/task-field.sh task-field.sh
stage scripts/fmt-omp-jsonl.mjs fmt-omp-jsonl.mjs

cat > "$WORKDIR/20-install.sh" <<'EOF'
set -e
mkdir -p /tmp/.omp/agent
mv /tmp/stage/models.yml /tmp/stage/config.yml /tmp/.omp/agent/
cd /tmp/gyre
mkdir -p scripts
cp /tmp/stage/worker.sh /tmp/stage/task-field.sh /tmp/stage/fmt-omp-jsonl.mjs scripts/
# MAX_ROUNDS env-overridable in staged copy (repo copy stays loop-owned)
sed -i "s/^MAX_ROUNDS=6/MAX_ROUNDS=\${MAX_ROUNDS:-6}/" scripts/worker.sh
chmod +x scripts/worker.sh scripts/task-field.sh
ls scripts/
EOF
stage "$WORKDIR/20-install.sh"
osexec "$WORKDIR/20-install.sh" || { log "!!! install failed"; exit 1; }

# ---- Run the worker loop inside the sandbox, one round per exec ----
# Why round-per-exec: a k8s sandbox can die mid-run (phase Error — observed
# after 1h43m during a cargo build). Pushing after every round preserves
# work-in-progress on the remote branch, and if the sandbox dies we recreate
# it and resume from the pushed branch instead of losing everything.
cat > "$WORKDIR/30-run.sh" <<EOF
cd /tmp/gyre
export MAX_ROUNDS=\${MAX_ROUNDS_OVERRIDE:-1}
bash scripts/worker.sh specs/tasks/$TASK_REL /tmp/gyre
status=\$(bash scripts/task-field.sh specs/tasks/$TASK_REL progress 2>/dev/null)
echo "WORKER-STATUS=\$status"
EOF
stage "$WORKDIR/30-run.sh"

cat > "$WORKDIR/35-resume.sh" <<EOF
set -e
cd /tmp/gyre
git fetch -q origin worker/$TASK_NAME 2>/dev/null || true
git checkout -q worker/$TASK_NAME 2>/dev/null || git checkout -q -b worker/$TASK_NAME origin/worker/$TASK_NAME
git reset -q --hard origin/worker/$TASK_NAME 2>/dev/null || true
git log --oneline -1
EOF
stage "$WORKDIR/35-resume.sh"

cat > "$WORKDIR/40-push.sh" <<EOF
cd /tmp/gyre
# Safety net: agents routinely end their session with edits in the working
# tree but no commit — a round would then push nothing and the work would be
# lost on sandbox recreation. Commit any dirty tree before deciding.
if ! git diff --quiet HEAD 2>/dev/null || [ -n "\$(git ls-files --others --exclude-standard)" ]; then
  git add -A
  git commit -q -m "wip($TASK_NAME): round auto-commit (uncommitted agent work)" --no-verify || true
fi
if git diff --quiet origin/$BASE_BRANCH..HEAD 2>/dev/null; then
  echo NO-CHANGES
else
  out=""
  for try in 1 2 3; do
    out=\$(git push --force-with-lease origin worker/$TASK_NAME 2>&1) && break
    # Worker branches are single-owner; rebases onto main make plain pushes
    # non-fast-forward. Retry with lease first, force on stale-tip races.
    out=\$(git push --force origin worker/$TASK_NAME 2>&1) && break
  done
  if [ -z "\$out" ] || ! echo "\$out" | grep -q 'fatal\|error\|rejected\|Could not'; then
    echo PUSHED
  else
    echo "\$out" | tail -2; echo PUSH-FAILED
  fi
fi
EOF
stage "$WORKDIR/40-push.sh"

ensure_sandbox() {
  reauth
  # Phase can briefly read Provisioning/Updating after a provider/policy
  # revision even though the sandbox is fine — retry before recreating.
  local attempt phase
  for attempt in 1 2 3 4 5; do
    phase=$(timeout 60 "$OPENSHELL_BIN" -g gyre-gyre sandbox get "$SANDBOX" 2>/dev/null | grep -m1 'Phase:' || true)
    case "$phase" in *Ready*) return 0;; esac
    [ "$attempt" = 5 ] && break
    log "    sandbox phase '$phase' (attempt $attempt) — waiting 15s"
    sleep 15
    reauth
  done
  log "!!! Sandbox not Ready — recreating $SANDBOX"
  local create_try
  for create_try in 1 2 3; do
    timeout 60 "$OPENSHELL_BIN" -g gyre-gyre sandbox delete "$SANDBOX" >/dev/null 2>&1
    timeout 600 "$OPENSHELL_BIN" -g gyre-gyre sandbox create \
      --name "$SANDBOX" \
      --from "$IMAGE" \
      --provider gyre-pricetag \
      --provider gyre-github-rw \
      --policy /tmp/gyre-sandbox/policy.yaml \
      --detach -- bash -c 'while true; do sleep 3600; done' 2>&1 | tail -2
    if wait_ready; then
      break
    fi
    log "!!! recreate attempt $create_try failed — retrying after 30s"
    sleep 30
    reauth
    [ "$create_try" = 3 ] && { log "!!! recreated sandbox never reached Ready"; return 1; }
  done
    # Restore staged state: clone, resume branch, reinstall configs/scripts.
    # NOTE: 20-install.sh is generated in the bootstrap phase but was never
    # staged here — the recreate path executed a script that did not exist
    # in the sandbox ("bash: /tmp/stage/20-install.sh: No such file or
    # directory"), failing every recreation. Also retry the re-clone: fresh
    # sandboxes have a flaky egress window (see bootstrap clone loop).
    stage "$WORKDIR/10-clone.sh" || return 1
    clone_ok=""
    for clone_try in 1 2 3 4 5; do
      osexec "$WORKDIR/10-clone.sh" >/dev/null && { clone_ok=1; break; }
      log "    re-clone attempt $clone_try failed — retrying in 30s"
      sleep 30
      reauth
    done
    [ -n "$clone_ok" ] || { log "!!! re-clone failed after 5 attempts"; return 1; }
    stage "$MODELS_YML" models.yml
    stage "$CONFIG_YML" config.yml
    stage scripts/worker.sh worker.sh
    stage scripts/task-field.sh task-field.sh
    stage scripts/fmt-omp-jsonl.mjs fmt-omp-jsonl.mjs
    stage "$WORKDIR/20-install.sh"
    stage "$WORKDIR/30-run.sh"
    stage "$WORKDIR/40-push.sh"
    osexec "$WORKDIR/20-install.sh" >/dev/null || return 1
    osexec "$WORKDIR/35-resume.sh" >/dev/null || return 1
    log "    Sandbox recreated, resumed from remote branch"
}

wait_ready() {
  # Poll until the sandbox reports Ready (create's streamed wait is flaky).
  local attempt phase
  for attempt in $(seq 1 40); do
    phase=$(timeout 60 "$OPENSHELL_BIN" -g gyre-gyre sandbox get "$SANDBOX" 2>/dev/null | grep -m1 'Phase:' || true)
    case "$phase" in *Ready*) return 0;; esac
    sleep 15
    reauth
  done
  return 1
}

ROUND=0
publish_status starting
TOTAL_ROUNDS="${TOTAL_ROUNDS:-6}"
for round in $(seq 1 "$TOTAL_ROUNDS"); do
  ROUND=$round
  publish_status "round-$round" "starting"
  log ">>> Worker round $round/$TOTAL_ROUNDS"
  ensure_sandbox || { publish_status error "sandbox unrecoverable"; exit 1; }
  publish_status "round-$round" "agent running"
  # LIVE STREAM: a long-lived `sandbox exec -- tail -f` (the gateway's
  # exec channel streams stdout incrementally — verified) pipes the agent
 # log + worker log from inside the sandbox into a local relay file. The
  # dashboard SSE-streams that file to the browser. Killed at round end.
  cat > "$WORKDIR/70-live-tail.sh" <<'MIR'
cd /tmp/gyre 2>/dev/null || exit 1
echo "===STREAM-OPEN==="
# Follow both logs: worker loop log line-buffered, agent log from offset 0.
tail -n 200 -F /tmp/gyre-loop.log /tmp/gyre/.agent.log 2>/dev/null
MIR
  stage "$WORKDIR/70-live-tail.sh" 2>/dev/null || true
  STREAM_PID=""
  start_stream() {
    [ -n "$STREAM_PID" ] && return 0
    reauth
    ( reauth
      exec timeout 14400 "$OPENSHELL_BIN" -g gyre-gyre sandbox exec \
        -n "$SANDBOX" --no-login-shell --workdir /tmp --env HOME=/tmp \
        -- bash "/tmp/stage/70-live-tail.sh" \
        >> "${STATUS_DIR:-/tmp/gyre-sandbox/status}/$TASK_NAME/stream.log" 2>/dev/null
    ) &
    STREAM_PID=$!
  }
  stop_stream() {
    [ -n "$STREAM_PID" ] && { kill "$STREAM_PID" 2>/dev/null; STREAM_PID=""; }
  }
  publish_status "round-$round" "agent running"
  start_stream
  status_line=$(osexec "$WORKDIR/30-run.sh" 2>&1 | tail -3)
  stop_stream
  push_result=$(osexec "$WORKDIR/40-push.sh")
  [ -n "$push_result" ] && log "    push: $push_result"
  echo "$status_line" | grep -q 'WORKER-STATUS=complete' && { log "=== Task complete after round $round"; publish_status complete; break; }
done
log "<<< Worker rounds finished"
if osexec "$WORKDIR/40-push.sh" | grep -q PUSHED; then
  log ">>> Opening PR (idempotent)"
  cat > "$WORKDIR/50-pr.sh" <<EOF
resp=\$(curl -s -X POST https://api.github.com/repos/jsell-rh/gyre/pulls \\
  -H "Authorization: Bearer \$GITHUB_TOKEN" \\
  -H "Accept: application/vnd.github+json" \\
  -d '{"title":"$TASK_NAME (sandbox worker)","head":"worker/$TASK_NAME","base":"$BASE_BRANCH","body":"Implemented in OpenShell sandbox $SANDBOX by the gyre sandbox worker loop."}')
url=\$(echo "\$resp" | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{try{console.log(JSON.parse(d).html_url||"")}catch(e){console.log("")}})' 2>/dev/null)
if [ -n "\$url" ]; then
  echo "PR-URL=\$url"
else
  # 422 = PR already exists for this head/base — fetch it
  existing=\$(curl -s "https://api.github.com/repos/jsell-rh/gyre/pulls?head=jsell-rh:worker/$TASK_NAME&state=open" -H "Authorization: Bearer \$GITHUB_TOKEN" | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{try{console.log(JSON.parse(d)[0].html_url||"")}catch(e){console.log("")}})' 2>/dev/null)
  echo "PR-URL=\${existing:-FAILED}"
  [ -n "\$existing" ] || echo "\$resp" | head -c 300
fi
EOF
  stage "$WORKDIR/50-pr.sh"
  PR_OUT=$(osexec "$WORKDIR/50-pr.sh")
  log "=== $PR_OUT"
  publish_status pr-opened "$PR_OUT"
else
  log "=== No changes to push (task may have been informational)"
  publish_status done "no changes"
fi
