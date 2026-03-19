#!/usr/bin/env bash
# Em-dash check: reject Unicode em-dash (U+2014) in source files.
#
# Em-dashes in code comments or strings are a signal of AI-generated prose
# that hasn't been reviewed. Use a regular hyphen or double-hyphen instead.
#
# Remediation: Replace the em-dash character (--) with a hyphen-minus (-).

set -euo pipefail

FAIL=0

for file in "$@"; do
    if grep -Pn "\xe2\x80\x94" "$file" 2>/dev/null; then
        echo "EM-DASH VIOLATION: ${file} contains Unicode em-dash (U+2014)"
        echo "  Remediation: Replace em-dash with a hyphen-minus (-)."
        FAIL=1
    fi
done

exit "$FAIL"
