#!/usr/bin/env bash
# Cross-surface parity check: detect frontend Set/array literals of edge-type
# strings that claim parity with a backend Rust constant but have diverged.
#
# Flaw class (specs/reviews/task-062.md F3): a backend semantic constant is
# fixed (e.g., TEST_REACHABILITY_EDGES restricted to Calls-only per
# view-query-grammar.md §3), but the frontend parallel implementation — and its
# "matches backend X" comment — is never swept. The parity comment becomes a
# false assertion about another file's contents, and the two surfaces compute
# different results. Inline/untestable frontend helpers (mirrored-logic tests
# exempt the production path) let the divergence survive review rounds.
#
# Detection signal:
#   1. Parse Rust constants of the form
#        const NAME: &[EdgeType] = &[EdgeType::X, EdgeType::Y, ...];
#      from crates/**/src/*.rs into a NAME -> elements table.
#   2. Scan web/src *.svelte and *.js (excluding __tests__ and node_modules)
#      for `new Set([...])` or array literals whose elements are all
#      snake_case EdgeType strings.
#   3. A literal is a "claimed mirror" when either:
#      a. it is assigned to a variable/const whose name equals a backend
#         constant name (e.g., `const TEST_REACHABILITY_EDGES = new Set([...])`),
#         or
#      b. a comment within 5 lines above the literal names a backend constant
#         (e.g., "matches backend TEST_REACHABILITY_EDGES") or contains
#         "matches backend" / "match backend" / "same as backend" /
#         "as the backend" / "mirrors backend".
#   4. Compare the literal's elements against the backend constant's actual
#      elements. Any difference (missing, extra, or reordered-set inequality)
#      is a violation.
#
# Exemptions:
#   - A `// parity:ok` comment on the same line or within 5 lines above the
#     literal (with a reason: `// parity:ok — intentionally different because ...`)
#   - Files listed in scripts/cross-surface-parity-exemptions.txt
#
# Remediation: either update the frontend literal to match the backend
# constant (and update the comment), or remove the parity claim. If the
# divergence is intentional, remove the "matches backend" comment and document
# the intentional difference at the site.
#
# See: specs/reviews/task-062.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$REPO_ROOT/crates"
WEB_DIR="$REPO_ROOT/web/src"
EXEMPTIONS_FILE="$SCRIPT_DIR/cross-surface-parity-exemptions.txt"

if [ ! -d "$WEB_DIR" ]; then
    echo "web/src not found; skipping cross-surface parity check."
    exit 0
fi

# Load exemption list (file paths, one per line, # comments allowed)
EXEMPTED_FILES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_FILES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for cross-surface parity drift between backend EdgeType constants and frontend mirrors..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# ---------------------------------------------------------------------------
# Phase 1: parse Rust EdgeType constants.
#   Input:  const NAME: &[EdgeType] = &[EdgeType::X, EdgeType::Y];
#   Output: "NAME<TAB>x<TAB>y" (variants converted to snake_case)
# ---------------------------------------------------------------------------
RUST_CONSTS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE" "$RUST_CONSTS_FILE"' EXIT

find "$CRATES_DIR" -path '*/src/*.rs' -type f 2>/dev/null | sort | while IFS= read -r rs; do
    awk -v file="$rs" '
    {
        # Join multi-line const definitions: keep appending while the
        # statement has not been terminated by a semicolon.
        if (pending != "") {
            pending = pending " " $0
        } else if ($0 ~ /^const [A-Z][A-Z0-9_]*:\s*&\[EdgeType\]/) {
            pending = $0
        }
        if (pending != "" && pending ~ /;/) {
            line = pending
            pending = ""
            if (match(line, /^const ([A-Z][A-Z0-9_]*):\s*&\[EdgeType\]\s*=\s*&\[([^\]]*)\]/, m)) {
                name = m[1]
                body = m[2]
                n = split(body, parts, ",")
                elems = ""
                for (i = 1; i <= n; i++) {
                    v = parts[i]
                    gsub(/^[ \t]+|[ \t]+$/, "", v)
                    if (v == "") continue
                    if (!match(v, /^EdgeType::[A-Za-z]+$/)) { elems = ""; break }
                    variant = substr(v, index(v, "::") + 2)
                    # CamelCase -> snake_case: DependsOn -> depends_on
                    # (char-wise: gsub has no backreferences)
                    snake = ""
                    for (k = 1; k <= length(variant); k++) {
                        c = substr(variant, k, 1)
                        if (c ~ /[A-Z]/ && k > 1) snake = snake "_"
                        snake = snake tolower(c)
                    }
                    elems = (elems == "") ? snake : elems "\t" snake
                }
                if (elems != "") print name "\t" elems
            }
        }
    }
    ' "$rs" 2>/dev/null
