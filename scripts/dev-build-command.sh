#!/usr/bin/env bash
# Bound peak build memory within one sandbox; distinct sandboxes stay parallel.
set -euo pipefail
command_name=$(basename "$0")
case "$command_name" in cargo|npm|npx) ;; *) exit 2;; esac
key="GYRE_DEV_REAL_${command_name^^}"
real=${!key:?missing original build command}
case "${1:-}" in --version|-V|--help|-h|help|metadata|locate-project) exec "$real" "$@";; esac
if [ "${GYRE_PIPELINE_STAGE:-}" = review ] && [ "$command_name" = cargo ]; then
  for option in "$@"; do
    case "$option" in --all|--workspace|--all-targets)
      echo 'Gyre: workspace-wide builds and suites belong to deterministic verification. Run a focused package/test probe and record deferred checks in review-evidence.' >&2
      exit 64
    ;; esac
  done
fi
# A worker's build budget is mechanical, including explicit agent -j flags.
export CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_PROFILE_DEV_INCREMENTAL=false CARGO_PROFILE_TEST_INCREMENTAL=false
export MALLOC_ARENA_MAX=2
export RUSTFLAGS='-C link-arg=-fuse-ld=mold -C link-arg=-Wl,--threads=1'
if [ "$command_name" != cargo ]; then export NODE_OPTIONS=--max-old-space-size=2048; fi
if [ "$command_name" = cargo ]; then
  bounded=(); skip=0; passthrough=0
  for option in "$@"; do
    if [ "$passthrough" = 1 ]; then bounded+=("$option"); continue; fi
    if [ "$skip" = 1 ]; then skip=0; continue; fi
    case "$option" in
      --) passthrough=1; bounded+=("$option");;
      -j|--jobs) skip=1;;
      -j[0-9]*|--jobs=*) ;;
      *) bounded+=("$option");;
    esac
  done
  set -- "${bounded[@]}"
fi
if { [ "$command_name" = npm ] && { [ "${1:-}" = test ] || { [ "${1:-}" = run ] && [ "${2:-}" = test ]; }; }; } || { [ "$command_name" = npx ] && [ "${1:-}" = vitest ]; }; then
  if [ "$command_name" = npm ] && [[ " $* " != *" -- "* ]]; then set -- "$@" --; fi
  set -- "$@" --no-file-parallelism --maxWorkers=1
fi
if [ "${GYRE_DEV_BUILD_LOCK_HELD:-0}" != 1 ]; then
  # Inherit the lock into compiler children so an orphaned compiler still
  # occupies the lane. Build scripts can invoke npm/cargo recursively without
  # reacquiring their parent's lock.
  exec 8>"${GYRE_DEV_BUILD_LOCK:-/tmp/stage/build.lock}"
  flock 8
  export GYRE_DEV_BUILD_LOCK_HELD=1
fi
exec "$real" "$@"
