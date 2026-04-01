#!/usr/bin/env python3
"""Extract call-graph edges from a Python repository.

Uses the stdlib `ast` module to:
1. Build a global symbol table  (qualified_name -> file:line)
2. Parse all import / from-import statements to build an alias map per file
3. Walk function/method bodies looking for Call nodes
4. Resolve each call through the import map + symbol table
5. Output JSON:  [{"from": "module.Class.method", "to": "other.func"}, ...]

Usage:
    python3 scripts/python-callgraph.py /path/to/repo

The script prints a JSON array to stdout.  Any diagnostics go to stderr.
"""

from __future__ import annotations

import ast
import json
import os
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple


def path_to_module(repo_root: Path, file_path: Path) -> str:
    """Convert a file path to a dotted module name, matching the Rust extractor."""
    rel = file_path.relative_to(repo_root)
    parts = list(rel.parts)
    # Strip .py extension from last part
    if parts and parts[-1].endswith(".py"):
        parts[-1] = parts[-1][:-3]
    return ".".join(parts)


SKIP_DIRS = {"__pycache__", ".git", ".venv", "venv", "node_modules", ".tox", ".mypy_cache"}


def discover_py_files(repo_root: Path) -> List[Path]:
    """Walk the repo and return all .py files, skipping common non-source dirs."""
    result = []
    for dirpath, dirnames, filenames in os.walk(repo_root):
        # Prune skipped directories in-place
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for fn in filenames:
            if fn.endswith(".py"):
                result.append(Path(dirpath) / fn)
    return result


# ---------------------------------------------------------------------------
# Symbol table: maps qualified_name -> True  (we only need existence)
# ---------------------------------------------------------------------------

class SymbolTable:
    """Global symbol table built from all .py files."""

    def __init__(self) -> None:
        # qualified_name -> True
        self.symbols: Set[str] = set()
        # module_name -> set of top-level names defined in that module
        self.module_exports: Dict[str, Set[str]] = {}

    def add(self, qname: str, module: str, short_name: str) -> None:
        self.symbols.add(qname)
        self.module_exports.setdefault(module, set()).add(short_name)

    def exists(self, qname: str) -> bool:
        return qname in self.symbols

    def resolve_in_module(self, module: str, name: str) -> Optional[str]:
        """Check if `name` is defined in `module` and return the qname."""
        qname = f"{module}.{name}"
        if qname in self.symbols:
            return qname
        return None


def build_symbol_table(repo_root: Path, files: List[Path]) -> SymbolTable:
    """Parse all files and register every class/function/method."""
    table = SymbolTable()

    for fpath in files:
        try:
            source = fpath.read_text(encoding="utf-8", errors="replace")
            tree = ast.parse(source, filename=str(fpath))
        except (SyntaxError, UnicodeDecodeError):
            continue

        module = path_to_module(repo_root, fpath)

        for node in ast.iter_child_nodes(tree):
            if isinstance(node, ast.ClassDef):
                class_qname = f"{module}.{node.name}"
                table.add(class_qname, module, node.name)
                # Register methods
                for item in ast.iter_child_nodes(node):
                    if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                        method_qname = f"{class_qname}.{item.name}"
                        table.add(method_qname, module, f"{node.name}.{item.name}")
            elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                func_qname = f"{module}.{node.name}"
                table.add(func_qname, module, node.name)

    return table


# ---------------------------------------------------------------------------
# Import map: per-file alias -> qualified module/symbol
# ---------------------------------------------------------------------------

class ImportMap:
    """Per-file mapping from local aliases to qualified names."""

    def __init__(self) -> None:
        # alias -> module_qname  (for `import X` or `import X as Y`)
        self.module_imports: Dict[str, str] = {}
        # alias -> (module_qname, original_name)  (for `from X import Y`)
        self.from_imports: Dict[str, Tuple[str, str]] = {}

    def resolve(self, name: str, symbol_table: SymbolTable, current_module: str) -> Optional[str]:
        """Resolve a dotted name through the import map.

        `name` can be:
        - "foo"          -> simple name
        - "foo.bar"      -> attribute access
        - "foo.bar.baz"  -> chained attribute
        """
        parts = name.split(".")

        # 1. Check from-imports first (most specific)
        if parts[0] in self.from_imports:
            mod_qname, orig_name = self.from_imports[parts[0]]
            # `from mod import Cls` then `Cls.method()` -> mod.Cls.method
            candidate = f"{mod_qname}.{orig_name}"
            if len(parts) > 1:
                candidate += "." + ".".join(parts[1:])
            if symbol_table.exists(candidate):
                return candidate
            # Also try: the imported name IS the module
            # `from pkg import sub` then `sub.func()` -> pkg.sub.func
            candidate2 = f"{mod_qname}.{orig_name}"
            if len(parts) > 1:
                candidate2 = f"{candidate2}.{'.'.join(parts[1:])}"
                # Already tried above; now try treating orig_name as module
                candidate3 = f"{mod_qname}.{'.'.join(parts)}"
                if symbol_table.exists(candidate3):
                    return candidate3

        # 2. Check module imports (`import X` / `import X as Y`)
        if parts[0] in self.module_imports:
            mod_qname = self.module_imports[parts[0]]
            if len(parts) > 1:
                candidate = f"{mod_qname}.{'.'.join(parts[1:])}"
            else:
                candidate = mod_qname
            if symbol_table.exists(candidate):
                return candidate

        # 3. Check if it's a local (same-module) reference
        candidate = f"{current_module}.{name}"
        if symbol_table.exists(candidate):
            return candidate

        # 4. Check if it's already a fully qualified name
        if symbol_table.exists(name):
            return name

        return None


