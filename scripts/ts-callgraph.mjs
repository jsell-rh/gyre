#!/usr/bin/env node
// ts-callgraph.mjs — Extract caller→callee edges from a TypeScript project
// using `typescript-language-server --stdio` (LSP subprocess) for full
// type-resolved call graph. See specs/system/lsp-call-graph.md §5 TypeScript
// and specs/tasks/task-075.md.
//
// Usage: node scripts/ts-callgraph.mjs /path/to/ts-project
// Output: JSON array to stdout:
//   [{"from": "src/app.compute", "to": "src/math.add", "line": 4}, ...]
//
// Approach (the spec's inverted "find references" model):
// 1. Discover .ts/.tsx/.js/.jsx source files (.svelte files are skipped with
//    a warning — the Svelte compiler is not TypeScript, tsserver cannot
//    analyze them).
// 2. Start `typescript-language-server --stdio` as an LSP subprocess.
// 3. didOpen every source file, then request textDocument/documentSymbol for
//    each file (hierarchical symbols carry the declaration nesting).
// 4. For every top-level function/class/interface definition, send
//    textDocument/references. tsserver's symbol resolution handles the
//    TypeScript-specific patterns: structural typing, overloads, JSX
//    component usage, and re-exports through barrel files (references are
//    reported at the final usage site with the symbol identity intact).
// 5. For each reference site, resolve the enclosing function/method (by
//    symbol range containment) → emit a Calls edge.
// 6. Shut the server down.
//
// Qualified-name format (must match Pass 1, typescript_extractor.rs):
//   top-level fn/class/interface: "<relpath-without-ext>.<Name>"
//   nested (method in class etc.): "<relpath-without-ext>.<Outer>.<inner>"
// Edges whose names do not match a Pass 1 node are dropped later by
// gyre_domain::call_graph_resolve (Pass 1 only extracts exported top-level
// declarations), so raw output may legitimately contain more edges than the
// graph keeps.
//
// Graceful degradation: if the language server binary, a TypeScript
// installation, or node itself is unavailable, print a warning to stderr and
// `[]` to stdout with exit code 0 — Pass 2 is best-effort and never fails a
// push.
//
// Environment overrides:
//   TS_CALLGRAPH_SERVER     path to the typescript-language-server binary
//                           (authoritative: if set but missing, degrade)
//   TS_CALLGRAPH_TSSERVER   path to a TypeScript lib dir (or tsserver.js)
//                           passed to the server as tsserver.path

import { spawn } from "node:child_process";
import * as path from "node:path";
import * as fs from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  "build",
  ".next",
  ".svelte-kit",
  ".git",
  "coverage",
]);

/** Map of analyzable extensions → LSP languageId. */
const SOURCE_EXTENSIONS = new Map([
  [".ts", "typescript"],
  [".tsx", "typescriptreact"],
  [".js", "javascript"],
  [".jsx", "javascriptreact"],
]);

const SVELTE_EXTENSION = ".svelte";

/** Max time for the LSP initialize handshake (server loads tsserver). */
const INIT_TIMEOUT_MS = 30_000;
/** Max time per documentSymbol request. The first one on a large project
 * includes tsserver's project load, so it gets more headroom than reference
 * queries. */
const DOCUMENT_SYMBOL_TIMEOUT_MS = 30_000;
/** documentSymbol timeout after the first (project already loaded). */
const SUBSEQUENT_SYMBOL_TIMEOUT_MS = 10_000;
/** Max time per references request. */
const REQUEST_TIMEOUT_MS = 10_000;
/** Self-imposed budget for the query loop. The Rust adapter kills the
 * subprocess at 60s; stop querying a bit before that so partial results
 * still get printed. */
const QUERY_BUDGET_MS = 50_000;

/** LSP SymbolKind values we care about (subset of the LSP spec). */
export const SymbolKind = {
  Class: 5,
  Method: 6,
  Constructor: 9,
  Interface: 11,
  Function: 12,
  Variable: 14,
};

// ---------------------------------------------------------------------------
// Pure helpers (exported for scripts/test/ts-callgraph.test.mjs)
// ---------------------------------------------------------------------------

