#!/usr/bin/env node
// ts-callgraph.mjs — Extract caller→callee edges from a TypeScript project
// using the TypeScript compiler API for full type-resolution.
//
// Usage: node scripts/ts-callgraph.mjs /path/to/ts-project
// Output: JSON array to stdout:
//   [{"from": "src/foo.bar", "to": "src/baz.qux", "line": 42}, ...]

import { createRequire } from "node:module";
import * as path from "node:path";
import * as fs from "node:fs";

const require = createRequire(import.meta.url);
const ts = require("typescript");

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  ".next",
  "build",
  ".svelte-kit",
  ".git",
]);

const TS_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx"]);

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

const repoRoot = process.argv[2];
if (!repoRoot) {
  console.error("Usage: node ts-callgraph.mjs <repo-path>");
  process.exit(1);
}

const absRoot = path.resolve(repoRoot);
if (!fs.existsSync(absRoot)) {
  console.error(`Directory not found: ${absRoot}`);
  process.exit(1);
}

const edges = extractCallGraph(absRoot);
// Output JSON to stdout
console.log(JSON.stringify(edges));

// ---------------------------------------------------------------------------
// Core extraction
// ---------------------------------------------------------------------------

/**
 * @param {string} rootDir
 * @returns {Array<{from: string, to: string, line: number}>}
 */
function extractCallGraph(rootDir) {
  const sourceFiles = collectSourceFiles(rootDir);
  if (sourceFiles.length === 0) return [];

  // Try to load tsconfig.json; fall back to a default config
  const { options, fileNames } = loadCompilerOptions(rootDir, sourceFiles);

  const program = ts.createProgram(fileNames, options);
  const checker = program.getTypeChecker();

  /** @type {Array<{from: string, to: string, line: number}>} */
  const results = [];

  for (const sf of program.getSourceFiles()) {
    // Skip declaration files and files outside the project
    if (sf.isDeclarationFile) continue;
    const relPath = path.relative(rootDir, sf.fileName);
    if (relPath.startsWith("..") || path.isAbsolute(relPath)) continue;
    // Skip node_modules that might have been pulled in
    if (relPath.includes("node_modules")) continue;

    visitNode(sf, sf, checker, rootDir, results);
  }

  return dedup(results);
}

// ---------------------------------------------------------------------------
// AST walking
// ---------------------------------------------------------------------------

/**
 * Walk every node in a source file looking for CallExpressions.
 */
function visitNode(node, sourceFile, checker, rootDir, results) {
  if (ts.isCallExpression(node)) {
    const edge = resolveCall(node, sourceFile, checker, rootDir);
    if (edge) results.push(edge);
  }

  ts.forEachChild(node, (child) =>
    visitNode(child, sourceFile, checker, rootDir, results)
  );
}

/**
 * Resolve a CallExpression to a from→to edge.
 * Returns null if the callee can't be resolved to a project symbol.
 */
function resolveCall(callExpr, sourceFile, checker, rootDir) {
  const callerQName = getEnclosingQualifiedName(callExpr, sourceFile, rootDir);
  if (!callerQName) return null;

  const line =
    ts.getLineAndCharacterOfPosition(sourceFile, callExpr.getStart()).line + 1;

  // Try to resolve the called symbol
  const expr = callExpr.expression;
  let symbol;

  try {
    // For property access (obj.method), resolve the property
    if (ts.isPropertyAccessExpression(expr)) {
      symbol = checker.getSymbolAtLocation(expr.name);
    } else {
      symbol = checker.getSymbolAtLocation(expr);
    }
  } catch {
    return null;
  }

  if (!symbol) return null;

  // Follow aliases (import aliases, re-exports)
  try {
    if (symbol.flags & ts.SymbolFlags.Alias) {
      symbol = checker.getAliasedSymbol(symbol);
    }
  } catch {
    // getAliasedSymbol can throw if the target doesn't exist
    return null;
  }

  // Get the declaration to find the target file
  const declarations = symbol.getDeclarations?.();
  if (!declarations || declarations.length === 0) return null;

  const decl = declarations[0];
  const declFile = decl.getSourceFile();
  if (!declFile || declFile.isDeclarationFile) return null;

  const declRelPath = path.relative(rootDir, declFile.fileName);
  if (declRelPath.startsWith("..") || path.isAbsolute(declRelPath)) return null;
  if (declRelPath.includes("node_modules")) return null;

  const calleeQName = getSymbolQualifiedName(symbol, declFile, rootDir);
  if (!calleeQName) return null;

  // Skip self-calls
  if (callerQName === calleeQName) return null;

  return { from: callerQName, to: calleeQName, line };
}

// ---------------------------------------------------------------------------
// Qualified name computation
// ---------------------------------------------------------------------------

