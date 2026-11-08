#!/usr/bin/env bash
# check-fail-open-ref-resolution.sh — `resolve_ref(...)` must not be
# fail-opened with `.unwrap_or_default()` / `.unwrap_or("")`.
#
# Procedural background (specs/reviews/task-095.md R3-F3):
# repo_status (api/recovery.rs) computed `head_sha =
# resolve_ref(...).unwrap_or_default()`. On ref-resolution failure the
# empty string flowed into worktree creation, which failed, which fell
# back to running gates in the server cwd, and the endpoint returned
# `main_green: true` — a fail-open default on the exact boolean the
# human consults for the "is main green / may I resume the queue"
# decision. The queue-pause mechanism elsewhere is deliberately
# fail-closed; a health signal that inverts the protocol's safety
# posture under failure is a defect even when the happy path is green.
#
# `resolve_ref` returns Option<String> (git_refs.rs). A None means the
# ref does not resolve — there is no valid empty-string SHA. Any
# same-statement `.unwrap_or_default()` / `.unwrap_or("")` on its result
# converts "cannot determine state" into "state is fine".
#
# Allowed patterns: `?`, `.unwrap_or_else(...)` returning a real SHA,
# explicit 404/500 error propagation, or logging + fail-closed return.
#
# Exemptions are legacy debt in
# scripts/fail-open-ref-resolution-exemptions.txt (path:line form). The
# file is EMPTY at baseline zero: the two original R3-F3 sites were fixed
# fail-closed in product code. The count must stay 0 — never add entries.
FROZEN_EXEMPTION_COUNT=0
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/fail-open-ref-resolution-exemptions.txt"
SRCDIR="crates"
FAIL=0

declare -A EXEMPT
EXEMPT_TOTAL=0
if [ -f "$EXEMPTIONS_FILE" ]; then
    while IFS=: read -r file line rest; do
        case "$file" in ''|'#'*) continue ;; esac
        [ -n "$line" ] || continue
        EXEMPT["${file}:${line}"]=1
        EXEMPT_TOTAL=$((EXEMPT_TOTAL + 1))
    done < "$EXEMPTIONS_FILE"
fi

if [ "$EXEMPT_TOTAL" -gt "$FROZEN_EXEMPTION_COUNT" ]; then
    echo "FAIL: fail-open-ref-resolution-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-095 R3-F3), not an approval"
    echo "mechanism. Fix a flagged site by failing closed (propagate the None,"
    echo "return an error, or log + fail-closed) — never by exempting it. If"
    echo "you fixed a site, delete the line and lower FROZEN_EXEMPTION_COUNT;"
    echo "never raise it."
    exit 1
fi

# Match resolve_ref( ... .unwrap_or_default()/...unwrap_or("") within one
# statement (up to the terminating `;`). Statement-level matching is robust
# against rustfmt's multi-line continuations (.await / .unwrap_* on their
# own lines). A `resolve_ref(` argument list cannot contain a `;`, so the
# non-greedy scan cannot bleed across statements.
VIOLATIONS=""
find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Blank string-literal contents (length- and line-preserving) so
        # braces inside format!("...{}") do not read as block openers.
        my $scan = $src;
        $scan =~ s/("(?:[^"\\]|\\.)*")/ my $t = $1; $t =~ s\/[^"\n]\/x\/g; $t /ges;
        # Match resolve_ref( ... .unwrap_or_default()/.unwrap_or("") with no
        # `;`, `{`, or `}` between the call and the fail-open: the head
        # segment cannot cross a statement boundary or block opener, which
        # rules out an unrelated .unwrap_or_default() inside a subsequent
        # block (task-095 R3 false-positive class).
        while ($scan =~ /resolve_ref\([^;{}]*?(?:\.unwrap_or_default\(\)|\.unwrap_or\(""\))/gs) {
            my $start = $-[0];
            my $line = 1 + (substr($scan, 0, $start) =~ tr/\n//);
            my $stmt = substr($src, $start, $+[0] - $start);
            $stmt =~ s/\s+/ /g;
            print "$file:$line: $stmt\n";
        }
    ' "$f" < "$f"
done > /tmp/.failopen-refs.$$

while IFS= read -r line; do
    [ -n "$line" ] || continue
    loc="$(echo "$line" | cut -d: -f1,2)"
    if [ -z "${EXEMPT["$loc"]:-}" ]; then
        echo "$line" >> /tmp/.failopen-violations.$$
        FAIL=1
    fi
done < /tmp/.failopen-refs.$$

rm -f /tmp/.failopen-refs.$$
if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: resolve_ref() results fail-opened with .unwrap_or_default()/.unwrap_or(\"\"):"
    echo ""
    cat /tmp/.failopen-violations.$$ 2>/dev/null || true
    echo ""
    echo "A None from resolve_ref means the ref does not resolve — there is no"
    echo "valid empty-string SHA. Fail-open defaults here convert 'cannot"
    echo "determine state' into 'state is fine' (task-095 R3-F3: main_green"
    echo "returned true while gates ran in the server cwd). Fail closed:"
    echo "propagate the None as an error/404, or log and return a failure."
    exit 1
fi

echo "OK: no fail-open .unwrap_or_default()/.unwrap_or(\"\") on resolve_ref() results."
