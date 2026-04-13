#!/usr/bin/env node
// rust-callgraph.mjs — Extract caller→callee edges from a Rust project
// using rust-analyzer LSP for full type-resolved call graph.
//
// Usage: node scripts/rust-callgraph.mjs /path/to/rust-project
// Output: JSON array to stdout:
//   [{"from": "crate::module::func", "to": "crate::other::func", "line": 42}, ...]
//
// Approach:
// 1. Start rust-analyzer as an LSP subprocess
// 2. Collect all function/method definitions from the syn-based pass 1 output (stdin)
//    or by scanning .rs files for fn declarations
// 3. For each definition, send textDocument/references to find all call sites
// 4. For each call site, determine the enclosing function → emit a Calls edge
// 5. Output JSON

import * as path from "node:path";
import * as fs from "node:fs";
import { spawn } from "node:child_process";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------
const SKIP_DIRS = new Set(["target", ".git", "node_modules", ".cargo"]);
const TIMEOUT_MS = 60_000; // Max time for rust-analyzer init
const REF_TIMEOUT_MS = 5_000; // Per-reference query timeout

// ---------------------------------------------------------------------------
// LSP Client
// ---------------------------------------------------------------------------
class LspClient {
  constructor(proc) {
    this.proc = proc;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = "";
    this.ready = false;

    proc.stdout.on("data", (chunk) => {
      this.buffer += chunk.toString();
      this._processBuffer();
    });

    proc.stderr.on("data", (chunk) => {
      // Log rust-analyzer stderr to our stderr
      process.stderr.write(chunk);
    });
  }

  _processBuffer() {
    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;

      const header = this.buffer.slice(0, headerEnd);
      const match = header.match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        this.buffer = this.buffer.slice(headerEnd + 4);
        continue;
      }

      const contentLen = parseInt(match[1], 10);
      const bodyStart = headerEnd + 4;
      if (this.buffer.length < bodyStart + contentLen) break;

      const body = this.buffer.slice(bodyStart, bodyStart + contentLen);
      this.buffer = this.buffer.slice(bodyStart + contentLen);

      try {
        const msg = JSON.parse(body);
        if (msg.id !== undefined && this.pending.has(msg.id)) {
          const { resolve } = this.pending.get(msg.id);
          this.pending.delete(msg.id);
          resolve(msg);
        }
      } catch {
        // Ignore parse errors (notifications, etc.)
      }
    }
  }

  send(method, params) {
    const id = this.nextId++;
    const msg = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    const packet = `Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`;
    this.proc.stdin.write(packet);

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          resolve({ result: null }); // Timeout → null result
        }
      }, REF_TIMEOUT_MS);
    });
  }

  notify(method, params) {
    const msg = JSON.stringify({ jsonrpc: "2.0", method, params });
    const packet = `Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`;
    this.proc.stdin.write(packet);
  }

  async initialize(rootUri) {
    const resp = await new Promise((resolve, reject) => {
      const id = this.nextId++;
      const msg = JSON.stringify({
        jsonrpc: "2.0",
        id,
        method: "initialize",
        params: {
          processId: process.pid,
          rootUri,
          capabilities: {
            textDocument: {
              references: { dynamicRegistration: false },
              definition: { dynamicRegistration: false },
            },
          },
          initializationOptions: {},
        },
      });
      const packet = `Content-Length: ${Buffer.byteLength(msg)}\r\n\r\n${msg}`;
      this.proc.stdin.write(packet);
      this.pending.set(id, { resolve, reject });
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error("LSP initialize timeout"));
        }
      }, TIMEOUT_MS);
    });

    this.notify("initialized", {});
    this.ready = true;
    return resp;
  }

  shutdown() {
    if (!this.ready) return;
    try {
      this.notify("shutdown", null);
      this.notify("exit", null);
    } catch {
      // Ignore
    }
    this.proc.kill();
  }
}

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------
function discoverRsFiles(dir) {
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
      else if (entry.isFile() && entry.name.endsWith(".rs")) results.push(full);
    }
  }
  walk(dir);
  return results;
}

// ---------------------------------------------------------------------------
// Extract function definitions from .rs files using simple regex
// ---------------------------------------------------------------------------
function extractFnDefs(filePath) {
  const content = fs.readFileSync(filePath, "utf-8");
  const lines = content.split("\n");
  const defs = [];

  // Match: pub/pub(crate)/async fn name, fn name, pub async fn name
  const fnRegex =
    /^[\t ]*(pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)/;
  // Match: impl blocks for context
  const implRegex = /^[\t ]*impl(?:<[^>]*>)?\s+(?:(\w+(?:::\w+)*)\s+for\s+)?(\w+)/;

  let currentImpl = null;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    const implMatch = line.match(implRegex);
    if (implMatch) {
      currentImpl = implMatch[2]; // The type being impl'd
    }

    const fnMatch = line.match(fnRegex);
    if (fnMatch) {
      const fnName = fnMatch[2];
      // Skip test functions for call graph edges (they're callers, detected by test_node flag)
      const qualName = currentImpl ? `${currentImpl}::${fnName}` : fnName;
      defs.push({
        name: fnName,
        qualName,
        line: i, // 0-indexed
        character: line.indexOf(fnName),
      });
    }

    // Reset impl context at closing braces at column 0
    if (line.match(/^}/)) {
      currentImpl = null;
    }
  }

  return defs;
}

