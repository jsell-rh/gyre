#!/usr/bin/env bash
# One isolated attempt in OpenShell. The controller owns scheduling and merging.
set -euo pipefail
MODE=$1 TASK=$2 ARG1=$3 ARG2=$4 ARG3=${5:-}
case "$MODE:$TASK" in
  worker:task-[0-9]*|check:task-[0-9]*) ;;
  *) echo "invalid mode/task" >&2; exit 2;;
esac
ROOT=$(cd "$(dirname "$0")/.." && pwd)
[[ "$ARG3" =~ ^[a-f0-9]{16}$ ]] || { echo "invalid attempt id" >&2; exit 2; }
SANDBOX="gyre-${TASK#task-}-${MODE:0:1}-${ARG3:0:8}"
OS=${OPENSHELL:-openshell}
export OPENSHELL_GATEWAY_INSECURE=true OPENSHELL_WORKSPACE=default
reauth() {
  OPENSHELL_OIDC_CLIENT_SECRET="${OPENSHELL_OIDC_CLIENT_SECRET:?}" OPENSHELL_NO_BROWSER=1 \
    "$OS" gateway login gyre-gyre >/dev/null
}
osrun() {
  reauth
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
    --env GYRE_DEV_IMPLEMENTATION_MODEL="${GYRE_DEV_IMPLEMENTATION_MODEL:-Inferact/Qwen3.8-Flash-Next-NVFP4}" \
    --env GYRE_DEV_REPO_URL="${GYRE_DEV_REPO_URL:-https://github.com/jsell-rh/gyre.git}" \
    -- bash /tmp/stage/dev-remote.sh "$MODE" "$TASK" "$ARG1" "$ARG2" "$ARG3"
}
stage_bundle() {
  local bundle stage_rc retry role
  bundle=$(mktemp -d "${TMPDIR:-/tmp}/gyre-stage.XXXXXX")
  cp "$ROOT/scripts/dev-remote.sh" "$ROOT/scripts/dev-round.sh" \
    "$ROOT/scripts/dev-stream.mjs" "$ROOT/scripts/dev-check.sh" \
    "$ROOT/scripts/check-rustfmt-diff.py" "$ROOT/scripts/check-clippy-diff.py" "$bundle/"
  for role in implementation review rebase integration-review; do
    cp "$ROOT/specs/prompts/dev-$role.md" "$bundle/dev-$role.md"
  done
  cp "${MODELS_YML:-/tmp/sbx-models.yml}" "$bundle/models.yml"
  cp "${CONFIG_YML:-$HOME/.pi/agent/config.yml}" "$bundle/config.yml"
  stage_rc=1
  for retry in 1 2 3; do
    reauth
    set +e
    tar -C "$bundle" -cf - . | timeout 120 "$OS" -g gyre-gyre sandbox exec -n "$SANDBOX" \
      --no-login-shell --workdir /tmp -- bash -c 'mkdir -p /tmp/stage && tar -C /tmp/stage -xf -' >/dev/null
    stage_rc=${PIPESTATUS[1]}
    set -e
    [ "$stage_rc" -eq 0 ] && break
    echo "sandbox staging interrupted; retrying in $SANDBOX ($retry/3)" >&2
    sleep "$((retry * 5))"
  done
  rm -rf "$bundle"
  return "$stage_rc"
}
# shellcheck disable=SC2329 # invoked by the EXIT trap
cleanup() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ] && ! grep -q 'GYRE_BOOTSTRAP_COMPLETE' "${transport_log:-/dev/null}" 2>/dev/null; then
    rc=75
  fi
  if [ "${CREATED:-0}" != 1 ]; then exit "$rc"; fi
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
timeout 600 "$OS" -g gyre-gyre sandbox create --name "$SANDBOX" \
  --from "${GYRE_DEV_IMAGE:-ghcr.io/jsell-rh/gyre-worker@sha256:0c4a04a340e20c91e89f855c5d75d940b8550798441990ca823c7b8ebb8cbcec}" \
  --provider gyre-pricetag --provider gyre-github-rw \
  --policy "${GYRE_DEV_POLICY:-/tmp/gyre-sandbox/policy.yaml}" \
  --detach -- bash -c 'while true; do sleep 3600; done'
CREATED=1
ready=0
for try in $(seq 1 40); do
  reauth
  phase=$(timeout 60 "$OS" -g gyre-gyre sandbox get "$SANDBOX" 2>/dev/null) || phase=""
  if [[ "$phase" == *"Phase: Ready"* ]]; then
    ready=1; break
  fi
  echo "waiting for sandbox Ready ($try/40)" >&2
  sleep 15
done
[ "$ready" -eq 1 ] || { echo "sandbox never became Ready" >&2; exit 1; }
stage_bundle
transport_log="${GYRE_DEV_STATE:-$ROOT/.gyre-dev-controller}/attempts/$ARG3/transport.log"
remote_rc=74
for reconnect in 1 2 3; do
  set +e
  osrun 2>&1 | tee -a "$transport_log"
  remote_rc=${PIPESTATUS[0]}
  set -e
  [ "$remote_rc" -eq 74 ] || break
  echo "sandbox transport interrupted; reconnecting to $SANDBOX ($reconnect/3)" >&2
  sleep "$((reconnect * 5))"
done
if [ "$remote_rc" -ne 0 ] && ! grep -q 'GYRE_BOOTSTRAP_COMPLETE' "$transport_log"; then
  echo "sandbox bootstrap or transport failed before agent work" >&2
  exit 75
fi
exit "$remote_rc"