done > "$RUST_CONSTS_FILE"

# ---------------------------------------------------------------------------
# Phase 2: scan frontend files for claimed mirrors and compare.
# ---------------------------------------------------------------------------
# EdgeType snake_case universe (crates/gyre-common/src/graph.rs, serde snake_case)
EDGE_UNIVERSE='contains|implements|depends_on|calls|field_of|returns|routes_to|renders|persists_to|governed_by|produced_by'

is_exempted_file() {
    [ -z "$EXEMPTED_FILES" ] && return 1
    while IFS= read -r exempt; do
        [ -z "$exempt" ] && continue
        [[ "$1" == *"$exempt"* ]] && return 0
    done <<< "$EXEMPTED_FILES"
    return 1
}

backend_const_names=""
if [ -s "$RUST_CONSTS_FILE" ]; then
    backend_const_names=$(cut -f1 "$RUST_CONSTS_FILE" | sort -u)
fi

for file in $(find "$WEB_DIR" \( -name '*.svelte' -o -name '*.js' \) -not -path '*__tests__*' -not -path '*node_modules*' -type f 2>/dev/null | sort); do
    is_exempted_file "$file" && continue

    [ -z "$backend_const_names" ] && break

    # For each backend constant, look for a claimed mirror in this file.
    while IFS= read -r const_name; do
        [ -z "$const_name" ] && continue

        # For each backend constant, look for a claimed mirror in this file.
        # Only literals whose elements are ALL EdgeType strings count — a
        # node-type or UI Set sitting under the same comment window must not
        # be swept in as a claimed edge-set mirror.
        candidates=$(awk -v const_name="$const_name" -v edge_universe="^($EDGE_UNIVERSE)$" '
        {
            lines[NR] = $0
        }
        END {
            for (i = 1; i <= NR; i++) {
                line = lines[i]
                # Must be a Set/array literal of quoted strings
                if (line !~ /new Set\(\[|=\s*\[/) continue
                if (line !~ /\x27[^\x27]*\x27/) continue
                # All single-quoted elements must be EdgeType strings
                all_edges = 1
                s = line
                elem_count = 0
                while (match(s, /\x27([^\x27]*)\x27/, m)) {
                    v = m[1]
                    s = substr(s, RSTART + RLENGTH)
                    elem_count++
                    if (v !~ edge_universe) { all_edges = 0; break }
                }
                if (elem_count == 0) all_edges = 0
                if (!all_edges) continue
                # Claim 1: assigned to a variable named like the backend const
                assigned = (line ~ ("(const|let|var)[ \t]+" const_name "[ \t]*="))
                # Claim 2: a comment in the contiguous comment block directly
                # above the literal (within 5 lines, blanks skipped, code ends
                # the window) names the backend const or claims backend parity
                claim_comment = 0
                for (j = i - 1; j >= 1 && j >= i - 5; j--) {
                    if (lines[j] ~ /^[ \t]*$/) continue
                    if (lines[j] !~ /^[ \t]*\/\//) break
                    if (lines[j] ~ ("(^|[^A-Za-z0-9_])" const_name "([^A-Za-z0-9_]|$)")) { claim_comment = 1; break }
                    # Generic "matches backend" phrases only claim when the
                    # comment names no other SCREAMING_CASE constant.
                    if (lines[j] ~ /[Mm]atches backend|[Mm]atch backend|mirrors backend|same as backend|as the backend/) {
                        other = 0
                        cline = lines[j]
                        sub(/^[ \t]*\/\/[ \t]*/, "", cline)
                        while (match(cline, /[A-Z][A-Z0-9_]{2,}/, om)) {
                            oc = om[1]
                            cline = substr(cline, RSTART + RLENGTH)
                            if (oc != const_name) { other = 1; break }
                        }
                        if (!other) { claim_comment = 1; break }
                    }
                }
                if (assigned || claim_comment) print i
            }
        }
        ' "$file" 2>/dev/null)

        [ -z "$candidates" ] && continue

        backend_elems=$(awk -F'\t' -v name="$const_name" '$1 == name { for (i = 2; i <= NF; i++) printf "%s%s", $i, (i < NF ? "\t" : "\n"); exit }' "$RUST_CONSTS_FILE")
        [ -z "$backend_elems" ] && continue

        while IFS= read -r lineno; do
            [ -z "$lineno" ] && continue
            # parity:ok inline exemption (same line or up to 5 lines above)
            if awk -v target="$lineno" '
                { lines[NR] = $0 }
                END {
                    for (j = (target > 5 ? target - 5 : 1); j <= target; j++)
                        if (lines[j] ~ /parity:ok/) exit 1
                    exit 0
                }' "$file" 2>/dev/null; then
                :
            else
                continue
            fi


            # Extract frontend elements from the literal on this line
            frontend_elems=$(awk -v lineno="$lineno" '
            NR == lineno {
                # Extract all single-quoted strings on the line
                s = $0
                out = ""
                while (match(s, /\x27([^\x27]*)\x27/, m)) {
                    v = m[1]
                    s = substr(s, RSTART + RLENGTH)
                    if (out == "") out = v; else out = out "\t" v
                }
                print out
            }' "$file" 2>/dev/null)

            [ -z "$frontend_elems" ] && continue

            # Compare as sets
            mismatch=$(awk -v backend="$backend_elems" -v frontend="$frontend_elems" -F'\t' '
            BEGIN {
                split(backend, b, "\t"); split(frontend, f, "\t")
                for (i in b) want[b[i]] = 1
                for (i in f) got[f[i]] = 1
                diff = 0
                for (e in want) if (!(e in got)) { diff = 1; missing = missing (missing==""?"":",") e }
                for (e in got) if (!(e in want)) { diff = 1; extra = extra (extra==""?"":",") e }
                if (diff) {
                    msg = ""
                    if (missing != "") msg = "missing " missing
                    if (extra != "") msg = msg (msg==""?"":"; ") "unexpected " extra
                    print msg
                }
            }')
            if [ -n "$mismatch" ]; then
                src_line=$(sed -n "${lineno}p" "$file" | sed 's/^[ \t]*//')
                echo "$file:$lineno: frontend mirror of backend constant $const_name has diverged ($mismatch)" >> "$HITS_FILE"
                echo "    backend: $(echo "$backend_elems" | tr '\t' ',')" >> "$HITS_FILE"
                echo "    site:    $src_line" >> "$HITS_FILE"
            fi
        done <<< "$candidates"
    done <<< "$backend_const_names"
done

if [ -s "$HITS_FILE" ]; then
    echo
    echo "VIOLATION: cross-surface parity drift detected."
    echo
    cat "$HITS_FILE"
    echo
    echo "A frontend Set/array of edge types claims parity with a backend Rust constant"
    echo "(by name or via a \"matches backend\" comment) but its contents differ."
    echo
    echo "Remediation:"
    echo "  1. Update the frontend literal to match the backend constant and fix the"
    echo "     comment (cite the spec section that defines the semantics)."
    echo "  2. If the divergence is intentional, remove the parity claim and document"
    echo "     why the surfaces differ at the site."
    echo "  3. For intentional divergence with a remaining parity claim, add a"
    echo "     \`// parity:ok — <reason>\` comment or list the file in"
    echo "     scripts/cross-surface-parity-exemptions.txt."
    echo
    echo "See: specs/reviews/task-062.md F3 (cross-surface parity staleness)."
    exit 1
fi

echo "Cross-surface parity check passed."
exit 0