/**
 * Serialize a JSON-RPC message as an LSP wire packet
 * (`Content-Length: N\r\n\r\n<payload>`). Byte length, not char length.
 */
export function encodeMessage(msg) {
  const payload = JSON.stringify(msg);
  return `Content-Length: ${Buffer.byteLength(payload)}\r\n\r\n${payload}`;
}

/**
 * Parse complete JSON-RPC messages out of a stdio byte stream. `buffer`
 * is a Buffer (or string, coerced to its UTF-8 bytes) of bytes received so
 * far; returns `{ messages, rest }` — `rest` is a Buffer holding the
 * unconsumed tail (a partial message waiting for more data). Malformed
 * headers are skipped.
 *
 * Framing is strictly byte-based: the wire `Content-Length` counts UTF-8
 * bytes, so the buffer must be accumulated and sliced in bytes, never as a
 * JS string (UTF-16 code units). typescript-language-server emits raw UTF-8
 * in response bodies — JSON.stringify does not escape non-ASCII — so any
 * body containing multi-byte characters (non-ASCII symbol names, log
 * messages, paths) is longer in bytes than in code units, and a code-unit
 * length gate can never be satisfied: the request times out, the file is
 * skipped, and the over-long slice desynchronizes the parser for everything
 * that follows. Each body is decoded to a string only after byte-exact
 * extraction.
 */
export function extractMessages(buffer) {
  const messages = [];
  let rest = Buffer.isBuffer(buffer)
    ? buffer
    : Buffer.from(buffer, "utf8");
  while (true) {
    const headerEnd = rest.indexOf("\r\n\r\n");
    if (headerEnd === -1) break;
    // Headers are ASCII; latin1 decoding preserves the bytes for the regex.
    const match = rest
      .subarray(0, headerEnd)
      .toString("latin1")
      .match(/Content-Length:\s*(\d+)/i);
    if (!match) {
      rest = rest.subarray(headerEnd + 4);
      continue;
    }
    const contentLen = parseInt(match[1], 10);
    const bodyStart = headerEnd + 4;
    if (rest.length < bodyStart + contentLen) break;
    const body = rest
      .subarray(bodyStart, bodyStart + contentLen)
      .toString("utf8");
    rest = rest.subarray(bodyStart + contentLen);
    try {
      messages.push(JSON.parse(body));
    } catch {
      // Malformed body — skip it rather than kill the whole extraction.
    }
  }
  return { messages, rest };
}

/**
 * Module qualified name for a file, matching Pass 1 (typescript_extractor.rs
 * `module_qname_from_path`): repo-relative path, extension stripped, `/`
 * separators. "src/components/UserCard.tsx" → "src/components/UserCard".
 */
export function moduleQualifiedName(repoRoot, fileName) {
  let rel = path.relative(repoRoot, fileName);
  const ext = path.extname(rel);
  if (ext) rel = rel.slice(0, -ext.length);
  return rel.split(path.sep).join("/");
}

/** Convert a `file://` URI to a filesystem path (decodes %XX escapes). */
export function uriToPath(uri) {
  try {
    return fileURLToPath(uri);
  } catch {
    return null;
  }
}

/** True when LSP position `p` lies inside `range` (start-inclusive, end-exclusive). */
function rangeContains(range, p) {
  const { start, end } = range;
  if (p.line < start.line || p.line > end.line) return false;
  if (p.line === start.line && p.character < start.character) return false;
  if (p.line === end.line && p.character >= end.character) return false;
  return true;
}

/** True when a Variable symbol's source text looks like a function
 * initializer (arrow or function expression). Used both to decide which
 * variables get reference queries and which symbols can enclose a call. */
export function variableLooksLikeFunction(symbolText) {
  return /=>|\bfunction\b/.test(symbolText);
}

function sliceByText(fileText, range) {
  // Line/character → byte-ish offset over the JS string (LSP positions are
  // UTF-16 code units, which match JS string indexing).
  const lines = fileText.split("\n");
  const start = offsetOf(lines, range.start);
  const end = offsetOf(lines, range.end);
  if (start === null || end === null) return "";
  return fileText.slice(start, end);
}

