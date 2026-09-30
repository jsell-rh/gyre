#!/usr/bin/env bash
# Architecture lint: every MessageKind variant must be EMITTED somewhere —
# dead enum variants mean a spec-named event is never delivered.
#
# gyre-common's MessageKind enum is the wire vocabulary of the message bus.
# Each Event/Directed-tier kind corresponds to a spec-defined notification
# ("X receives Y via MCP" / "broadcast Z to the workspace"). The spec names
# the DELIVERY, the enum names the KIND — if a variant is defined but no
# production code ever constructs it, the spec's delivery chain has a dead
# link and consumers (MCP SSE stream, WebSocket dashboards) can never see it
# (task-095 review F3: MessageKind::MrReverted existed, spec §6 step 6 said
# "Author agent receives RevertNotification via MCP", but zero emitters —
# the notification function wrote user-inbox rows only, so the MCP relay
# could never fire).
#
# This is the inverse of check-missing-domain-events.sh (which detects
# emitters that should exist); this detects kinds that exist but never
# emit. A variant must have at least one EMITTER construction site — a use
# in expression position (`kind: MessageKind::X`, `MessageKind::X,` as an
# emit argument) — outside gyre-common/src/message.rs itself. References in
# PATTERN position (match arms like `MessageKind::X => {...}`) are consumers,
# not emitters, and do not count.
#
# Tier-3 telemetry kinds (ToolCallStart, ToolCallEnd, TextMessageContent,
# RunStarted, RunFinished, StateChanged) are constructible by any client
# via the A2A/telemetry submit endpoints and Custom(String) is open-ended —
# both are exempt from the emitter requirement.
#
# Exemptions: scripts/dead-message-kind-exemptions.txt (one variant per
# line, with justification). Exemptions should SHRINK, never grow.
#
# Run by pre-commit and CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MESSAGE_RS="crates/gyre-common/src/message.rs"
EXEMPTIONS_FILE="$SCRIPT_DIR/dead-message-kind-exemptions.txt"

FAIL=0

if [ ! -f "$MESSAGE_RS" ]; then
    echo "FAIL: $MESSAGE_RS not found (run from repo root)"
    exit 1
fi

# Kinds that are client-suppliable (telemetry tier / open Custom variant) —
# no server emitter required.
CLIENT_SUPPLIABLE="ToolCallStart ToolCallEnd TextMessageContent RunStarted RunFinished StateChanged Custom"

# ── Enumerate variants ──────────────────────────────────────────────────
# The enum block starts at 'pub enum MessageKind {' and ends at the first
# column-0 '}'. Variants are 4-space-indented identifiers ending in ','.
ENUM_START=$(grep -n '^pub enum MessageKind {' "$MESSAGE_RS" | cut -d: -f1)
ENUM_END=$(awk -v s="$ENUM_START" 'NR > s && /^}/ {print NR; exit}' "$MESSAGE_RS")

if [ -z "$ENUM_START" ] || [ -z "$ENUM_END" ]; then
    echo "FAIL: could not locate MessageKind enum block in $MESSAGE_RS"
    exit 1
fi

sed -n "${ENUM_START},${ENUM_END}p" "$MESSAGE_RS" \
    | grep -oE '^    [A-Z][A-Za-z0-9]*,' | tr -d ' ,' > /tmp/.msgkinds-variants.$$

# ── Collect emitter sites ───────────────────────────────────────────────
# Emitter = value-position use of MessageKind::X outside the enum's own
# file. We approximate by rejecting the two consumer shapes:
#   - match-arm patterns: 'MessageKind::X =>' or '| MessageKind::X =>'
#   - assert/assert_eq comparisons: 'assert*(*MessageKind::X' / '== MessageKind::X'
# Everything else (struct field init `kind: MessageKind::X,` or bare
# `MessageKind::X,` as a call argument) is an emitter.
grep -rn 'MessageKind::' crates web/src --include='*.rs' 2>/dev/null \
    | grep -v "^$MESSAGE_RS" \
    | grep -vE 'MessageKind::[A-Za-z0-9]+ *=>' \
    | grep -vE '(\| *MessageKind::|== *MessageKind::|!= *MessageKind::|assert[^ ]*\(.*MessageKind::)' \
    | grep -vE '/tests?/|_test\.rs' \
    | grep -oE 'MessageKind::[A-Za-z0-9]+' | sort -u > /tmp/.msgkinds-emitters.$$

# ── Compare ─────────────────────────────────────────────────────────────
dead=""
while IFS= read -r variant; do
    # Client-suppliable kinds need no server emitter.
    case " $CLIENT_SUPPLIABLE " in
        *" $variant "*) continue ;;
    esac
    # Exempted kinds (documented legacy).
    if [ -f "$EXEMPTIONS_FILE" ] \
        && grep -qE "^\s*$variant\s*(#.*)?$" "$EXEMPTIONS_FILE"; then
        continue
    fi
    if ! grep -q "^MessageKind::$variant$" /tmp/.msgkinds-emitters.$$; then
        dead="$dead $variant"
    fi
done < /tmp/.msgkinds-variants.$$

rm -f /tmp/.msgkinds-variants.$$ /tmp/.msgkinds-emitters.$$

if [ -n "$dead" ]; then
    echo "FAIL: MessageKind variants with no emitter outside $MESSAGE_RS:"
    for v in $dead; do
        echo "  MessageKind::$v"
    done
    echo ""
    echo "A message kind that is never constructed is a dead spec link: the spec's"
    echo "notification ('receives X via MCP/bus') can never be delivered because no"
    echo "code emits the event. Either emit it at the spec-defined moment"
    echo "(state.emit_event(..., MessageKind::X, ...)) or remove the variant."
    echo "If the kind is intentionally emitterless (documented legacy), add it to"
    echo "$EXEMPTIONS_FILE with justification."
    exit 1
fi

echo "OK: every MessageKind variant has an emitter (or documented exemption)."
