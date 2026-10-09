#!/usr/bin/env bash
# Bound peak build memory within one sandbox; distinct sandboxes stay parallel.
set -euo pipefail
command_name=$(basename "$0")
case "$command_name" in cargo|npm|npx) ;; *) exit 2;; esac
key="GYRE_DEV_REAL_${command_name^^}"
real=${!key:?missing original build command}
case "${1:-}" in --version|-V|--help|-h|help|metadata|locate-project) exec "$real" "$@";; esac
if [ "${GYRE_DEV_BUILD_LOCK_HELD:-0}" != 1 ]; then
  # Inherit the lock into compiler children so an orphaned compiler still
  # occupies the lane. Build scripts can invoke npm/cargo recursively without
  # reacquiring their parent's lock.
  exec 8>"${GYRE_DEV_BUILD_LOCK:-/tmp/stage/build.lock}"
  flock 8
  export GYRE_DEV_BUILD_LOCK_HELD=1
fi
exec "$real" "$@"
