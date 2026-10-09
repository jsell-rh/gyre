#!/usr/bin/env bash
# One isolated attempt in OpenShell. The controller owns scheduling and merging.
set -euo pipefail
MODE=$1 TASK=$2 ARG1=$3 ARG2=$4 ARG3=${5:-}
case "$MODE:$TASK" in
  worker:task-[0-9]*|check:task-[0-9]*) ;;
  *) echo "invalid mode/task" >&2; exit 2;;
esac
ROOT=${GYRE_DEV_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
[[ "$ARG3" =~ ^[a-f0-9]{16}$ ]] || { echo "invalid attempt id" >&2; exit 2; }
SANDBOX="gyre-${TASK#task-}-${MODE:0:1}-${ARG3:0:8}"
if [ "${#SANDBOX}" -gt 19 ]; then
  SANDBOX=$(python3 - "$TASK" "$MODE" "$ARG3" <<'PY'
import sys
number, encoded = int(sys.argv[1][5:]), ''
while number:
    number, digit = divmod(number, 36)
    encoded = '0123456789abcdefghijklmnopqrstuvwxyz'[digit] + encoded
name = f'gyr-z{encoded or "0"}-{sys.argv[2][0]}-{sys.argv[3][:8]}'
if len(name) > 19:
    raise SystemExit('task ID exceeds compact sandbox naming capacity')
print(name)
PY
  )
fi
OS=${OPENSHELL:-openshell}
GATEWAY_LOCK="${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/gateway-login.lock"
export OPENSHELL_GATEWAY_INSECURE=true OPENSHELL_WORKSPACE=default
report_phase() {
  python3 - "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/phase.json" "$1" "${2:-}" "${3:-0}" <<'PY'
import json, os, pathlib, sys, time
path = pathlib.Path(sys.argv[1]); path.parent.mkdir(parents=True, exist_ok=True)
temporary = path.with_suffix('.tmp')
temporary.write_text(json.dumps({'phase': sys.argv[2], 'reason': sys.argv[3][-1000:],
                                'at': int(time.time()), 'retry_at': int(sys.argv[4])}))
os.replace(temporary, path)
PY
}
reauth() {
  # OpenShell writes shared credentials; serialize logins across attempts.
  (
    flock -x -w 30 8 || exit 77
    OPENSHELL_OIDC_CLIENT_SECRET="${OPENSHELL_OIDC_CLIENT_SECRET:?}" OPENSHELL_NO_BROWSER=1 \
      timeout 30 "$OS" gateway login gyre-gyre >/dev/null || exit 77
  ) 8>"$GATEWAY_LOCK"
}
osrun() {
  reauth
  local cursor="${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/remote-log.offset"
  local offset=0
  local -a retry=()
  case "${1:-}" in registry|git) retry=(--retry-failed);; esac
  [ ! -f "$cursor" ] || offset=$(cat "$cursor")
  timeout "${GYRE_DEV_EXEC_TIMEOUT:-14400}" "$OS" -g gyre-gyre sandbox exec \
    -n "$SANDBOX" --no-login-shell --workdir /tmp \
    --env HOME=/tmp --env RUSTUP_HOME=/usr/local/rustup --env CARGO_HOME=/tmp/cargo \
    --env CARGO_TARGET_DIR=/tmp/gyre-target \
    --env CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc \
    --env 'RUSTFLAGS=-C link-arg=-fuse-ld=lld' \
    --env GYRE_DEV_WORKER_ROUNDS="${GYRE_DEV_WORKER_ROUNDS:-6}" \
    --env GYRE_DEV_ROUND_TIMEOUT="${GYRE_DEV_ROUND_TIMEOUT:-1800}" \
    --env GYRE_DEV_COMPACT_BYTES="${GYRE_DEV_COMPACT_BYTES:-400000}" \
    --env GYRE_DEV_GATE_TIMEOUT="${GYRE_DEV_GATE_TIMEOUT:-1800}" \
    --env GYRE_DEV_MODEL="${GYRE_DEV_MODEL:-enmaas-glm-5-3/rits/zai-org/glm-5-3}" \
    --env GYRE_DEV_IMPLEMENTATION_MODEL="${GYRE_DEV_IMPLEMENTATION_MODEL:-${GYRE_DEV_MODEL:-enmaas-glm-5-3/rits/zai-org/glm-5-3}}" \
    --env GYRE_DEV_REPO_URL="${GYRE_DEV_REPO_URL:-https://github.com/jsell-rh/gyre.git}" \
    -- python3 /tmp/stage/dev-attach.py "$MODE" "$TASK" "$ARG1" "$ARG2" "$ARG3" "$offset" "${retry[@]}" \
    | python3 "$ROOT/scripts/dev-log-cursor.py" "$cursor"
}
stage_bundle() {
  local bundle stage_rc retry role stage_log
  bundle=$(mktemp -d "${TMPDIR:-/tmp}/gyre-stage.XXXXXX")
  cp "$ROOT/scripts/dev-remote.sh" "$ROOT/scripts/dev-round.sh" \
    "$ROOT/scripts/dev-attach.py" "$ROOT/scripts/dev-process.sh" "$ROOT/scripts/dev-build-command.sh" "$ROOT/scripts/dev-checkpoint.py" "$ROOT/scripts/dev-review-guard.py" \
    "$ROOT/scripts/dev-stream.mjs" "$ROOT/scripts/dev-check.sh" \
    "$ROOT/scripts/dev-merge-message.py" \
    "$ROOT/scripts/dev-static-gate.py" \
    "$ROOT/scripts/dev-coverage.py" "$ROOT/scripts/dev-audit-check.py" "$ROOT/scripts/dev-attribution.py" \
    "$ROOT/scripts/dev-context.py" "$ROOT/scripts/dev-contract.py" \
    "$ROOT/scripts/check-rustfmt-diff.py" "$ROOT/scripts/check-clippy-diff.py" "$bundle/"
  for role in implementation review rebase integration-review audit audit-review; do
    cp "$ROOT/specs/prompts/dev-$role.md" "$bundle/dev-$role.md"
  done
  cp "${MODELS_YML:-$ROOT/docker/dev-worker/models.yml}" "$bundle/models.yml"
  cp "${CONFIG_YML:-$ROOT/docker/dev-worker/config.yml}" "$bundle/config.yml"
  if [ -f "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/repair.md" ]; then
    cp "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/repair.md" "$bundle/repair.md"
  fi
  if [ -f "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/task.md" ]; then
    cp "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/task.md" "$bundle/task.md"
  fi
  if [ -f "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/audit-contract.json" ]; then
    cp "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/audit-contract.json" "$bundle/audit-contract.json"
  fi
  stage_rc=1
  stage_log=$(mktemp "${TMPDIR:-/tmp}/gyre-stage-log.XXXXXX")
  for retry in 1 2 3; do
    reauth
    set +e
    tar -C "$bundle" -cf - . | timeout 120 "$OS" -g gyre-gyre sandbox exec -n "$SANDBOX" \
      --no-login-shell --workdir /tmp -- bash -c 'mkdir -p /tmp/stage && tar -C /tmp/stage -xf -' >"$stage_log" 2>&1
    stage_rc=${PIPESTATUS[1]}
    set -e
    [ "$stage_rc" -eq 0 ] && break
    cat "$stage_log" >&2
    echo "sandbox staging interrupted; retrying in $SANDBOX ($retry/3)" >&2
    sleep "$((retry * 5))"
  done
  rm -rf "$bundle"
  if [ "$stage_rc" -ne 0 ] && grep -Eiq 'h2 protocol error|tls handshake eof|peer closed connection|failed to connect to gateway|timed out' "$stage_log"; then
    rm -f "$stage_log"
    return 77
  fi
  rm -f "$stage_log"
  return "$stage_rc"
}
# shellcheck disable=SC2329 # invoked by the EXIT trap
cleanup() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ] && [ "$rc" -ne 77 ] && [ "$rc" -ne 78 ] && [ "$rc" -ne 79 ] &&
     ! grep -q 'GYRE_BOOTSTRAP_COMPLETE' "${transport_log:-/dev/null}" 2>/dev/null; then
    rc=75
  fi
  if [ "${CREATED:-0}" != 1 ]; then exit "$rc"; fi
  report_phase Deleting "attempt finished (exit=$rc)"
  # Attempt output is already stored locally. Sandboxes are expensive; delete
  # on success and failure. The controller retries cleanup after driver crashes.
  if reauth; then
    if [ "$rc" -ne 0 ] && [ "$MODE" = worker ] &&
       grep -q 'GYRE_BOOTSTRAP_COMPLETE' "${transport_log:-/dev/null}" 2>/dev/null; then
      python3 "$ROOT/scripts/dev-recover.py" "$SANDBOX" \
        "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/recovery.patch" || true
    fi
    if timeout 180 "$OS" -g gyre-gyre sandbox delete "$SANDBOX" >/dev/null 2>&1; then
      echo "deleted sandbox: $SANDBOX"
    else
      echo "sandbox delete failed; controller will retry: $SANDBOX" >&2
    fi
  fi
  exit "$rc"
}
trap cleanup EXIT
CREATED=0
reauth
CREATED=1 # create may provision compute before its response fails
report_phase Provisioning
create_log=$(mktemp "${TMPDIR:-/tmp}/gyre-create.XXXXXX")
set +e
timeout 600 "$OS" -g gyre-gyre sandbox create --name "$SANDBOX" \
  --from "${GYRE_DEV_IMAGE:-ghcr.io/jsell-rh/gyre-worker@sha256:0c4a04a340e20c91e89f855c5d75d940b8550798441990ca823c7b8ebb8cbcec}" \
  --provider gyre-enmaas --provider gyre-github-rw \
  --label "gyre.dev/controller=${GYRE_DEV_OWNER:-unregistered}" --label "gyre.dev/attempt=$ARG3" --label "gyre.dev/task=$TASK" \
  --policy "${GYRE_DEV_POLICY:-$ROOT/docker/dev-worker/policy.yaml}" \
  --detach -- bash -c 'while true; do sleep 3600; done' 2>&1 | tee "$create_log"
