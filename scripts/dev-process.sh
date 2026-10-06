#!/usr/bin/env bash
# Record a child result atomically so a restarted controller can adopt it.
set -uo pipefail
result=$1
shift
"$@"
rc=$?
printf '%s\n' "$rc" > "$result.tmp"
mv "$result.tmp" "$result"
exit "$rc"