function offsetOf(lines, pos) {
  if (pos.line >= lines.length) return null;
  let off = 0;
  for (let i = 0; i < pos.line; i++) off += lines[i].length + 1;
  return off + Math.min(pos.character, lines[pos.line].length);
}

/** Kinds that can be the *enclosing* callable of a reference site. */
function isScopeKind(kind) {
  return (
    kind === SymbolKind.Function ||
    kind === SymbolKind.Method ||
    kind === SymbolKind.Constructor
  );
}

/**
 * Walk a hierarchical DocumentSymbol tree and return the name chain of the
 * innermost function-like symbol containing LSP position {line, character}:
 * e.g. ["UserService", "handleRequest"] for a reference inside a method.
 * Returns null when the position is not inside any function (top-level
 * import/export references are not calls — they are skipped).
 *
 * The chain includes intermediate container names (classes) so method call
 * sites get `module.Class.method` qualified names.
 */
export function findEnclosingChain(symbols, fileText, line, character) {
  for (const sym of symbols) {
    if (!rangeContains(sym.range, { line, character })) continue;
    const deeper = sym.children
      ? findEnclosingChain(sym.children, fileText, line, character)
      : null;
    if (deeper) return [sym.name, ...deeper];
    if (isScopeKind(sym.kind)) return [sym.name];
    if (
      sym.kind === SymbolKind.Variable &&
      variableLooksLikeFunction(sliceByText(fileText, sym.range))
    ) {
      return [sym.name];
    }
    // Contained in a non-function symbol (e.g. directly in a class body)
    // and nothing deeper matches — no enclosing function.
    return null;
  }
  return null;
}

/**
 * Select the definitions to send reference queries for: top-level
 * functions, classes, and interfaces (per spec §5: "for each
 * function/class/interface definition"), plus top-level variables whose
 * initializer is a function (arrow consts — Pass 1 makes Function nodes for
 * exported ones). Nested methods/functions are never query targets: Pass 1
 * extracts no nodes for them, so edges to them could never resolve.
 *
 * Returns [{name, position}] where position is the definition's name
 * position (selectionRange.start) — where a references request must point.
 */
export function collectQueryTargets(symbols, fileText) {
  const targets = [];
  for (const sym of symbols) {
    if (
      sym.kind === SymbolKind.Function ||
      sym.kind === SymbolKind.Class ||
      sym.kind === SymbolKind.Interface
    ) {
      targets.push({ name: sym.name, position: sym.selectionRange.start });
    } else if (
      sym.kind === SymbolKind.Variable &&
      variableLooksLikeFunction(sliceByText(fileText, sym.range))
    ) {
      targets.push({ name: sym.name, position: sym.selectionRange.start });
    }
  }
  return targets;
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

/**
 * Recursively collect analyzable source files under `rootDir`, skipping
 * build/vendor directories. `.svelte` files are counted (not analyzed) so
 * the caller can warn: the Svelte compiler is not TypeScript.
 */
export function collectSourceFiles(rootDir) {
  const files = [];
  let svelteCount = 0;
  function walk(dir) {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (SKIP_DIRS.has(entry.name)) continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile()) {
        if (SOURCE_EXTENSIONS.has(path.extname(entry.name))) files.push(full);
        else if (entry.name.endsWith(SVELTE_EXTENSION)) svelteCount++;
      }
    }
  }
  walk(rootDir);
  return { files, svelteCount };
}

// ---------------------------------------------------------------------------
// Toolchain discovery
// ---------------------------------------------------------------------------

