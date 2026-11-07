#!/usr/bin/env node
// python-callgraph-pyright.mjs - Extract caller→callee edges from a Python
// project using the pyright language server (pyright-langserver --stdio).
//
// Usage: node scripts/python-callgraph-pyright.mjs /path/to/python-project
// Output: JSON array to stdout:
//   [{"from": "app.main.run", "to": "app.util.helper", "line": 12}, ...]
//
// Approach (specs/system/lsp-call-graph.md §5 Python):
// 1. Start pyright-langserver --stdio as an LSP subprocess.
// 2. Scan every .py file for class/function/method definitions and the line
//    ranges they cover (indentation-aware; decorator lines are attributed to
//    the definition they decorate).
// 3. didOpen every file so pyright's analysis program includes it.
// 4. For each Pass-1-visible definition, send textDocument/references at the
//    definition-name position.
// 5. For each reference site: skip import-binding sites (an import binds a
//    name, it is not a call), resolve the enclosing definition - a reference
//    at module level attributes to the module itself - and emit a Calls edge.
// 6. Shut the server down, deduplicate, print the edge list as JSON.
//
// Qualified names match the Pass 1 tree-sitter Python extractor exactly:
// `pkg.mod.func`, `pkg.mod.Class`, `pkg.mod.Class.method`, where the module
// is the dotted file path (`pkg/__init__.py` -> `pkg.__init__`).
//
// pyright behavior notes (verified empirically against pyright 1.1.414):
// - initialize MUST carry workspaceFolders. rootUri alone is ignored: the
//   server falls back to a nonexistent "<default workspace root>", no import
//   resolves, and every cross-file references query returns [].
// - references only report cross-file sites after textDocument/didOpen.
// - Class instantiation (`Service()`) is reported as a reference to the CLASS
//   symbol, never to `__init__`; instantiation edges therefore land on the
//   class node, which Pass 1 extracts. `__call__` behaves the same way.
// - Dynamic imports (importlib) do not resolve and produce no references.
//
// The server binary comes from PYRIGHT_LANGSERVER_BIN or PATH. Any failure
// prints "[]" and exits 0: Pass 2 degrades, it never fails a push.

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawn } from "node:child_process";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------
const SKIP_DIRS = new Set([
  ".git",
  "__pycache__",
  "node_modules",
  ".venv",
  "venv",
  ".tox",
  ".mypy_cache",
  ".pytest_cache",
  "build",
  "dist",
  ".eggs",
]);
const INIT_TIMEOUT_MS = 60_000; // Max wait for the initialize response
const REF_TIMEOUT_MS = 5_000; // Per-references-query timeout
const ANALYSIS_MAX_MS = 30_000; // Max wait for pyright's background analysis
const ANALYSIS_QUIET_MS = 3_000; // Proceed anyway if no progress is reported

// ---------------------------------------------------------------------------
// LSP client (Content-Length framed JSON-RPC over stdio)
// ---------------------------------------------------------------------------
class LspClient {
  constructor(proc) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = "";
    this.ready = false;
    this.beginProgressSeen = false;
    this.endProgressSeen = false;
    this.analysisWaiters = [];

