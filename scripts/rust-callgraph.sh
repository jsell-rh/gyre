#!/usr/bin/env bash
# rust-callgraph.sh — Extract caller→callee edges from a Rust project
# using rust-analyzer's LSP textDocument/references.
#
# Usage: scripts/rust-callgraph.sh /path/to/rust-project
# Output: JSON array to stdout:
#   [{"from": "crate::module::func_a", "to": "crate::module::func_b", "line": 42}, ...]
#
# Requires: rust-analyzer binary in PATH
# This delegates to the Node.js script that drives the LSP protocol.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_PATH="${1:-.}"

if ! command -v rust-analyzer &>/dev/null; then
  echo "[]" # Return empty if rust-analyzer not available
  exit 0
fi

exec node "${SCRIPT_DIR}/rust-callgraph.mjs" "$REPO_PATH"
