#!/usr/bin/env bash
# check-lossy-secret-conversion.sh — byte-oriented security material
# (secret values) must not pass through String::from_utf8_lossy.
#
# Procedural background (specs/reviews/task-097.md F4):
# the spawn secret-injection loop converted each resolved secret value
# with `String::from_utf8_lossy(&value)` (api/spawn.rs). The port
# contract passes values as Vec<u8> and neither spec nor task restricts
# secret values to UTF-8 — a binary secret was silently mangled with
# U+FFFD replacement characters, no error, no log, and the agent then
# used a corrupted credential and failed opaquely downstream.
#
# Detection: any `from_utf8_lossy` applied to a secret-domain value —
# a binding named `value`/`v`/`*secret*` inside a loop over a
# `resolve_for_agent` result, or an argument whose name mentions
# secret/credential/token/key material. This is deliberately NARROW:
# from_utf8_lossy on subprocess stdout/stderr and file reads is the
# established, legitimate pattern (~100 sites) and is NOT flagged.
# Only the security-material conversion is a defect.
#
# If a conversion is INTENTIONALLY lossy-tolerant on non-security
# material, it will not match the secret-name pattern. If a flagged
# site must be exempted: // lossy-secret-conversion:ok — <reason>
# on the conversion line.
#
# Exemptions are legacy debt in
# scripts/lossy-secret-conversion-exemptions.txt (path:line form,
# frozen count). Fix by skipping the secret and logging a warning
# naming the secret (not the value); never add entries.
FROZEN_EXEMPTION_COUNT=1
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/lossy-secret-conversion-exemptions.txt"
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
    echo "FAIL: lossy-secret-conversion-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-097 F4), not an approval"
    echo "mechanism. Fix a flagged site by skipping the secret and logging a"
    echo "warning that NAMES the secret (never the value); never by exempting"
    echo "it. If you fixed a site, delete the line and lower"
    echo "FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

# Match `from_utf8_lossy( ... )` whose argument or enclosing binding
# carries a secret-domain name (secret, cred, token, api_key, password,
# credential), excluding test modules. Statement-level scan; the
# argument cannot contain `;` so the non-greedy match stays within one
# call.
find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Skip everything from the first #[cfg(test)] mod to end of file
        # (test modules are trailing in this codebase; a mid-file test
        # mod followed by more production code is not a shape here).
        my $cut = index($src, "#[cfg(test)]");
        my $scan = $cut >= 0 ? substr($src, 0, $cut) : $src;
        while ($scan =~ /from_utf8_lossy\s*\(\s*&?([A-Za-z_][A-Za-z0-9_.]*)/g) {
            my $arg = $1;
            if ($arg =~ /secret|cred|token|api_key|password|credential/i) {
                my $start = $-[0];
                my $line = 1 + (substr($scan, 0, $start) =~ tr/\n//);
                # line context for inline-exemption check
                my $ctx = $scan;
                my $stmt = substr($src, $start, 120);
                $stmt =~ s/\s+/ /g;
                print "$file:$line:ARG:$arg: $stmt\n";
            }
        }
        # Also match tuple destructuring over resolve_for_agent results:
        # for (name, value) in resolved { ... from_utf8_lossy(&value) }
        # The value binding is generic, so bind on the enclosing
        # resolve_for_agent call within 1500 chars before the conversion.
        while ($scan =~ /from_utf8_lossy\s*\(\s*&?(value|v)\b/g) {
            my $conv_pos = $-[0];
            my $window = substr($scan, $conv_pos > 1500 ? $conv_pos - 1500 : 0, 1500);
            if ($window =~ /resolve_for_agent/) {
                my $line = 1 + (substr($scan, 0, $conv_pos) =~ tr/\n//);
                my $stmt = substr($src, $conv_pos, 120);
                $stmt =~ s/\s+/ /g;
                print "$file:$line:RESOLVE: $stmt\n";
            }
        }
    ' "$f" < "$f"
done > /tmp/.lossy-secret.$$

while IFS= read -r line; do
    [ -n "$line" ] || continue
    loc="$(echo "$line" | cut -d: -f1,2)"
    if [ -z "${EXEMPT["$loc"]:-}" ]; then
        # inline exemption check: the marker on the flagged line
        fpath="$(echo "$line" | cut -d: -f1)"
        lineno="$(echo "$line" | cut -d: -f2)"
        ctx="$(sed -n "${lineno}p" "$fpath" 2>/dev/null || true)"
        case "$ctx" in
            *lossy-secret-conversion:ok*) continue ;;
        esac
        echo "$line" >> /tmp/.lossy-secret-violations.$$
        FAIL=1
    fi
done < /tmp/.lossy-secret.$$

rm -f /tmp/.lossy-secret.$$
if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: from_utf8_lossy applied to secret-domain byte material:"
    echo ""
    cat /tmp/.lossy-secret-violations.$$ 2>/dev/null || true
    echo ""
    echo "A secret value is Vec<u8>; neither spec nor task restricts secrets"
    echo "to UTF-8. A lossy conversion silently corrupts binary secrets with"
    echo "U+FFFD and the agent uses a corrupted credential (task-097 F4)."
    echo "Skip the secret and log a warning NAMING the secret (never the"
    echo "value), mirroring the resolve-failure availability posture."
    exit 1
fi

rm -f /tmp/.lossy-secret-violations.$$
echo "OK: no lossy from_utf8_lossy conversion of secret-domain byte material."