    proc.stdout.on("data", (chunk) => {
      this.buffer += chunk.toString();
      this.processBuffer();
    });
    proc.stderr.on("data", (chunk) => process.stderr.write(chunk));
  }

  processBuffer() {
    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;
      const match = this.buffer.slice(0, headerEnd).match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        this.buffer = this.buffer.slice(headerEnd + 4);
        continue;
      }
      const contentLen = parseInt(match[1], 10);
      const bodyStart = headerEnd + 4;
      if (this.buffer.length < bodyStart + contentLen) break;
      const body = this.buffer.slice(bodyStart, bodyStart + contentLen);
      this.buffer = this.buffer.slice(bodyStart + contentLen);
      let msg;
      try {
        msg = JSON.parse(body);
      } catch {
        continue;
      }
      if (msg.id !== undefined && this.pending.has(msg.id)) {
        const { resolve, timer } = this.pending.get(msg.id);
        clearTimeout(timer);
        this.pending.delete(msg.id);
        resolve(msg);
      } else if (msg.method) {
        this.handleNotification(msg);
      }
    }
  }

  handleNotification(msg) {
    if (msg.method === "pyright/beginProgress") this.beginProgressSeen = true;
    if (msg.method === "pyright/endProgress") {
      this.endProgressSeen = true;
      for (const w of this.analysisWaiters) w();
      this.analysisWaiters = [];
    }
  }

  send(method, params) {
    const id = this.nextId++;
    const msg = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    this.proc.stdin.write(`Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`);
    return new Promise((resolve) => {
      // The timer must be cleared on resolve: an uncleared timeout keeps
      // node's event loop (and this process) alive until it fires.
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ result: null }); // Timeout -> empty result
      }, REF_TIMEOUT_MS);
      this.pending.set(id, { resolve, timer });
    });
  }

  notify(method, params) {
    const msg = JSON.stringify({ jsonrpc: "2.0", method, params });
    this.proc.stdin.write(`Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`);
  }

  async initialize(rootUri) {
    // workspaceFolders is REQUIRED: pyright ignores rootUri alone (see header).
    const resp = await new Promise((resolve, reject) => {
      const id = this.nextId++;
      const msg = JSON.stringify({
        jsonrpc: "2.0",
        id,
        method: "initialize",
        params: {
          processId: process.pid,
          rootUri,
          workspaceFolders: [{ uri: rootUri, name: "repo" }],
          capabilities: {
            textDocument: {
              references: { dynamicRegistration: false },
              synchronization: { dynamicRegistration: false },
            },
          },
        },
      });
      this.proc.stdin.write(`Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`);
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("LSP initialize timeout"));
      }, INIT_TIMEOUT_MS);
      this.pending.set(id, { resolve, timer });
    });
    this.notify("initialized", {});
    this.ready = true;
    return resp;
  }

  // Wait for pyright's background analysis to finish so cross-file reference
  // queries see resolved imports. If the server reports no progress at all
  // (e.g. a minimal stub server), proceed after a short quiet window.
  async waitForAnalysis() {
    if (this.endProgressSeen) return;
    const start = Date.now();
    while (!this.endProgressSeen) {
      const elapsed = Date.now() - start;
      if (this.beginProgressSeen ? elapsed >= ANALYSIS_MAX_MS : elapsed >= ANALYSIS_QUIET_MS) {
        return;
      }
      if (this.beginProgressSeen) {
        await new Promise((r) => this.analysisWaiters.push(r));
      } else {
        await new Promise((r) => setTimeout(r, 100));
      }
    }
  }

  async shutdown() {
    if (!this.ready) return;
    try {
      await this.send("shutdown", null);
      this.notify("exit", null);
    } catch {
      // Ignore
    }
    setTimeout(() => {
      try {
        this.proc.kill();
      } catch {
        // Ignore
      }
    }, 500);
  }
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------
function discoverPyFiles(dir) {
  const results = [];
  function walk(d) {
    let entries;
    try {
      entries = fs.readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (SKIP_DIRS.has(entry.name)) continue;
      const full = path.join(d, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile() && entry.name.endsWith(".py")) results.push(full);
    }
  }
  walk(dir);
  return results;
}

// ---------------------------------------------------------------------------
// Line classification: which lines can open/close scopes, which belong to
// import statements, which are decorators.
// ---------------------------------------------------------------------------
function walkLine(raw, parenDepth) {
  // Scan one line outside a string: count brackets, detect an unterminated
  // triple-quoted string, stop at a comment.
  let depth = parenDepth;
  let quote = null;
  for (let j = 0; j < raw.length; j++) {
    const c = raw[j];
    if (quote !== null) {
      if (c === "\\") j++;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === "#") break;
    if (c === "'" || c === '"') {
      const triple = raw.slice(j, j + 3);
      if (triple === c.repeat(3)) {
        const close = raw.indexOf(triple, j + 3);
        if (close === -1) return { parenDepth: depth, stringDelim: triple };
        j = close + 2;
      } else {
        quote = c;
      }
      continue;
    }
    if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") depth = Math.max(0, depth - 1);
  }
  return { parenDepth: depth, stringDelim: null };
}

