// python-callgraph.test.mjs - deterministic coverage for the pyright LSP
// call-graph driver (scripts/python-callgraph-pyright.mjs) using a stub
// pyright-langserver that replays reference locations verified against real
// pyright 1.1.414 on the same fixture. This pins the driver's observable
// contract in CI (LSP framing, initialize handshake with workspaceFolders,
// didOpen, references requests at def-name positions, import-site filtering,
// decorator/module-level/method enclosing resolution, qualified naming,
// dedup, JSON output) without requiring pyright to be installed. Where a real
// pyright-langserver IS available, the Rust adapter test
// (crates/gyre-adapters/src/call_graph.rs) additionally exercises the full
// wrapper -> driver -> real server chain.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const MAIN_PY = [
  "from app.util import helper",
  "from app.deco import wrap",
  "from app.service import Service",
  "",
  "s = Service()",
  "",
  "@wrap",
  "def decorated():",
  "    pass",
  "",
  "def run():",
  "    helper(1)",
  "    s = Service()",
  "    return s.handle()",
  "",
  "def other():",
  "    run()",
  "",
  "helper(2)",
  "",
].join("\n");

// Reference locations per (uri:line:character) def query, exactly as real
// pyright 1.1.414 reports them for this fixture: import sites, call sites,
// the decorator application, and instantiation sites (class symbol, not
// __init__).
function stubRefMap(fixture) {
  const u = (name) => `file://${join(fixture, "app", name)}`;
  return {
    [`${u("util.py")}:0:4`]: [
      { uri: u("main.py"), line: 0, character: 21 },
      { uri: u("main.py"), line: 11, character: 4 },
      { uri: u("main.py"), line: 18, character: 0 },
    ],
    [`${u("deco.py")}:0:4`]: [
      { uri: u("main.py"), line: 1, character: 21 },
      { uri: u("main.py"), line: 6, character: 1 },
    ],
    [`${u("service.py")}:0:6`]: [
      { uri: u("main.py"), line: 2, character: 24 },
      { uri: u("main.py"), line: 4, character: 4 },
      { uri: u("main.py"), line: 12, character: 8 },
    ],
    [`${u("service.py")}:1:8`]: [],
    [`${u("service.py")}:4:8`]: [{ uri: u("main.py"), line: 13, character: 13 }],
    [`${u("main.py")}:7:4`]: [],
    [`${u("main.py")}:10:4`]: [{ uri: u("main.py"), line: 16, character: 4 }],
    [`${u("main.py")}:15:4`]: [],
  };
}

// A minimal pyright-langserver stand-in: Content-Length framed JSON-RPC over
// stdio. Responds to initialize (echoing minimal capabilities), serves
// textDocument/references from the canned map, acknowledges shutdown.
const STUB_SERVER = `
const refs = JSON.parse(require("fs").readFileSync(process.env.STUB_REFS, "utf8"));
let buffer = "";
function write(msg) {
  const body = JSON.stringify(msg);
  process.stdout.write("Content-Length: " + Buffer.byteLength(body) + "\\r\\n\\r\\n" + body);
}
process.stdin.on("data", (chunk) => {
  buffer += chunk.toString();
  while (true) {
    const headerEnd = buffer.indexOf("\\r\\n\\r\\n");
    if (headerEnd === -1) break;
    const m = buffer.slice(0, headerEnd).match(/Content-Length:\\s*(\\d+)/i);
    if (!m) { buffer = buffer.slice(headerEnd + 4); continue; }
    const len = parseInt(m[1], 10);
    const start = headerEnd + 4;
    if (buffer.length < start + len) break;
    const body = buffer.slice(start, start + len);
    buffer = buffer.slice(start + len);
    let msg;
    try { msg = JSON.parse(body); } catch { continue; }
    if (msg.id === undefined) continue; // notifications (didOpen etc.)
    if (msg.method === "initialize") {
      write({ jsonrpc: "2.0", id: msg.id, result: { capabilities: { referencesProvider: true } } });
    } else if (msg.method === "textDocument/references") {
      const pos = msg.params.position;
      const key = msg.params.textDocument.uri + ":" + pos.line + ":" + pos.character;
      const locations = (refs[key] || []).map((l) => ({
        uri: l.uri,
        range: { start: { line: l.line, character: l.character }, end: { line: l.line, character: l.character + 1 } },
      }));
      write({ jsonrpc: "2.0", id: msg.id, result: locations });
    } else {
      write({ jsonrpc: "2.0", id: msg.id, result: null });
    }
  }
});
`;

test("pyright driver maps reference results to Calls edges", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "gyre-pyright-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const fixture = join(dir, "fixture");
  const app = join(fixture, "app");
  await mkdir(app, { recursive: true });
  await writeFile(join(fixture, "pyproject.toml"), "[project]\nname = 'fixture'\n");
  await writeFile(join(app, "__init__.py"), "");
  await writeFile(join(app, "util.py"), "def helper(x):\n    return x + 1\n");
  await writeFile(join(app, "deco.py"), "def wrap(f):\n    return f\n");
  await writeFile(
    join(app, "service.py"),
    "class Service:\n    def __init__(self):\n        self.ready = True\n\n    def handle(self):\n        return 1\n"
  );
  await writeFile(join(app, "main.py"), MAIN_PY);

  const refsFile = join(dir, "refs.json");
  await writeFile(refsFile, JSON.stringify(stubRefMap(fixture)));
  const stub = join(dir, "stub-langserver.cjs");
  await writeFile(stub, "#!/usr/bin/env node\n" + STUB_SERVER);
  await chmod(stub, 0o755);

  const child = spawn("node", [resolve("scripts/python-callgraph-pyright.mjs"), fixture], {
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      PYRIGHT_LANGSERVER_BIN: stub,
      STUB_REFS: refsFile,
    },
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (c) => (stdout += c));
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (c) => (stderr += c));
  const exit = await new Promise((res) => child.on("close", res));

  assert.equal(exit, 0, `driver must exit 0 (stderr: ${stderr})`);
  const edges = JSON.parse(stdout);
  const pairs = edges.map((e) => `${e.from} -> ${e.to}`).sort();

  // The seven edges real pyright yields for this fixture:
  // - import-site references (main.py lines 0-2) are filtered out
  // - the decorator application attributes to the decorated function
  // - module-level calls attribute to the module node
  // - instantiation sites reference the class symbol
  // - method dispatch resolves through the typed receiver
  assert.deepEqual(pairs, [
    "app.main -> app.service.Service",
    "app.main -> app.util.helper",
    "app.main.decorated -> app.deco.wrap",
    "app.main.other -> app.main.run",
    "app.main.run -> app.service.Service",
    "app.main.run -> app.service.Service.handle",
    "app.main.run -> app.util.helper",
  ]);
});
