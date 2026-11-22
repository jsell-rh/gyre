#!/usr/bin/env bash
# Toggle-coverage lint: the HSI §12 notification-preferences UI must offer a
# toggle for EVERY canonical NotificationType variant.
#
# The profile UI renders its per-type toggles from a canonical id list
# (web/src/lib/notification-types.js), and each toggle needs a localized
# label under user_profile.notif_types in web/src/locales/en.json. Both are
# hand-maintained against the Rust enum in
# crates/gyre-common/src/notification.rs -- the backend PUT rejects
# non-canonical names, so a missing or misspelled entry means the human
# cannot disable that notification type at all (a silent spec gap), and a
# stray legacy parse alias ('SpecPendingApproval'/'BudgetWarning') is a
# toggle that never matches a stored notification (silently inert).
#
# This check fails when:
#   1. a NotificationType variant has no toggle id (or vice versa), or
#   2. a toggle id has no user_profile.notif_types label (or vice versa).
#
# See: task-209 review F1 -- 'ComputeTargetUnavailable' (a Priority-1 inbox
# type) shipped with no toggle and no label, and the test hardcoded the
# implementation's count, so nothing failed.
#
# Run by pre-commit and CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ENUM_FILE="$REPO_ROOT/crates/gyre-common/src/notification.rs"
LIST_FILE="$REPO_ROOT/web/src/lib/notification-types.js"
LOCALE_FILE="$REPO_ROOT/web/src/locales/en.json"

if [ ! -f "$ENUM_FILE" ] || [ ! -f "$LIST_FILE" ] || [ ! -f "$LOCALE_FILE" ]; then
    echo "Skipping notification toggle coverage check: source files not found"
    exit 0
fi

FAIL=0

# ── 1. Enum variants ↔ toggle ids ───────────────────────────────────────────
# Unit variants are indented 4 spaces under `pub enum NotificationType {`.
ENUM_VARIANTS=$(sed -n '/^pub enum NotificationType {/,/^}/p' "$ENUM_FILE" \
    | grep -oE '^    [A-Z][A-Za-z0-9]+,?$' \
    | tr -d ' ,' | sort)

# Ids are the quoted strings inside the exported NOTIF_TYPE_IDS array.
LIST_IDS=$(sed -n '/^export const NOTIF_TYPE_IDS = \[/,/^\];/p' "$LIST_FILE" \
    | grep -oE "'[A-Za-z0-9]+'" | tr -d "'" | sort)

if [ -z "$ENUM_VARIANTS" ]; then
    echo "FAIL: could not extract NotificationType variants from $ENUM_FILE"
    exit 1
fi
if [ -z "$LIST_IDS" ]; then
    echo "FAIL: could not extract NOTIF_TYPE_IDS from $LIST_FILE"
    exit 1
fi

MISSING_TOGGLES=$(comm -23 <(echo "$ENUM_VARIANTS") <(echo "$LIST_IDS"))
if [ -n "$MISSING_TOGGLES" ]; then
    echo "FAIL: NotificationType variants with no profile toggle (HSI §12 requires a toggle for EVERY variant):"
    echo "$MISSING_TOGGLES" | sed 's/^/  /'
    echo "  → add the id to web/src/lib/notification-types.js and a user_profile.notif_types label to web/src/locales/en.json"
    FAIL=1
fi

STRAY_IDS=$(comm -13 <(echo "$ENUM_VARIANTS") <(echo "$LIST_IDS"))
if [ -n "$STRAY_IDS" ]; then
    echo "FAIL: toggle ids that match no NotificationType variant (inert toggles / parse aliases):"
    echo "$STRAY_IDS" | sed 's/^/  /'
    echo "  → remove them from web/src/lib/notification-types.js"
    FAIL=1
fi

# ── 2. Toggle ids ↔ locale labels ───────────────────────────────────────────
MISSING_LABELS=$(python3 - "$LIST_FILE" "$LOCALE_FILE" <<'EOF'
import json
import re
import sys

list_file, locale_file = sys.argv[1], sys.argv[2]
src = open(list_file).read()
body = re.search(r'NOTIF_TYPE_IDS = \[(.*?)\];', src, re.S).group(1)
ids = re.findall(r"'([A-Za-z0-9]+)'", body)

labels = json.load(open(locale_file))["user_profile"]["notif_types"]

for i in ids:
    if i not in labels:
        print(f"  {i}")
for extra in sorted(set(labels) - set(ids)):
    print(f"  {extra} (label with no toggle)")
EOF
)

if [ -n "$MISSING_LABELS" ]; then
    echo "FAIL: user_profile.notif_types label mismatches (missing labels, or labels with no toggle):"
    echo "$MISSING_LABELS"
    echo "  → keep web/src/locales/en.json user_profile.notif_types in lockstep with web/src/lib/notification-types.js"
    FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
    exit 1
fi

ENUM_COUNT=$(echo "$ENUM_VARIANTS" | wc -l)
LIST_COUNT=$(echo "$LIST_IDS" | wc -l)
echo "Notification toggle coverage OK: $ENUM_COUNT enum variants, $LIST_COUNT toggles, labels in lockstep."