def build_import_map(tree: ast.Module, repo_root: Path, file_path: Path) -> ImportMap:
    """Extract import aliases from a parsed AST."""
    imap = ImportMap()
    current_module = path_to_module(repo_root, file_path)
    current_pkg = ".".join(current_module.split(".")[:-1])

    for node in ast.iter_child_nodes(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                local_name = alias.asname if alias.asname else alias.name
                imap.module_imports[local_name] = alias.name
        elif isinstance(node, ast.ImportFrom):
            if node.module is None:
                continue
            # Resolve relative imports
            if node.level and node.level > 0:
                # Relative import: go up `level` packages
                pkg_parts = current_pkg.split(".")
                up = node.level - 1
                if up < len(pkg_parts):
                    base = ".".join(pkg_parts[:len(pkg_parts) - up]) if pkg_parts[0] else ""
                else:
                    base = ""
                if base and node.module:
                    mod_qname = f"{base}.{node.module}"
                elif base:
                    mod_qname = base
                else:
                    mod_qname = node.module or ""
            else:
                mod_qname = node.module

            for alias in node.names:
                if alias.name == "*":
                    continue
                local_name = alias.asname if alias.asname else alias.name
                imap.from_imports[local_name] = (mod_qname, alias.name)

    return imap


# ---------------------------------------------------------------------------
# Call extraction from function bodies
# ---------------------------------------------------------------------------

def call_name_from_node(node: ast.expr) -> Optional[str]:
    """Extract the dotted name from a Call's func node.

    Handles:
      - Name("foo")                -> "foo"
      - Attribute(Name("a"), "b")  -> "a.b"
      - Attribute(Attribute(Name("a"), "b"), "c") -> "a.b.c"
    """
    if isinstance(node, ast.Name):
        return node.id
    elif isinstance(node, ast.Attribute):
        parent = call_name_from_node(node.value)
        if parent:
            return f"{parent}.{node.attr}"
    return None


def extract_calls_from_body(
    body: List[ast.stmt],
) -> List[str]:
    """Walk AST body and collect all call target names."""
    calls: List[str] = []

    class CallVisitor(ast.NodeVisitor):
        def visit_Call(self, node: ast.Call) -> None:
            name = call_name_from_node(node.func)
            if name:
                calls.append(name)
            self.generic_visit(node)

    visitor = CallVisitor()
    for stmt in body:
        visitor.visit(stmt)
    return calls


# ---------------------------------------------------------------------------
# Main extraction
# ---------------------------------------------------------------------------

def extract_callgraph(repo_root: Path) -> List[Dict[str, str]]:
    """Extract call-graph edges from a Python repository."""
    files = discover_py_files(repo_root)
    if not files:
        print("No Python files found", file=sys.stderr)
        return []

    print(f"Found {len(files)} Python files", file=sys.stderr)

    # Pass 1: build symbol table
    symbol_table = build_symbol_table(repo_root, files)
    print(f"Symbol table: {len(symbol_table.symbols)} symbols", file=sys.stderr)

    # Pass 2: extract calls
    edges: List[Dict[str, str]] = []
    seen: Set[Tuple[str, str]] = set()

    for fpath in files:
        try:
            source = fpath.read_text(encoding="utf-8", errors="replace")
            tree = ast.parse(source, filename=str(fpath))
        except (SyntaxError, UnicodeDecodeError):
            continue

        module = path_to_module(repo_root, fpath)
        imap = build_import_map(tree, repo_root, fpath)

        # Walk top-level definitions
        for node in ast.iter_child_nodes(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                caller_qname = f"{module}.{node.name}"
                if caller_qname not in symbol_table.symbols:
                    continue
                raw_calls = extract_calls_from_body(node.body)
                for raw_name in raw_calls:
                    resolved = imap.resolve(raw_name, symbol_table, module)
                    if resolved and resolved != caller_qname:
                        key = (caller_qname, resolved)
                        if key not in seen:
                            seen.add(key)
                            edges.append({"from": caller_qname, "to": resolved})

            elif isinstance(node, ast.ClassDef):
                class_qname = f"{module}.{node.name}"
                for item in ast.iter_child_nodes(node):
                    if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                        caller_qname = f"{class_qname}.{item.name}"
                        if caller_qname not in symbol_table.symbols:
                            continue
                        raw_calls = extract_calls_from_body(item.body)
                        for raw_name in raw_calls:
                            resolved = imap.resolve(raw_name, symbol_table, module)
                            if resolved and resolved != caller_qname:
                                key = (caller_qname, resolved)
                                if key not in seen:
                                    seen.add(key)
                                    edges.append({"from": caller_qname, "to": resolved})

    print(f"Resolved {len(edges)} call edges", file=sys.stderr)
    return edges


def main() -> None:
    if len(sys.argv) < 2:
        print("Usage: python3 python-callgraph.py <repo-path>", file=sys.stderr)
        sys.exit(1)

    repo_root = Path(sys.argv[1]).resolve()
    if not repo_root.is_dir():
        print(f"Not a directory: {repo_root}", file=sys.stderr)
        sys.exit(1)

    edges = extract_callgraph(repo_root)
    json.dump(edges, sys.stdout, indent=None)
    print()  # trailing newline


if __name__ == "__main__":
    main()
