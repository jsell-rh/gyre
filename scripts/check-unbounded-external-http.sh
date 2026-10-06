#!/usr/bin/env bash
# check-unbounded-external-http.sh — outbound HTTP clients must be
# latency-bounded.
#
# Procedural background (specs/reviews/task-107.md F10):
# the Fulcio/Rekor signing path guaranteed "any external failure falls
# back to local signing so squash never fails because of the external
# stack" — but issued four external awaits per round through
# `reqwest::Client::new()` with no `.timeout(...)` and no
# `tokio::time::timeout` wrapper. reqwest has NO default total timeout:
# the fallback guarantee covered fast errors (connection refused, tested)
# but not a hung service, which stalled the squash request indefinitely.
#
# Detection: `reqwest::Client::new()` in non-test Rust (production code
# only: each file is cut at the first `#[cfg(test)]`) that is not
# (a) followed within the same statement/builder chain by `.timeout(`
# or `Client::builder()`/`ClientBuilder` construction carrying
# `.timeout(`, AND not (b) inside a function body that wraps its
# external awaits in `tokio::time::timeout`.
#
# Remediation: build the client with
#   reqwest::Client::builder().timeout(Duration::from_secs(N)).build()?
# or wrap each external await in `tokio::time::timeout` (repo precedent:
# api/agent_logs.rs, api/audit.rs).
#
# Exemptions are legacy debt in
# scripts/unbounded-external-http-exemptions.txt (path:line form,
# frozen count). Fix by bounding the client; never add entries.
FROZEN_EXEMPTION_COUNT=5
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/unbounded-external-http-exemptions.txt"
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
    echo "FAIL: unbounded-external-http-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-107 F10), not an"
    echo "approval mechanism. Fix a flagged client by building it with"
    echo ".timeout(...) or wrapping external awaits in"
    echo "tokio::time::timeout. If you fixed a site, delete the line and"
    echo "lower FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

TMPFILE="$(mktemp /tmp/.unbounded-http.XXXXXX)"
trap 'rm -f "$TMPFILE"' EXIT

find "$SRCDIR" -name '*.rs' -not -path '*/tests/*' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Production code only: cut at first #[cfg(test)].
        my $cut = index($src, "#[cfg(test)]");
        my $scan = $cut >= 0 ? substr($src, 0, $cut) : $src;

        # Split into fn chunks so the timeout-context check is scoped to
        # the ENCLOSING function: a builder with .timeout() in a sibling
        # function never bounds this Client::new().
        my @chunks;
        while ($scan =~ /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/gs) {
            push @chunks, $-[0];
        }
        for my $ci (0 .. $#chunks) {
            my $fstart = $chunks[$ci];
            my $fend = $ci < $#chunks ? $chunks[$ci + 1] : length($scan);
            my $fbody = substr($scan, $fstart, $fend - $fstart);
            while ($fbody =~ /\breqwest::Client::new\s*\(\s*\)/gs) {
                my $off = $-[0];
                my $line_start = rindex(substr($fbody, 0, $off), "\n") + 1;
                my $line_end = index($fbody, "\n", $off);
                $line_end = length($fbody) if $line_end < 0;
                my $line_txt = substr($fbody, $line_start, $line_end - $line_start);
                next if $line_txt =~ /unbounded-external-http:ok/;
                # A .timeout( on the same line satisfies the bound
                # (builder chains and field inits commonly carry it
                # inline).
                next if $line_txt =~ /\.timeout\s*\(/;
                # A tokio::time::timeout wrapper anywhere in the SAME
                # function bounds the awaits made through this client.
                next if $fbody =~ /\btokio::time::timeout\b/;
                my $line = 1 + (substr($scan, 0, $fstart) =~ tr/\n//);
                $line += (substr($fbody, 0, $off) =~ tr/\n//);
                my $stmt = $line_txt;
                $stmt =~ s/^\s+|\s+$//g;
                print "$file:$line: reqwest::Client::new() without .timeout(...) build or tokio::time::timeout wrapper: $stmt\n";
            }
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
    echo "FAIL: outbound HTTP clients without a latency bound."
    echo ""
    echo "reqwest has no default total timeout. A client built with"
    echo "reqwest::Client::new() (no .timeout) on a path with a fallback"
    echo "guarantee turns a hung external service into an indefinite"
    echo "caller stall — the guarantee covers fast errors only (task-107"
    echo "F10: hung Fulcio/Rekor stalled the squash request while the"
    echo "connection-refused fallback test stayed green)."
    echo ""
    echo "Fix: reqwest::Client::builder().timeout(Duration::from_secs(N))"
    echo ".build()?, or wrap external awaits in tokio::time::timeout."
    echo "Do not exempt new sites."
    exit 1
fi

echo "OK: no unbounded reqwest::Client::new() in non-test Rust."
