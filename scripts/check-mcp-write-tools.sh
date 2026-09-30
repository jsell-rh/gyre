#!/usr/bin/env bash
# check-mcp-write-tools.sh — Verify that every MCP tool whose handler performs
# write effects is listed in the `needs_write` RBAC gate in mcp.rs.
#
# Procedural background (specs/reviews/task-093.md F7): `gyre_message_send`
# writes a message via `state.messages.store(&msg)` but was never added to
# the `needs_write` match arm list — so any ReadOnly principal could send
# (write) messages while the gate only guarded the ack side of the same
# mailbox. The tool was added to the dispatch match but not to the gate,
# and nothing derived one from the other: two hand-maintained lists with no
# mechanical link.
#
# The fix-class rule: gate membership is DERIVED, not hand-maintained.
#   1. Parse the `tools/call` dispatch match arms in mcp_handler
#      ("tool_name" => handle_x(...)).
#   2. Parse the `needs_write` match arm list.
#   3. For each dispatched handler fn, scan its body for repository write
#      calls: `state.<repo>.create|store|update|upsert|delete|remove|save|insert|append`
#      (method-call form, `state.x.create(` etc.).
#   4. A handler with write effects whose tool is absent from the
#      needs_write list is a violation.
#
# NOT flagged:
#   - Read-only handlers (no write-effect calls).
#   - Tools listed in scripts/mcp-write-tools-exemptions.txt (one tool name
#     per line, `#` comments allowed). Every entry there is a real F7-class
#     hazard exempted only so the check runs green while the fix is owned by
#     the recorded task — delete the entry when the tool is added to the gate.
#   - Handler fns inside #[cfg(test)]/mod tests regions.
#
# Run by pre-commit and CI.

set -uo pipefail

MCP="crates/gyre-server/src/mcp.rs"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPT_FILE="$SCRIPT_DIR/mcp-write-tools-exemptions.txt"

if [ ! -f "$MCP" ]; then
    echo "FAIL: $MCP not found (run from repo root)"
    exit 1
fi

MCP="$MCP" EXEMPT_FILE="$EXEMPT_FILE" python3 - <<'PYEOF'
import os
import re
import sys

mcp_path = os.environ['MCP']
exempt_path = os.environ['EXEMPT_FILE']
text = open(mcp_path, encoding='utf-8', errors='replace').read()

# Cut everything from the first `#[cfg(test)]` / `mod tests` onward.
cut = re.search(r'^\s*(mod tests|#\[cfg\(test\)\])', text, re.M)
if cut:
    text = text[:cut.start()]

# 1. Dispatch arms: "tool_name" => handle_x( — possibly across lines.
DISPATCH = re.compile(r'"([a-z_]+)"\s*=>\s*\{?\s*(?:\n\s*)?(handle_[a-z_]+)\s*\(', re.M)
dispatch = dict(DISPATCH.findall(text))  # tool -> handler

if not dispatch:
    print("check-mcp-write-tools: FAIL — no tools/call dispatch arms found in mcp.rs")
    print("  (parser drift: the dispatch regex no longer matches)")
    sys.exit(1)

# 2. needs_write arm list.
nw = re.search(r'let needs_write = matches!\(\s*tool_name,\s*((?:"[a-z_]+"\s*\|?\s*)+)\);', text)
if not nw:
    print("check-mcp-write-tools: FAIL — needs_write matches! block not found in mcp.rs")
    print("  (parser drift: the gate regex no longer matches)")
    sys.exit(1)
needs_write = set(re.findall(r'"([a-z_]+)"', nw.group(1)))

# 3. Per-handler write effects.
WRITE_CALL = re.compile(
    r'\bstate\.[a-z_]+\.(create|store|update|upsert|delete|remove|save|insert|append)\s*\('
)

def fn_body(name):
    """Return the body lines of `async fn name` / `fn name`, or None."""
    fn_re = re.compile(r'^\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+' + re.escape(name) + r'\s*[\(<]', re.M)
    m = fn_re.search(text)
    if not m:
        return None
    depth = 0
    started = False
    for i in range(m.start(), len(text)):
        if text[i] == '{':
            depth += 1
            started = True
        elif text[i] == '}':
            depth -= 1
            if started and depth <= 0:
                return text[m.start():i + 1]
    return text[m.start():]

exempt = set()
if os.path.exists(exempt_path):
    for raw in open(exempt_path, encoding='utf-8'):
        raw = raw.split('#', 1)[0].strip()
        if raw:
            exempt.add(raw)

errors = 0
checked = 0
for tool, handler in sorted(dispatch.items()):
    body = fn_body(handler)
    if body is None:
        print(f"check-mcp-write-tools: FAIL — dispatch arm for `{tool}` names handler "
              f"`{handler}` but no such fn exists in {mcp_path}")
        errors += 1
        continue
    if not WRITE_CALL.search(body):
        continue  # read-only tool
    checked += 1
    if tool in needs_write:
        continue
    if tool in exempt:
        continue
    print(f"ERROR: MCP tool `{tool}` performs write effects but is not in needs_write")
    print(f"  handler `{handler}` calls a repository write method "
          "(state.<repo>.create/store/update/...), so the tool mutates state,")
    print("  but the per-tool RBAC gate (needs_write match in mcp_handler) omits")
    print("  it — a ReadOnly-role principal can invoke this write. This is the")
    print("  specs/reviews/task-093.md F7 flaw class (gate membership not derived")
    print("  from handler effects).")
    print("  Add the tool to the needs_write list, or record ownership in")
    print(f"  {exempt_path} if the fix is owned by an open task.")
    print()
    errors += 1

if errors:
    print(f"check-mcp-write-tools: FAILED — {errors} violation(s) ({checked} write-capable tool(s) checked)")
    sys.exit(1)
print(f"check-mcp-write-tools: OK ({checked} write-capable tool(s) checked, all gated)")
PYEOF
