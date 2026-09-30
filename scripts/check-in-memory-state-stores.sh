#!/usr/bin/env bash
# check-in-memory-state-stores.sh — AppState stores must be port-backed,
# never bare in-memory maps.
#
# Procedural background (specs/reviews/task-107.md F9):
# the Fulcio commit-signature feature persisted its records in
# `AppState.commit_signatures: Arc<Mutex<HashMap<String, CommitSignature>>>`
# — a bare in-memory map, no port, no adapter, no load path — while every
# one of the 47 sibling stores in `build_state` was wired through
# `gyre_ports` (the `store!` pattern). Tests passed (single process),
# but any server restart or crash silently orphaned every signed commit,
# and the verification endpoint — an acceptance criterion — 404s on all
# of them in deployment.
#
# Detection: `type XStore = Arc<Mutex<HashMap<...>>>` or
# `type XStore = Arc<Mutex<Vec<...>>>` aliases in non-test server code
# (production code only: each file is cut at the first `#[cfg(test)]`).
# A Vec-backed store is the same failure class: append-only memory with
# no persistence and no query surface.
#
# Remediation: route new durable state through `gyre-ports` + an adapter
# (the `store!` wiring pattern in `build_state`), like every sibling
# store. Ephemeral caches that are legitimately process-local should not
# be named `*Store` and should carry an inline
#   // in-memory-state-stores:ok — <reason>
# marker on the type alias line.
#
# Exemptions are legacy debt in
# scripts/in-memory-state-stores-exemptions.txt (path:line form,
# frozen count). Fix by moving the store behind a port; never add
# entries.
FROZEN_EXEMPTION_COUNT=5
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/in-memory-state-stores-exemptions.txt"
SRCDIR="crates/gyre-server/src"
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
    echo "FAIL: in-memory-state-stores-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-107 F9), not an approval"
    echo "mechanism. Fix a flagged store by routing it through gyre-ports"
    echo "+ an adapter (the store! wiring pattern in build_state). If you"
    echo "fixed a site, delete the line and lower FROZEN_EXEMPTION_COUNT;"
    echo "never raise it."
    exit 1
fi

TMPFILE="$(mktemp /tmp/.inmem-stores.XXXXXX)"
trap 'rm -f "$TMPFILE"' EXIT

find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Production code only: cut at first #[cfg(test)].
        my $cut = index($src, "#[cfg(test)]");
        my $scan = $cut >= 0 ? substr($src, 0, $cut) : $src;

        while ($scan =~ /\btype\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*Arc\s*<\s*Mutex\s*<\s*(?:HashMap|Vec)\s*</gs) {
            my $off = $-[0];
            my $name = $1;
            my $line_start = rindex(substr($scan, 0, $off), "\n") + 1;
            my $line_end = index($scan, "\n", $off);
            $line_end = length($scan) if $line_end < 0;
            my $line_txt = substr($scan, $line_start, $line_end - $line_start);
            next if $line_txt =~ /in-memory-state-stores:ok/;
            my $line = 1 + (substr($scan, 0, $off) =~ tr/\n//);
            my $stmt = $line_txt;
            $stmt =~ s/^\s+|\s+$//g;
            print "$file:$line: store type alias $name is an in-memory Arc<Mutex<...>> with no port/adapter backing: $stmt\n";
        }
    ' "$f" < "$f"
done > "$TMPFILE"

while IFS= read -r line; do
    [ -n "$line" ] || continue
    loc="$(echo "$line" | cut -d: -f1,2)"
    if [ -z "${EXEMPT["$loc"]:-}" ]; then
        echo "$line"
        FAIL=1
    fi
done < "$TMPFILE"

if [ "$FAIL" -ne 0 ]; then
    echo ""
    echo "FAIL: in-memory AppState stores without port/adapter backing."
    echo ""
    echo "Every store persisted behind an acceptance-criterion endpoint"
    echo "must survive a restart. A bare Arc<Mutex<HashMap/Vec<...>>>"
    echo "type alias is process-memory-only: a restart orphans every"
    echo "record and the endpoint 404s on data the tests just created"
    echo "(task-107 F9: commit signatures lost on restart while all 47"
    echo "sibling stores were port-backed)."
    echo ""
    echo "Fix: route the store through gyre-ports + an adapter (the"
    echo "store! wiring in build_state). Do not exempt new sites."
    exit 1
fi

echo "OK: no in-memory Arc<Mutex<HashMap/Vec<...>>> store aliases in non-test server code."