function isFile(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

/** Absolute `dir` and its ancestors (bounded), closest first. */
function walkUp(dir) {
  const roots = [];
  let current = path.resolve(dir);
  for (let i = 0; i < 12 && current; i++) {
    roots.push(current);
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return roots;
}

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

/**
 * Locate the `typescript-language-server` binary.
 *
 * Search order:
 * 1. `TS_CALLGRAPH_SERVER` env var (authoritative override — if set but the
 *    file is missing, degrade rather than fall through)
 * 2. `node_modules/.bin/typescript-language-server` under the analyzed repo
 *    root or its ancestors (project-local install)
 * 3. `node_modules/.bin/typescript-language-server` under the Gyre
 *    workspace root (this script's location, walked up)
 * 4. `typescript-language-server` on `PATH`
 */
export function findLanguageServer(repoRoot) {
  const env = process.env.TS_CALLGRAPH_SERVER;
  if (env) {
    if (isFile(env)) return env;
    return null;
  }
  for (const root of [...walkUp(repoRoot), ...walkUp(SCRIPT_DIR)]) {
    const candidate = path.join(root, "node_modules", ".bin", "typescript-language-server");
    if (isFile(candidate)) return candidate;
  }
  return whichOnPath("typescript-language-server");
}

/** Resolve an executable name on PATH (no deps). */
export function whichOnPath(name) {
  const pathVar = process.env.PATH;
  if (!pathVar) return null;
  for (const dir of pathVar.split(path.delimiter)) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    if (isFile(candidate)) return candidate;
  }
  return null;
}

/** Normalize a user tsserver path (accept lib dir or tsserver.js). */
function normalizeTsserverPath(p) {
  if (p.endsWith(`tsserver${path.extname(p) || ".js"}`) || p.endsWith("tsserver.js")) {
    return path.dirname(p);
  }
  return p;
}

function tsserverLibValid(libDir) {
  // TypeScript 7 (native) lib dirs have no tsserver.js — typescript-language-server
  // cannot use them.
  return isFile(path.join(libDir, "tsserver.js"));
}

/**
 * Locate a TypeScript `lib` directory containing `tsserver.js`, to pass to
 * the language server as `initializationOptions.tsserver.path`.
 *
 * Search order:
 * 1. `TS_CALLGRAPH_TSSERVER` env var (authoritative override)
 * 2. `node_modules/typescript/lib` under the analyzed repo root or its
 *    ancestors (the project's own TypeScript — what tsserver should use)
 * 3. same under the Gyre workspace root
 * 4. sibling of the discovered language-server install (`npm install
 *    typescript-language-server typescript` puts both under one prefix)
 *
 * Returns null when nothing is found — the server then tries its own
 * workspace resolution, and if that fails too, initialize fails and the
 * driver degrades to `[]`.
 */
export function findTsserverLib(repoRoot, serverPath) {
  const env = process.env.TS_CALLGRAPH_TSSERVER;
  if (env) {
    const lib = normalizeTsserverPath(env);
    return tsserverLibValid(lib) ? lib : null;
  }
  for (const root of [...walkUp(repoRoot), ...walkUp(SCRIPT_DIR)]) {
    const lib = path.join(root, "node_modules", "typescript", "lib");
    if (tsserverLibValid(lib)) return lib;
  }
  if (serverPath) {
    try {
      const real = fs.realpathSync(serverPath);
      // <prefix>/node_modules/.bin/<bin> → <prefix>/node_modules/typescript/lib
      const marker = `${path.sep}node_modules${path.sep}.bin`;
      const idx = real.lastIndexOf(marker);
      if (idx !== -1) {
        const lib = path.join(
          real.slice(0, idx),
          "node_modules",
          "typescript",
          "lib"
        );
        if (tsserverLibValid(lib)) return lib;
      }
    } catch {
      // realpath failure — ignore this discovery branch
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// LSP client
// ---------------------------------------------------------------------------

/**
 * Minimal LSP client over the subprocess's stdio. Uses Content-Length framing
 * (see encodeMessage/extractMessages) and resolves pending requests by id.
 * Server-initiated requests get a `null` response; notifications are ignored.
 */
export class LspClient {
  constructor(proc) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    // Accumulate raw bytes: Content-Length counts UTF-8 bytes, so the
    // reassembly gate must run on byte counts, never on decoded-string
    // code units (see extractMessages).
    this.buffer = Buffer.alloc(0);

    proc.stdout.on("data", (chunk) => {
      this.buffer = Buffer.concat([
        this.buffer,
        typeof chunk === "string" ? Buffer.from(chunk, "utf8") : chunk,
      ]);
      const { messages, rest } = extractMessages(this.buffer);
      this.buffer = rest;
      for (const msg of messages) this._dispatch(msg);
    });

    proc.stderr.on("data", (chunk) => {
      process.stderr.write(chunk);
    });

    proc.on("exit", () => {
      // Fail all pending requests so waiters do not hang until their timeout.
      for (const { reject } of this.pending.values()) {
        reject(new Error("language server exited"));
      }
      this.pending.clear();
    });
  }

  _dispatch(msg) {
    if (msg.id !== undefined && msg.method !== undefined) {
      // Server-initiated request (workspace/configuration etc.) — answer null.
      this._write({ jsonrpc: "2.0", id: msg.id, result: null });
      return;
    }
    if (msg.id !== undefined && this.pending.has(msg.id)) {
      const { resolve, reject } = this.pending.get(msg.id);
      this.pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message || "LSP error"));
      else resolve(msg.result);
    }
    // Notifications (window/logMessage, $/typescriptVersion, ...) — ignored.
  }

  _write(msg) {
    try {
      this.proc.stdin.write(encodeMessage(msg));
    } catch {
      // stdin closed (server died) — pending requests are rejected by the
      // exit handler.
    }
  }

  /** Send a request; resolves with the `result` (or rejects). */
  request(method, params, timeoutMs = REQUEST_TIMEOUT_MS) {
    const id = this.nextId++;
    this._write({ jsonrpc: "2.0", id, method, params });
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`LSP request timed out: ${method}`));
        }
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (result) => {
          clearTimeout(timer);
          resolve(result);
        },
        reject: (err) => {
          clearTimeout(timer);
          reject(err);
        },
      });
    });
  }

  /** Send a notification (no response expected). */
  notify(method, params) {
    this._write({ jsonrpc: "2.0", method, params });
  }

  /**
   * LSP initialize handshake. `tsserverPath` (optional) is forwarded as
   * `initializationOptions.tsserver.path` so the server can use a TypeScript
   * installation outside the analyzed workspace.
   */
  async initialize(rootUri, tsserverPath) {
    const initializationOptions = tsserverPath
      ? { tsserver: { path: tsserverPath } }
      : {};
    const result = await this.request(
      "initialize",
      {
        processId: process.pid,
        rootUri,
        capabilities: {
          textDocument: {
            references: { dynamicRegistration: false },
            documentSymbol: {
              dynamicRegistration: false,
              hierarchicalDocumentSymbolSupport: true,
            },
          },
        },
        initializationOptions,
      },
      INIT_TIMEOUT_MS
    );
    this.notify("initialized", {});
    return result;
  }

  /** LSP shutdown sequence: shutdown request, exit notification, kill. */
  async shutdown() {
    try {
      await this.request("shutdown", null, 2_000);
    } catch {
      // Server already gone or slow — the kill below is the backstop.
    }
    try {
      this.notify("exit", null);
    } catch {
      // ignore
    }
    const proc = this.proc;
    setTimeout(() => {
      try {
        proc.kill();
      } catch {
        // ignore
      }
    }, 1_000).unref();
  }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