function classifyLines(lines) {
  const info = lines.map(() => ({ significant: false, importSpan: false, decorator: false }));
  let parenDepth = 0;
  let stringDelim = null;
  let importOpen = false; // inside a multi-line import statement
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    if (stringDelim !== null) {
      if (raw.includes(stringDelim)) stringDelim = null;
      continue; // string content: never significant
    }
    if (parenDepth > 0) {
      // Continuation of a bracketed expression opened on an earlier line.
      const res = walkLine(raw, parenDepth);
      parenDepth = res.parenDepth;
      stringDelim = res.stringDelim;
      info[i].importSpan = importOpen;
      if (parenDepth === 0) importOpen = false;
      continue;
    }
    if (importOpen) {
      // Backslash continuation of an import statement.
      info[i].importSpan = true;
      if (!/[\\]\s*$/.test(raw)) importOpen = false;
      continue;
    }
    const trimmed = raw.trim();
    if (trimmed === "" || trimmed.startsWith("#")) continue;
    if (trimmed.startsWith("@")) {
      // Decorator line: not scope-significant - it belongs to the following
      // def/class, which closes the previous scope at the decorator's line.
      info[i].decorator = true;
      const res = walkLine(raw, 0);
      parenDepth = res.parenDepth;
      stringDelim = res.stringDelim;
      continue;
    }
    info[i].significant = true;
    const isImport = /^\s*(from\s+[\w.][\w.\s]*import\b|import\b)/.test(raw);
    info[i].importSpan = isImport;
    const res = walkLine(raw, 0);
    parenDepth = res.parenDepth;
    stringDelim = res.stringDelim;
    if (isImport && (parenDepth > 0 || /[\\]\s*$/.test(raw))) importOpen = true;
  }
  return info;
}

// ---------------------------------------------------------------------------
// Definition extraction
// ---------------------------------------------------------------------------
function expandTabs(line) {
  return line.replace(/\t/g, "        ");
}

// Dotted module name from a file path, matching the Pass 1 extractor
// (crates/gyre-domain/src/python_extractor.rs path_to_module_qname):
// `src/api/handlers.py` -> `src.api.handlers`, `pkg/__init__.py` -> `pkg.__init__`.
function pathToModule(repoRoot, filePath) {
  const rel = path.relative(repoRoot, filePath);
  return rel.replace(/\.py$/, "").replace(/[/\\]/g, ".");
}

function extractDefs(repoRoot, filePath) {
  const content = fs.readFileSync(filePath, "utf8");
  const lines = content.split("\n");
  const info = classifyLines(lines);
  const module = pathToModule(repoRoot, filePath);

  const defs = [];
  // Stack of open scopes: { indent, kind, name, qualName, def }.
  const stack = [];
  let decorators = [];
  // Close every open scope at `indent` or shallower. The scope ends on the
  // line before `closeLine` (the first decorator line when the closing
  // statement is a decorated definition).
  const closeScopes = (indent, closeLine) => {
    while (stack.length && stack[stack.length - 1].indent >= indent) {
      const scope = stack.pop();
      scope.def.endLine = closeLine - 1;
    }
  };

  for (let i = 0; i < lines.length; i++) {
    if (!info[i].significant) {
      if (info[i].decorator) decorators.push(i);
      continue;
    }
    const line = expandTabs(lines[i]);
    const m = line.match(/^(\s*)(?:async\s+)?(def|class)\s+(\w+)/);
    const indent = line.match(/^\s*/)[0].length;
    if (!m) {
      closeScopes(indent, decorators.length ? decorators[0] : i);
      decorators = [];
      continue;
    }
    const [, , kind, name] = m;
    closeScopes(indent, decorators.length ? decorators[0] : i);

    const parentQual = stack.length ? stack[stack.length - 1].qualName : module;
    const qualName = `${parentQual}.${name}`;
    // startLine includes the decorator lines (they belong to this def);
    // defLine is the line carrying the def/class keyword.
    const startLine = decorators.length ? decorators[0] : i;
    // Pass 1 extracts: every class, every method (defs whose enclosing scopes
    // are all classes), and public module-level functions only.
    const classesOnly = stack.every((s) => s.kind === "class");
    const queryable =
      kind === "class" || (classesOnly && !(stack.length === 0 && name.startsWith("_")));
    // The references position is the NAME on the raw (unexpanded) line: LSP
    // columns are UTF-16 offsets in the real file, so tab expansion must not
    // shift them.
    const character = lines[i].indexOf(`${kind} ${name}`) + kind.length + 1;
    const def = {
      kind,
      name,
      qualName,
      module,
      indent,
      startLine,
      defLine: i,
      endLine: lines.length - 1,
      character,
      queryable,
    };
    defs.push(def);
    stack.push({ indent, kind, name, qualName, def });
    decorators = [];
  }
  return { module, defs, info };
}

