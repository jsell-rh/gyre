#!/usr/bin/env bash
# check-forged-scope-fields.sh — server-bound identity fields must not
# be read from the request body without a caller-scope guard.
#
# Procedural background (specs/reviews/task-102.md F2):
# record_audit_event (api/audit.rs) carefully bound `agent_id` to the
# verified caller identity server-side (NEW-31 anti-forgery) — and two
# lines later passed `req.workspace_id` / `req.repo_id` straight
# through into the persisted audit event, no existence check, no
# comparison against the caller's own scope. Any authenticated agent
# could attribute its audit events to another workspace, corrupting
# the compliance record the query API then filters on. The recurring
# escape is the "trust the body" reflex on a field the auth extractor
# already knows: tenant_id, workspace_id, repo_id.
#
# Detection (narrow, handler-scoped): a `req.<scope_field>` read in a
# handler that ALSO extracts `AuthenticatedAgent`, where the handler
# body does not contain a caller-scope guard for that field. Guards
# recognized (anywhere in the same handler body, because authorization
# ordering varies):
#   auth.<field> / claims.<field>            — derive-from-caller
#   (check|validate)_<...>_abac|scope        — policy/ABAC evaluation
#   check_tenant / check_workspace           — named containment checks
#   ws.tenant_id == ... auth.tenant_id       — explicit containment compare
#   auth.roles.contains(&UserRole::Admin)    — admin override (guarded)
#   Forbidden/NotFound early-return mentioning the field path     (weak — see below)
# Scope bound = the whole handler body between fn boundaries
# (guards may appear after the read — that is fine for attribution
# flaws; the finding is the ABSENCE of any guard, not its position).
#
# Weak-guard note: an early `NotFound` alone does not prove tenant
# containment, but pairing it with `auth.` on the SAME entity lookup
# (ws.tenant_id != auth.tenant_id) does; the containment-compare
# pattern covers that.
#
# False-positive escape hatch: if a handler legitimately accepts a
# caller-supplied scope field (e.g. an admin endpoint that resolves
# and authorizes the named workspace through ABAC), document it:
#   // forged-scope-fields:ok — <reason>  on the offending line.
#
# Exemptions are legacy debt in
# scripts/forged-scope-fields-exemptions.txt (path:line form,
# frozen count). Fix by deriving the field from the authenticated
# identity or validating it against the caller's scope; never add
# entries.
FROZEN_EXEMPTION_COUNT=2
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/forged-scope-fields-exemptions.txt"
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
    echo "FAIL: forged-scope-fields-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-102 F2), not an approval"
    echo "mechanism. Fix a flagged site by deriving the field from the"
    echo "authenticated identity or validating it against the caller's"
    echo "scope. If you fixed a site, delete the line and lower"
    echo "FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

TMPFILE="$(mktemp /tmp/.forged-scope.XXXXXX)"
trap 'rm -f "$TMPFILE"' EXIT

find "$SRCDIR" -name '*.rs' -print0 | while IFS= read -r -d '' f; do
    perl -0777 -e '
        my $file = $ARGV[0];
        local $/; my $src = <STDIN>;
        # Production code only: cut at first #[cfg(test)].
        my $cut = index($src, "#[cfg(test)]");
        my $scan = $cut >= 0 ? substr($src, 0, $cut) : $src;

        # Split into fn chunks: from each `fn NAME(` keyword to the next
        # fn keyword boundary. Overlap is avoided by consuming matches.
        my @chunks;
        while ($scan =~ /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/gs) {
            my $start = $-[0];
            my $name = $1;
            my $after = $+[0];
            push @chunks, [$start, $name];
        }
        for my $i (0 .. $#chunks) {
            my ($start, $name) = @{$chunks[$i]};
            my $body_end = $i < $#chunks ? $chunks[$i+1][0] : length($scan);
            my $body_start = $start;
            my $body = substr($scan, $body_start, $body_end - $body_start);
            # Only handlers that authenticate the caller.
            next unless $body =~ /\bAuthenticatedAgent\b/;
            for my $field (qw(tenant_id workspace_id repo_id)) {
                while ($body =~ /\breq\.$field\b/gs) {
                    my $off = $-[0];
                    # Guards may appear anywhere in the handler.
                    my $guarded = 0;
                    $guarded = 1 if $body =~ /\b(?:auth|claims|caller)\.$field\b/;
                    $guarded = 1 if $body =~ /\b(?:check|validate)_[A-Za-z_]*(?:abac|scope)/;
                    $guarded = 1 if $body =~ /\bcheck_(?:tenant|workspace|repo)\b/;
                    $guarded = 1 if $body =~ /\bauth\.roles\.contains\([^)]*Admin/;
                    # Explicit containment compare on the named entity:
                    # <entity>.tenant_id != / == ... auth.tenant_id
                    $guarded = 1 if $body =~ /\.\s*$field\b\s*(?:!=|==)\s*[^\n;]{0,80}\bauth\.$field\b/;
                    $guarded = 1 if $body =~ /\bauth\.$field\b\s*(?:!=|==)\s*[^\n;]{0,80}\.\s*$field\b/;
                    # Inline exemption marker on the flagged line.
                    my $line_start = rindex(substr($body, 0, $off), "\n") + 1;
                    my $line_end = index($body, "\n", $off);
                    $line_end = length($body) if $line_end < 0;
                    my $line_txt = substr($body, $line_start, $line_end - $line_start);
                    next if $line_txt =~ /forged-scope-fields:ok/;
                    next if $guarded;
                    my $line = 1 + (substr($scan, 0, $body_start) =~ tr/\n//);
                    $line += (substr($body, 0, $off) =~ tr/\n//);
                    my $stmt = $line_txt;
                    $stmt =~ s/^\s+|\s+$//g;
                    print "$file:$line: handler $name reads req.$field without a caller-scope guard: $stmt\n";
                }
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
    echo "FAIL: request-body scope fields read without a caller-scope guard."
    echo ""
    echo "The auth extractor already knows the caller's tenant/workspace/repo"
    echo "(JWT claims / authenticated identity). A handler that persists or"
    echo "acts on req.<scope_field> without deriving it from the caller or"
    echo "validating it against the caller's scope lets any authenticated"
    echo "agent attribute records to another scope (task-102 F2: audit"
    echo "events attributed to arbitrary workspaces, corrupting the"
    echo "compliance record)."
    echo ""
    echo "Fix: derive the field from the authenticated identity (auth.X /"
    echo "claims.X), or validate the named scope against the caller and"
    echo "return Forbidden on mismatch. Do not exempt new sites."
    exit 1
fi

echo "OK: no unguarded req.<scope_field> reads in authenticated handlers."
