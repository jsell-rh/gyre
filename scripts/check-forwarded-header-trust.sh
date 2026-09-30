#!/usr/bin/env bash
# check-forwarded-header-trust.sh — client-controlled forwarding headers
# must not be trusted without a trusted-proxy gate.
#
# Procedural background (specs/reviews/task-102.md F3):
# record_audit_event (api/audit.rs) derived `source_ip` from
# X-Forwarded-For (first hop), then X-Real-Ip, unconditionally — in
# the default deployment (no reverse proxy, no trusted-proxy list
# anywhere in the server), a direct-connect client sets
# `X-Forwarded-For: <arbitrary>` and the forged value is persisted as
# the event's source_ip and forwarded to SIEM as `src=` / `source_ip`.
# The spec added source_ip for forensic attribution; persisting an
# unverified caller claim is audit forgery (CWE-348) — in the SAME
# handler that binds agent_id server-side (NEW-31) to prevent exactly
# this class. The standard pattern: record the socket peer address
# (ConnectInfo<SocketAddr>), and honor forwarded headers only when the
# immediate peer is a configured trusted proxy.
#
# Detection: any read of X-Forwarded-For / X-Real-Ip / Forwarded
# headers in NON-TEST Rust. The trusted-proxy gate is configuration
# state, not a syntactic marker, so a read is flagged unless its line
# (or the line immediately above) carries the inline marker:
#   // forwarded-header-trust:ok — peer is validated against trusted proxies
# Reads inside #[cfg(test)] modules are excluded (tests legitimately
# forge headers).
#
# Exemptions are legacy debt in
# scripts/forwarded-header-trust-exemptions.txt (path:line form,
# frozen count). Fix by using the socket peer address and gating
# forwarded headers on a trusted-proxy list; never add entries.
FROZEN_EXEMPTION_COUNT=2
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/forwarded-header-trust-exemptions.txt"
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
    echo "FAIL: forwarded-header-trust-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-102 F3), not an approval"
    echo "mechanism. Fix a flagged site by recording the socket peer address"
    echo "and honoring forwarded headers only behind a trusted-proxy gate."
    echo "If you fixed a site, delete the line and lower"
    echo "FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

TMPFILE="$(mktemp /tmp/.fwd-hdr.XXXXXX)"
trap 'rm -f "$TMPFILE"' EXIT

find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Production code only: cut at first #[cfg(test)].
        my $cut = index($src, "#[cfg(test)]");
        my $scan = $cut >= 0 ? substr($src, 0, $cut) : $src;
        # Match actual header reads: .get("x-forwarded-for") / typed
        # HeaderName / forwarded("...") constants — not prose that
        # merely mentions the word "forwarded".
        while ($scan =~ /\.get\(\s*"(?:x-forwarded-for|x-real-ip|forwarded)"\s*\)|\.get\(\s*header::(?:X_FORWARDED_FOR|X_REAL_IP|FORWARDED)\s*\)|"(?:x-forwarded-for|x-real-ip)"\s*\.into\(\)|HeaderName::from_static\(\s*"(?:x-forwarded-for|x-real-ip|forwarded)"\s*\)/gi) {
            my $start = $-[0];
            my $line = 1 + (substr($scan, 0, $start) =~ tr/\n//);
            my $line_start = rindex(substr($scan, 0, $start), "\n") + 1;
            my $line_end = index($scan, "\n", $start);
            $line_end = length($scan) if $line_end < 0;
            my $line_txt = substr($scan, $line_start, $line_end - $line_start);
            # The marker may sit on the flagged line OR on a comment line
            # directly above it (the natural place to explain WHY the read
            # is safe). "In test mod" case: nothing to do — #[cfg(test)]
            # was already cut.
            my $prev_line = "";
            my $prev_start = rindex(substr($scan, 0, $line_start - 1), "\n") + 1;
            $prev_line = substr($scan, $prev_start, $line_start - $prev_start - 1) if $line_start > 0;
            next if $line_txt =~ /forwarded-header-trust:ok/;
            next if $prev_line =~ /forwarded-header-trust:ok/;
            my $stmt = $line_txt;
            $stmt =~ s/^\s+|\s+$//g;
            print "$file:$line: $stmt\n";
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
    echo "FAIL: forwarding headers (X-Forwarded-For / X-Real-Ip / Forwarded)"
    echo "trusted without a trusted-proxy gate."
    echo ""
    echo "These headers are caller-controlled. In the default deployment"
    echo "(no reverse proxy) any client can forge the value that is then"
    echo "persisted as source_ip and shipped to SIEM (task-102 F3, CWE-348)."
    echo "The same handler class already binds agent_id server-side; the"
    echo "peer address deserves the same treatment."
    echo ""
    echo "Fix: record the socket peer address (ConnectInfo<SocketAddr>) and"
    echo "honor forwarded headers only when the immediate peer is a"
    echo "configured trusted proxy (e.g. GYRE_TRUSTED_PROXIES CIDR list,"
    echo "empty by default = always use the peer address). Do not exempt"
    echo "new sites."
    exit 1
fi

echo "OK: no un-gated forwarded-header trust."