// Innermost definition covering `line` (decorator lines belong to the
// definition they decorate). Returns null for module-level references.
function findEnclosingDef(defs, line) {
  let best = null;
  for (const def of defs) {
    if (def.startLine <= line && line <= def.endLine) {
      if (!best || def.indent > best.indent || (def.indent === best.indent && def.startLine > best.startLine)) {
        best = def;
      }
    }
  }
  return best;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
const repoRoot = process.argv[2];
if (!repoRoot) {
  console.error("Usage: node python-callgraph-pyright.mjs <repo-path>");
  process.exit(1);
}
const absRoot = path.resolve(repoRoot);
if (!fs.existsSync(absRoot)) {
  console.error(`Directory not found: ${absRoot}`);
  console.log("[]");
  process.exit(0);
}

async function main() {
  const pyFiles = discoverPyFiles(absRoot);
  if (pyFiles.length === 0) {
    console.log("[]");
    return;
  }
  process.stderr.write(`python-callgraph: found ${pyFiles.length} .py files\n`);

  // Definition index per file.
  const perFile = new Map();
  let totalDefs = 0;
  let queryableDefs = 0;
  for (const f of pyFiles) {
    const parsed = extractDefs(absRoot, f);
    perFile.set(f, parsed);
    totalDefs += parsed.defs.length;
    queryableDefs += parsed.defs.filter((d) => d.queryable).length;
  }
  process.stderr.write(
    `python-callgraph: ${totalDefs} definitions (${queryableDefs} queryable)\n`
  );

  const serverBin = process.env.PYRIGHT_LANGSERVER_BIN || "pyright-langserver";
  const rootUri = pathToFileURL(absRoot).href;

  const server = await new Promise((resolve) => {
    const proc = spawn(serverBin, ["--stdio"], {
      stdio: ["pipe", "pipe", "pipe"],
      cwd: absRoot,
    });
    // Resolve on the first of spawn/error so a missing binary (ENOENT, fired
    // asynchronously) degrades instead of hanging until the init timeout.
    proc.once("error", (err) => resolve({ err }));
    proc.once("spawn", () => resolve({ proc }));
  });
  if (server.err) {
    process.stderr.write(
      `python-callgraph: cannot start ${serverBin}: ${server.err.message}\n`
    );
    console.log("[]");
    return;
  }

  const client = new LspClient(server.proc);
  try {
    await client.initialize(rootUri);
    process.stderr.write("python-callgraph: pyright-langserver initialized\n");

    // Open every file so pyright analyzes it and resolves cross-file imports.
    for (const f of pyFiles) {
      client.notify("textDocument/didOpen", {
        textDocument: {
          uri: pathToFileURL(f).href,
          languageId: "python",
          version: 1,
          text: fs.readFileSync(f, "utf8"),
        },
      });
    }
    await client.waitForAnalysis();

    const edges = [];
    let processed = 0;
    for (const [filePath, file] of perFile) {
      const fileUri = pathToFileURL(filePath).href;

      for (const def of file.defs) {
        if (!def.queryable) continue;
        processed++;
        if (processed % 200 === 0) {
          process.stderr.write(
            `python-callgraph: processed ${processed}/${queryableDefs} definitions\n`
          );
        }

        const resp = await client.send("textDocument/references", {
          textDocument: { uri: fileUri },
          position: { line: def.defLine, character: def.character },
          context: { includeDeclaration: false },
        });
        if (!resp.result || !Array.isArray(resp.result)) continue;

        for (const ref of resp.result) {
          if (!ref.uri) continue;
          let refPath;
          try {
            refPath = fileURLToPath(ref.uri);
          } catch {
            continue;
          }
          const refLine = ref.range?.start?.line ?? 0;
          const refFile = perFile.get(refPath);
          if (!refFile) continue;

          // Import sites bind names; they are not calls.
          if (refFile.info[refLine]?.importSpan) continue;

          const enclosing = findEnclosingDef(refFile.defs, refLine);
          const fromQual = enclosing ? enclosing.qualName : refFile.module;
          if (fromQual === def.qualName) continue; // self-reference

          edges.push({ from: fromQual, to: def.qualName, line: refLine + 1 });
        }
      }
    }

    // Deduplicate.
    const seen = new Set();
    const unique = edges.filter((e) => {
      const key = `${e.from}|${e.to}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });

    process.stderr.write(`python-callgraph: extracted ${unique.length} unique call edges\n`);
    console.log(JSON.stringify(unique));
  } finally {
    await client.shutdown();
  }
}

main().catch((err) => {
  process.stderr.write(`python-callgraph: error: ${err.message}\n`);
  console.log("[]");
  process.exit(0); // Never fail the extraction pipeline
});