/**
 * Build a qualified name for the enclosing function/method/class at a call site.
 * Format: "path/to/module.ClassName.methodName" or "path/to/module.funcName"
 */
function getEnclosingQualifiedName(node, sourceFile, rootDir) {
  const moduleQName = moduleQNameFromFile(sourceFile.fileName, rootDir);
  const parts = [];

  let current = node.parent;
  while (current && current !== sourceFile) {
    const name = getDeclName(current);
    if (name) parts.unshift(name);
    current = current.parent;
  }

  if (parts.length === 0) {
    // Top-level call — attribute to the module
    return moduleQName;
  }

  return `${moduleQName}.${parts.join(".")}`;
}

/**
 * Build a qualified name for a resolved symbol.
 */
function getSymbolQualifiedName(symbol, declFile, rootDir) {
  const moduleQName = moduleQNameFromFile(declFile.fileName, rootDir);
  const name = symbol.getName();
  if (!name || name === "default") {
    return moduleQName;
  }

  // Check if the symbol is a method on a class
  const declarations = symbol.getDeclarations?.();
  if (declarations && declarations.length > 0) {
    const decl = declarations[0];
    const parent = decl.parent;
    if (parent && ts.isClassDeclaration(parent) && parent.name) {
      return `${moduleQName}.${parent.name.text}.${name}`;
    }
  }

  return `${moduleQName}.${name}`;
}

/**
 * Get the declaration name from a node, if it's a named declaration.
 */
function getDeclName(node) {
  if (ts.isFunctionDeclaration(node) && node.name) return node.name.text;
  if (ts.isMethodDeclaration(node) && ts.isIdentifier(node.name))
    return node.name.text;
  if (ts.isClassDeclaration(node) && node.name) return node.name.text;
  if (
    ts.isVariableDeclaration(node) &&
    ts.isIdentifier(node.name) &&
    node.initializer &&
    (ts.isArrowFunction(node.initializer) ||
      ts.isFunctionExpression(node.initializer))
  ) {
    return node.name.text;
  }
  // Constructor
  if (ts.isConstructorDeclaration(node)) return "constructor";
  return null;
}

// ---------------------------------------------------------------------------
// File / module utilities
// ---------------------------------------------------------------------------

/**
 * Derive module qualified name from file path (mirrors Rust extractor logic).
 * "src/components/UserCard.tsx" → "src/components/UserCard"
 */
function moduleQNameFromFile(fileName, rootDir) {
  let rel = path.relative(rootDir, fileName);
  // Remove extension
  const ext = path.extname(rel);
  if (ext) rel = rel.slice(0, -ext.length);
  // Normalize separators to forward slashes
  return rel.split(path.sep).join("/");
}

/**
 * Recursively collect all .ts/.tsx/.js/.jsx files under rootDir,
 * skipping SKIP_DIRS.
 */
function collectSourceFiles(rootDir) {
  const files = [];
  walk(rootDir, files);
  return files;
}

function walk(dir, files) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (SKIP_DIRS.has(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, files);
    } else if (entry.isFile() && TS_EXTENSIONS.has(path.extname(entry.name))) {
      files.push(full);
    }
  }
}

/**
 * Load tsconfig.json or create a default compiler options object.
 */
function loadCompilerOptions(rootDir, sourceFiles) {
  const tsconfigPath = path.join(rootDir, "tsconfig.json");

  if (fs.existsSync(tsconfigPath)) {
    const configFile = ts.readConfigFile(tsconfigPath, ts.sys.readFile);
    if (!configFile.error) {
      const parsed = ts.parseJsonConfigFileContent(
        configFile.config,
        ts.sys,
        rootDir
      );
      // Merge discovered files with tsconfig-specified files
      const allFiles = new Set([...parsed.fileNames, ...sourceFiles]);
      return {
        options: {
          ...parsed.options,
          // Ensure we don't emit anything
          noEmit: true,
        },
        fileNames: [...allFiles],
      };
    }
  }

  // Default config for projects without tsconfig.json
  const defaultOptions = {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Node10,
    jsx: ts.JsxEmit.React,
    allowJs: true,
    checkJs: false,
    noEmit: true,
    esModuleInterop: true,
    skipLibCheck: true,
    strict: false,
    baseUrl: rootDir,
  };

  return { options: defaultOptions, fileNames: sourceFiles };
}

/**
 * Deduplicate edges by from+to pair, keeping the first occurrence's line.
 */
function dedup(edges) {
  const seen = new Set();
  const result = [];
  for (const e of edges) {
    const key = `${e.from}|${e.to}`;
    if (!seen.has(key)) {
      seen.add(key);
      result.push(e);
    }
  }
  return result;
}