function warn(message) {
  process.stderr.write(`ts-callgraph: ${message}\n`);
}

/** True when the source text imports a `.svelte` module. tsserver cannot
 * resolve Svelte imports (the Svelte compiler is not TypeScript) and,
 * empirically, a documentSymbol request for such a file never completes —
 * so these files are skipped up front rather than allowed to wedge the
 * extraction (see specs/tasks/task-075.md: Svelte handling). */
export function importsSvelteModule(text) {
  return /from\s+["'][^"']+\.svelte["']/.test(text) || /import\(\s*["'][^"']+\.svelte["']\s*\)/.test(text);
}

/**
 * Drive the full extraction for `absRoot` against an already-spawned,
 * initialized LSP client. `files` are absolute source file paths. Exposed
 * for the recorded-protocol test fixture recorder; `main` is the normal
 * entry point.
 */
export async function extractEdges(client, absRoot, files) {
  const deadline = Date.now() + QUERY_BUDGET_MS;
  // Open each file and collect its hierarchical symbols. Opening and
  // querying one file at a time keeps the server's queue shallow — a batch
  // of didOpens followed by a burst of documentSymbol requests makes the
  // first request absorb the whole project load and time out.
  // typescript-language-server only answers documentSymbol/references for
  // documents it has been told about.
  const fileTexts = new Map();
  const fileSymbols = new Map();
  let svelteImportsSkipped = 0;
  let firstSymbolRequest = true;
  // TypeScript files first: they are this task's subject and, on
  // JS-dominated repos without a tsconfig (inferred tsserver projects), far
  // more likely to resolve quickly than .js files whose dependency graphs
  // (vitest, testing libraries) can wedge tsserver for minutes.
  const priority = (f) => {
    const ext = path.extname(f);
    if (ext === ".ts") return 0;
    if (ext === ".tsx") return 1;
    return 2;
  };
  const orderedFiles = [...files].sort(
    (a, b) => priority(a) - priority(b) || a.localeCompare(b)
  );
  // Directories where a documentSymbol request timed out. Sibling files
  // usually share import patterns, so when one file in a directory wedges
  // tsserver (seen with vitest-importing test files in inferred projects),
  // its siblings almost certainly will too — skip the rest of that
  // directory instead of burning the time budget on 10s timeouts one file
  // at a time. Logged below so the skip is visible, never silent.
  const wedgedDirs = new Set();
  let wedgedDirFilesSkipped = 0;
  for (const file of orderedFiles) {
    if (Date.now() > deadline) {
      warn(`query budget of ${QUERY_BUDGET_MS / 1000}s exhausted before all files were analyzed; emitting partial results`);
      break;
    }
    const dir = path.dirname(file);
    if (wedgedDirs.has(dir)) {
      wedgedDirFilesSkipped++;
      continue;
    }
    let text;
    try {
      text = fs.readFileSync(file, "utf-8");
    } catch (e) {
      warn(`skipping unreadable file ${file}: ${e.message}`);
      continue;
    }
    if (text.includes("\0")) continue; // binary
    if (importsSvelteModule(text)) {
      svelteImportsSkipped++;
      continue;
    }
    fileTexts.set(file, text);
    const languageId =
      SOURCE_EXTENSIONS.get(path.extname(file)) || "typescript";
    client.notify("textDocument/didOpen", {
      textDocument: {
        uri: pathToFileURL(file).href,
        languageId,
        version: 1,
        text,
      },
    });
    // The first documentSymbol request absorbs tsserver's project load, so
    // it gets the full timeout; later requests on a loaded project are fast
    // and a timeout there means a pathological file — skip it, keep going.
    const timeout = firstSymbolRequest
      ? DOCUMENT_SYMBOL_TIMEOUT_MS
      : SUBSEQUENT_SYMBOL_TIMEOUT_MS;
    firstSymbolRequest = false;
    let symbols;
    try {
      symbols = await client.request(
        "textDocument/documentSymbol",
        { textDocument: { uri: pathToFileURL(file).href } },
        timeout
      );
    } catch (e) {
      // One unresponsive file must not sink the whole extraction — skip the
      // rest of its directory (see `wedgedDirs`) and keep going (Pass 2 is
      // best-effort).
      warn(`documentSymbol failed for ${file}: ${e.message}; skipping the rest of ${dir}`);
      wedgedDirs.add(dir);
      continue;
    }
    if (Array.isArray(symbols)) fileSymbols.set(file, symbols);
  }
  if (svelteImportsSkipped > 0) {
    warn(
      `skipping ${svelteImportsSkipped} file(s) that import .svelte modules — ` +
        `tsserver cannot resolve them and would hang on their analysis`
    );
  }
  if (wedgedDirFilesSkipped > 0) {
    warn(
      `skipped ${wedgedDirFilesSkipped} further file(s) in directories where ` +
        `tsserver stopped responding (see documentSymbol failures above)`
    );
  }

  // Reference queries: one per top-level function/class/interface definition.
  // The deadline is the one started before symbol collection: the whole
  // extraction must fit inside one QUERY_BUDGET_MS window (the Rust adapter
  // kills the subprocess at 60s).
  const edges = [];
  let queried = 0;
  for (const [file, symbols] of fileSymbols) {
    const moduleQName = moduleQualifiedName(absRoot, file);
    const text = fileTexts.get(file);
    for (const target of collectQueryTargets(symbols, text)) {
      if (Date.now() > deadline) {
        warn(`query budget of ${QUERY_BUDGET_MS / 1000}s exhausted; emitting partial results`);
        return dedupEdges(edges);
      }
      queried++;
      let locations;
      try {
        locations = await client.request("textDocument/references", {
          textDocument: { uri: pathToFileURL(file).href },
          position: target.position,
          context: { includeDeclaration: false },
        });
      } catch (e) {
        warn(`references query failed for ${moduleQName}.${target.name}: ${e.message}`);
        continue;
      }
      if (!Array.isArray(locations)) continue;

      const toQName = `${moduleQName}.${target.name}`;
      for (const loc of locations) {
        const refPath = loc.uri ? uriToPath(loc.uri) : null;
        if (!refPath || !fileTexts.has(refPath)) continue;
        const refSymbols = fileSymbols.get(refPath);
        if (!refSymbols) continue;
        const start = loc.range && loc.range.start;
        if (!start) continue;
        const chain = findEnclosingChain(
          refSymbols,
          fileTexts.get(refPath),
          start.line,
          start.character
        );
        if (!chain) continue; // top-level import/re-export — not a call
        const fromQName = `${moduleQualifiedName(absRoot, refPath)}.${chain.join(".")}`;
        if (fromQName === toQName) continue;
        edges.push({ from: fromQName, to: toQName, line: start.line + 1 });
      }
    }
  }
  warn(`queried ${queried} definitions, found ${edges.length} reference sites`);
  return dedupEdges(edges);
}

