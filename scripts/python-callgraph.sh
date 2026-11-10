#!/usr/bin/env bash
# python-callgraph.sh - Extract caller→callee edges from a Python project
# using the pyright language server (pyright-langserver --stdio).
#
# Usage: scripts/python-callgraph.sh /path/to/python-project
# Output: JSON array to stdout:
#   [{"from": "pkg.mod.func_a", "to": "pkg.other.func_b", "line": 42}, ...]
#
# Requires: the `pyright` package (provides pyright-langserver) -
# `npm install -g pyright` or `pip install pyright`. The server binary is
# located via PYRIGHT_LANGSERVER_BIN (explicit override) or PATH. When it is
# missing, print a warning to stderr and an empty JSON array so Pass 2
# degrades gracefully instead of failing the push.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_PATH="${1:-.}"

if [[ -n "${PYRIGHT_LANGSERVER_BIN:-}" ]]; then
  if [[ -x "${PYRIGHT_LANGSERVER_BIN}" ]]; then
    exec node "${SCRIPT_DIR}/python-callgraph-pyright.mjs" "${REPO_PATH}"
  fi
  echo "warning: PYRIGHT_LANGSERVER_BIN=${PYRIGHT_LANGSERVER_BIN} is not executable; skipping Python Pass 2 call-graph extraction" >&2
  echo "[]"
  exit 0
fi

if ! command -v pyright-langserver &>/dev/null; then
  echo "warning: pyright-langserver not found on PATH (install with 'npm install -g pyright' or 'pip install pyright'); skipping Python Pass 2 call-graph extraction" >&2
  echo "[]"
  exit 0
fi

exec node "${SCRIPT_DIR}/python-callgraph-pyright.mjs" "${REPO_PATH}"
