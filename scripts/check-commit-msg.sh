#!/usr/bin/env bash
# Conventional commit message lint.
#
# Format: <type>(<scope>): <description>
# Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
#
# Examples:
#   feat(server): add WebSocket endpoint
#   fix(domain): correct task status transition
#   docs(agents): update AGENTS.md with new commands
#   ci: add cargo clippy step to GitHub Actions
#
# See: AGENTS.md - Commit Message Convention

set -euo pipefail

COMMIT_MSG_FILE="${1:-}"

if [ -z "$COMMIT_MSG_FILE" ]; then
    echo "Usage: $0 <commit-msg-file>"
    exit 1
fi

MSG=$(cat "$COMMIT_MSG_FILE")

# Strip comments and leading/trailing whitespace
MSG=$(echo "$MSG" | grep -v '^#' | sed '/^$/d' | head -1)

PATTERN='^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9-]+\))?: .{1,100}$'

if ! echo "$MSG" | grep -Pq "$PATTERN"; then
    echo "COMMIT MESSAGE VIOLATION: Does not match conventional commit format."
    echo ""
    echo "  Expected: <type>(<scope>): <description>"
    echo "  Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert"
    echo ""
    echo "  Your message: ${MSG}"
    echo ""
    echo "  See AGENTS.md for the full commit convention."
    exit 1
fi

echo "Commit message lint passed."
