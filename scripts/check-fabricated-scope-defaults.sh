#!/usr/bin/env bash
# check-fabricated-scope-defaults.sh — a lookup failure must not fabricate a
# tenant/workspace scope identity via a string-literal fallback.
#
# Procedural background (specs/reviews/task-097.md F3):
# spawn_agent_core computed `tenant_id = workspace.map(...).unwrap_or_else(||
# "default".to_string())` after a `.ok().flatten()` workspace fetch. A
# transient store error silently re-targeted tenant-scope secret resolution
# at a fabricated "default" tenant: the real tenant's secrets silently stop
# applying, and if a real tenant is named "default" its secrets leak to
# agents whose workspace could not be resolved. The same class existed at
# constraint_check.rs:1364 (tenant resolution fallback) — unflagged by the
# review that caught the spawn site, which is exactly why this check is
# repo-wide rather than task-scoped. The correct pattern exists in
# lib.rs `emit_reconciliation_completed`: when the workspace cannot be
# resolved, SKIP the tenant-scoped operation and log — never invent an
# identity to resolve against.
#
# Flagged: any `unwrap_or("default")` / `unwrap_or_else(|| "default"...)`
# / `Id::new("default")` fallback where the receiver is a tenant_id,
# workspace_id / ws_id, or repo_id binding (the scope-identity names), OR
# where the fallback statement mentions tenant/workspace resolution
# context. Mechanical scope: the `.ok().flatten()` fetch-and-fallback
# statement is the dominant failure shape; binding-name matching keeps the
# check on tenancy/scope identity and away from benign defaults (CLI
# workspace-name UX default, JWT `kid` header default, agent token
# fallback claims — these are pre-existing, exemption-seeded below).
#
# Allowed patterns: propagate the error (`?`), skip the operation and log,
# or return an explicit error, as `emit_reconciliation_completed` does.
#
# Exemptions are legacy debt in
# scripts/fabricated-scope-defaults-exemptions.txt (path:line form,
# frozen count). Fix a flagged site by skipping/logging; never add entries.
FROZEN_EXEMPTION_COUNT=7
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/fabricated-scope-defaults-exemptions.txt"
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
    echo "FAIL: fabricated-scope-defaults-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-097 F3), not an approval"
    echo "mechanism. Fix a flagged site by skipping the scope-scoped operation"
    echo "and logging (see emit_reconciliation_completed, lib.rs), never by"
    echo "exempting it. If you fixed a site, delete the line and lower"
    echo "FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

# Match `<binding> = <expr>.unwrap_or("default")...` where the binding is a
# scope identity (tenant_id / workspace_id / ws_id / repo_id), the fallback
# is a literal "default" (String or Id::new), and no `;`/`{`/`}` intervenes
# between the let-binding and the fallback. Statement-level matching is
# robust against rustfmt's multi-line continuations (.await / .unwrap_*
# on their own lines).
find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Blank string-literal contents EXCEPT "default" (length- and
        # line-preserving) so braces inside format!("...{}") do not read
        # as block openers while the "default" fallback stays matchable.
        my $scan = $src;
        $scan =~ s/("(?:[^"\\]|\\.)*")/ my $t = $1; $t eq "\"default\"" ? $t : do { $t =~ s\/[^"\n]\/x\/g; $t } /ges;
        while ($scan =~ /
            (?:let\s+)?(?:mut\s+)?(tenant_id|workspace_id|ws_id|repo_id)\s*=\s*
            [;{}]?(?:(?!;|\{|\}).)*?
            (?:\.unwrap_or\("(?:default)"\)
              |\.unwrap_or_else\(\|\|\s*(?:"default"\.to_string\(\)|[A-Za-z_:]+::new\("default"\))\)
              |\.unwrap_or\(gyre_common::Id::new\("default"\)\)
            )
        /gsx) {
            my $start = $-[0];
            my $line = 1 + (substr($scan, 0, $start) =~ tr/\n//);
            my $stmt = substr($src, $start, $+[0] - $start);
            $stmt =~ s/\s+/ /g;
            print "$file:$line: $stmt\n";
        }
    ' "$f" < "$f"
done > /tmp/.fabricated-scope.$$

while IFS= read -r line; do
    [ -n "$line" ] || continue
    loc="$(echo "$line" | cut -d: -f1,2)"
    if [ -z "${EXEMPT["$loc"]:-}" ]; then
        echo "$line" >> /tmp/.fabricated-scope-violations.$$
        FAIL=1
    fi
done < /tmp/.fabricated-scope.$$

rm -f /tmp/.fabricated-scope.$$
if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: fabricated \"default\" tenant/workspace scope identity on lookup failure:"
    echo ""
    cat /tmp/.fabricated-scope-violations.$$ 2>/dev/null || true
    echo ""
    echo "A lookup failure means the scope cannot be determined — there is no"
    echo "valid literal-\"default\" tenant or workspace. Fabricating one silently"
    echo "re-targets the operation (and leaks data if a real scope is named"
    echo "\"default\" — task-097 F3). Skip the scope-scoped operation and log,"
    echo "or propagate the error, as emit_reconciliation_completed does."
    exit 1
fi

rm -f /tmp/.fabricated-scope-violations.$$
echo "OK: no fabricated \"default\" tenant/workspace scope-identity fallbacks."