create_rc=${PIPESTATUS[0]}
set -e
if [ "$create_rc" -ne 0 ]; then
  if grep -Eiq 'ConfigurationInvalid|invalid policy|invalid configuration|provider .*not found' "$create_log"; then
    rm -f "$create_log"; echo "sandbox configuration invalid" >&2; exit 79
  fi
  if grep -Eiq 'ProvisioningTimedOut|ConfigurationPending|insufficient|capacity|resource exhausted|timed out|timedout' "$create_log" || [ "$create_rc" -eq 124 ]; then
    # A timed-out response may still have allocated a pending object. Keep
    # reconciling that same name while the autoscaler supplies its capacity.
    if timeout 30 "$OS" -g gyre-gyre sandbox get "$SANDBOX" >/dev/null 2>&1; then
      echo "create timed out; existing sandbox will continue provisioning" >&2
    else
      rm -f "$create_log"; echo "sandbox provisioning deferred for capacity" >&2; exit 78
    fi
  elif grep -Eiq 'h2 protocol error|tls handshake eof|peer closed connection|failed to connect to gateway|Internal error.*create sandbox failed|sandbox provisioning stream ended before reaching terminal phase' "$create_log"; then
    # Losing the response stream does not cancel the remote allocation.
    # Observe the existing object rather than deleting and reallocating it.
    if timeout 30 "$OS" -g gyre-gyre sandbox get "$SANDBOX" >/dev/null 2>&1; then
      echo "create response interrupted; existing sandbox will continue provisioning" >&2
    else
      rm -f "$create_log"; echo "gateway transport unavailable during create" >&2; exit 77
    fi
  else
    rm -f "$create_log"; exit 75
  fi