// ---------------------------------------------------------------------------
// Determine enclosing function at a given position
// ---------------------------------------------------------------------------
function findEnclosingFn(filePath, line, fnDefs) {
  // Find the last fn definition before this line
  let best = null;
  for (const def of fnDefs) {
    if (def.line <= line) {
      if (!best || def.line > best.line) best = def;
    }
  }
  return best;
}

// ---------------------------------------------------------------------------
// Build qualified name from file path
// ---------------------------------------------------------------------------
function fileToModule(repoRoot, filePath) {
  let rel = path.relative(repoRoot, filePath);
  // Strip .rs extension
  rel = rel.replace(/\.rs$/, "");
  // Convert path separators to ::
  rel = rel.replace(/[/\\]/g, "::");
  // Handle lib.rs and mod.rs
  rel = rel.replace(/::mod$/, "");
  rel = rel.replace(/::lib$/, "");
  // Handle src/ prefix
  rel = rel.replace(/^src::/, "");
  return rel;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
const repoRoot = process.argv[2];
if (!repoRoot) {
  console.error("Usage: node rust-callgraph.mjs <repo-path>");
  process.exit(1);
}

const absRoot = path.resolve(repoRoot);
if (!fs.existsSync(absRoot)) {
  console.error(`Directory not found: ${absRoot}`);
  process.exit(1);
}

// Check for Cargo.toml
if (!fs.existsSync(path.join(absRoot, "Cargo.toml"))) {
  console.error(`No Cargo.toml found in ${absRoot}`);
  console.log("[]");
  process.exit(0);
}

async function main() {
  const rsFiles = discoverRsFiles(absRoot);
  if (rsFiles.length === 0) {
    console.log("[]");
    return;
  }

  process.stderr.write(
    `rust-callgraph: found ${rsFiles.length} .rs files\n`
  );

  // Build function definitions index per file
  const fileDefsMap = new Map();
  let totalDefs = 0;
  for (const f of rsFiles) {
    const defs = extractFnDefs(f);
    fileDefsMap.set(f, defs);
    totalDefs += defs.length;
  }
  process.stderr.write(`rust-callgraph: found ${totalDefs} function definitions\n`);

  // Start rust-analyzer
  const raProc = spawn("rust-analyzer", [], {
    stdio: ["pipe", "pipe", "pipe"],
    cwd: absRoot,
  });

  const client = new LspClient(raProc);

  try {
    const rootUri = `file://${absRoot}`;
    await client.initialize(rootUri);
    process.stderr.write("rust-callgraph: rust-analyzer initialized\n");

    // Wait a bit for indexing
    await new Promise((r) => setTimeout(r, 3000));

    const edges = [];
    let processed = 0;

    // For each file with function definitions, find references
    for (const [filePath, defs] of fileDefsMap) {
      const fileUri = `file://${filePath}`;
      const moduleName = fileToModule(absRoot, filePath);

      for (const def of defs) {
        processed++;
        if (processed % 50 === 0) {
          process.stderr.write(
            `rust-callgraph: processed ${processed}/${totalDefs} definitions\n`
          );
        }

        // Send textDocument/references request
        const resp = await client.send("textDocument/references", {
          textDocument: { uri: fileUri },
          position: { line: def.line, character: def.character },
          context: { includeDeclaration: false },
        });

        if (!resp.result || !Array.isArray(resp.result)) continue;

        for (const ref of resp.result) {
          const refUri = ref.uri;
          if (!refUri) continue;
          const refPath = refUri.replace("file://", "");
          const refLine = ref.range?.start?.line ?? 0;

          // Find enclosing function at the reference site
          const refDefs = fileDefsMap.get(refPath);
          if (!refDefs) continue;

          const enclosing = findEnclosingFn(refPath, refLine, refDefs);
          if (!enclosing) continue;

          const refModule = fileToModule(absRoot, refPath);
          const fromQual = refModule
            ? `${refModule}::${enclosing.qualName}`
            : enclosing.qualName;
          const toQual = moduleName
            ? `${moduleName}::${def.qualName}`
            : def.qualName;

          // Skip self-calls
          if (fromQual === toQual) continue;

          edges.push({
            from: fromQual,
            to: toQual,
            line: refLine + 1, // 1-indexed for consistency
          });
        }
      }
    }

    // Deduplicate edges
    const seen = new Set();
    const unique = edges.filter((e) => {
      const key = `${e.from}|${e.to}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });

    process.stderr.write(
      `rust-callgraph: extracted ${unique.length} unique call edges\n`
    );
    console.log(JSON.stringify(unique));
  } finally {
    client.shutdown();
  }
}

main().catch((err) => {
  process.stderr.write(`rust-callgraph: error: ${err.message}\n`);
  console.log("[]");
  process.exit(0); // Don't fail the extraction pipeline
});