/** Deduplicate edges by from+to pair, keeping the first occurrence's line. */
export function dedupEdges(edges) {
  const seen = new Set();
  const result = [];
  for (const e of edges) {
    const key = `${e.from}|${e.to}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(e);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
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

  const { files, svelteCount } = collectSourceFiles(absRoot);
  if (svelteCount > 0) {
    warn(
      `skipping ${svelteCount} .svelte file(s) — the Svelte compiler is not ` +
        `TypeScript; tsserver cannot analyze them`
    );
  }
  if (files.length === 0) {
    console.log("[]");
    return;
  }

  const server = findLanguageServer(absRoot);
  if (!server) {
    warn(
      "typescript-language-server not found (searched TS_CALLGRAPH_SERVER, " +
        "node_modules/.bin under the repo and workspace, and PATH); " +
        "skipping TypeScript Pass 2"
    );
    console.log("[]");
    return;
  }

  const tsserverLib = findTsserverLib(absRoot, server);
  if (!tsserverLib) {
    warn(
      "no TypeScript installation found for tsserver (searched " +
        "TS_CALLGRAPH_TSSERVER and node_modules/typescript/lib under the repo, " +
        "workspace, and language-server prefix); relying on the server's own " +
        "workspace resolution"
    );
  }

  let proc;
  try {
    proc = spawn(server, ["--stdio"], {
      stdio: ["pipe", "pipe", "pipe"],
      cwd: absRoot,
    });
  } catch (e) {
    warn(`failed to spawn typescript-language-server: ${e.message}`);
    console.log("[]");
    return;
  }

  const client = new LspClient(proc);
  try {
    await client.initialize(pathToFileURL(absRoot).href, tsserverLib);
  } catch (e) {
    warn(`LSP initialize failed: ${e.message}; skipping TypeScript Pass 2`);
    try {
      proc.kill();
    } catch {
      // ignore
    }
    console.log("[]");
    return;
  }

  try {
    const edges = await extractEdges(client, absRoot, files);
    console.log(JSON.stringify(edges));
  } finally {
    await client.shutdown();
  }
}

const isDirectRun =
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;
if (isDirectRun) {
  main().catch((err) => {
    warn(`error: ${err && err.message ? err.message : err}`);
    console.log("[]");
    process.exit(0); // Never fail the extraction pipeline.
  });
}