fi
rm -f "$create_log"
ready=0
deadline=$(( $(date +%s) + ${GYRE_DEV_READY_TIMEOUT:-1800} ))
delay=${GYRE_DEV_READY_POLL:-15}
try=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  try=$((try + 1))
  reauth
  phase=$(timeout 60 "$OS" -g gyre-gyre sandbox get "$SANDBOX" 2>/dev/null) || phase=""
  if [[ "$phase" == *"Phase: Ready"* ]]; then
    ready=1; break
  fi
  if [[ "$phase" == *"ConfigurationInvalid"* ]]; then
    echo "sandbox configuration invalid: $phase" >&2
    exit 79
  fi
  retry_at=$(( $(date +%s) + delay ))
  report_phase Pending "${phase:-gateway status unavailable}" "$retry_at"
  echo "waiting for sandbox Ready (poll=$try, next=${delay}s, existing=$SANDBOX)" >&2
  sleep "$delay"
  [ "$delay" -ge 120 ] || delay=$((delay * 2))
  [ "$delay" -le 120 ] || delay=120
done
[ "$ready" -eq 1 ] || { echo "sandbox never became Ready" >&2; exit 78; }
touch "${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/sandbox.ready"
report_phase Staging
stage_bundle
transport_log="${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/transport.log"
remote_rc=74
restart_reason=''
for reconnect in 1 2 3 4; do
  report_phase "${MODE^}" "exec reconnect=$reconnect"
  run_log=$(mktemp "${TMPDIR:-/tmp}/gyre-run.XXXXXX")
  set +e
  osrun "$restart_reason" 2>&1 | tee -a "$transport_log" "$run_log"
  remote_rc=${PIPESTATUS[0]}
  set -e
  restart_reason=''
  retry_reason=""
  if [ "$remote_rc" -eq 74 ]; then
    retry_reason="transport interrupted"
  elif [ "$remote_rc" -ne 0 ] &&
       grep -Eiq 'h2 protocol error|tls handshake eof|peer closed connection|failed to connect to gateway|OIDC credentials are missing' "$run_log"; then
    retry_reason="gateway transport interrupted"
  elif [ "$remote_rc" -ne 0 ] &&
       grep -Eq 'Failed to connect to (static|index)\.crates\.io|failed to download from .*static\.crates\.io' "$run_log"; then
    retry_reason="Cargo registry unavailable"
    restart_reason=registry
  elif [ "$remote_rc" -ne 0 ] &&
       ! grep -q 'GYRE_BOOTSTRAP_COMPLETE' "$transport_log" &&
       grep -Eiq 'RPC failed; curl (7|28|35|52|55|56)\b|Could not resolve host|Failed to connect to github\.com|unexpected disconnect|remote end hung up|fatal: early EOF' "$run_log"; then
    retry_reason="Git transport unavailable"
    restart_reason=git
  fi
  rm "$run_log"
  [ -n "$retry_reason" ] && [ "$reconnect" -lt 4 ] || break
  echo "$retry_reason; retrying in $SANDBOX ($reconnect/4)" >&2
  sleep "$((5 * 2 ** (reconnect - 1)))"
done
if [ -n "$retry_reason" ]; then
  echo "$retry_reason persisted after four reconnects; retry task explicitly" >&2
  exit 77
fi
if [ "$remote_rc" -ne 0 ] && ! grep -q 'GYRE_BOOTSTRAP_COMPLETE' "$transport_log"; then
  echo "sandbox bootstrap or transport failed before agent work" >&2
  exit 75
fi
exit "$remote_rc"
