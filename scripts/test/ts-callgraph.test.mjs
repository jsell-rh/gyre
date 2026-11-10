// Unit tests for the ts-callgraph driver's pure mapping logic
// (scripts/ts-callgraph.mjs). The LSP wire behavior (initialize →
// documentSymbol → references → edges) is pinned end-to-end by the
// recorded-protocol Rust tests in crates/gyre-adapters/src/call_graph.rs
// (`ts_driver_*`) and by the real-server run documented in
// specs/tasks/task-075.md — these tests cover the pieces in between.

import { test } from "node:test";
import assert from "node:assert/strict";
import * as path from "node:path";
import {
  encodeMessage,
  extractMessages,
  moduleQualifiedName,
  uriToPath,
  variableLooksLikeFunction,
  findEnclosingChain,
  collectQueryTargets,
  dedupEdges,
  SymbolKind,
} from "../ts-callgraph.mjs";

// ---------------------------------------------------------------------------
// LSP wire framing
// ---------------------------------------------------------------------------

test("encodeMessage frames payload with byte length", () => {
  // "é" is 2 bytes in UTF-8 but 1 char — the header must count bytes or the
  // server would truncate the body mid-character.
  const packet = encodeMessage({ jsonrpc: "2.0", id: 1, method: "m", params: { s: "é" } });
  const [header, body] = packet.split("\r\n\r\n");
  assert.match(header, /^Content-Length: \d+$/);
  assert.equal(Buffer.byteLength(body), parseInt(header.match(/(\d+)/)[1], 10));
});

test("extractMessages returns complete messages and keeps partial tail", () => {
  const a = encodeMessage({ jsonrpc: "2.0", id: 1, result: "x" });
  const b = encodeMessage({ jsonrpc: "2.0", id: 2, result: "y" });
  const wire = Buffer.concat([
    Buffer.from(a, "utf8"),
    Buffer.from(b, "utf8").subarray(0, 10),
  ]);
  const { messages, rest } = extractMessages(wire);
  assert.equal(messages.length, 1);
  assert.equal(messages[0].result, "x");
  assert.equal(rest.toString("utf8"), b.slice(0, 10));
});

test("extractMessages skips malformed headers without losing later messages", () => {
  const good = encodeMessage({ jsonrpc: "2.0", id: 7, result: null });
  const garbage = Buffer.from("not-a-header\r\n\r\n" + good, "utf8");
  const { messages } = extractMessages(garbage);
  assert.equal(messages.length, 1);
  assert.equal(messages[0].id, 7);
});

test("extractMessages waits for the full body across chunk boundaries", () => {
  const msg = encodeMessage({ jsonrpc: "2.0", id: 3, result: "hello" });
  const wire = Buffer.from(msg, "utf8");
  const first = extractMessages(wire.subarray(0, 25));
  assert.equal(first.messages.length, 0);
  const second = extractMessages(Buffer.concat([first.rest, wire.subarray(25)]));
  assert.equal(second.messages.length, 1);
  assert.equal(second.messages[0].result, "hello");
  assert.equal(second.rest.length, 0);
});

test("extractMessages counts Content-Length in bytes, not UTF-16 code units", () => {
  // Regression (task-075 review): response bodies with multi-byte UTF-8 —
  // e.g. documentSymbol results for files with non-ASCII symbol names,
  // which typescript-language-server emits as raw UTF-8 — never reassembled
  // when the length gate ran on decoded-string code units: the body is
  // longer in bytes than in code units, so the gate could never be
  // satisfied and the request timed out, silently losing every Calls edge.
  const msg = encodeMessage({ jsonrpc: "2.0", id: 5, result: "café ☕" });
  const wire = Buffer.from(msg, "utf8");
  const { messages, rest } = extractMessages(wire);
  assert.equal(messages.length, 1);
  assert.equal(messages[0].result, "café ☕");
  assert.equal(rest.length, 0);
});

test("extractMessages reassembles a body split inside a multi-byte sequence", () => {
  // A TCP/pipe chunk boundary can land between the bytes of one character;
  // the partial tail must be carried as bytes so reassembly is exact.
  const msg = encodeMessage({ jsonrpc: "2.0", id: 6, result: "résumé" });
  const wire = Buffer.from(msg, "utf8");
  const bodyStart = wire.indexOf("\r\n\r\n") + 4;
  // First 0xc3 byte inside the body: the lead byte of the é in "résumé".
  const split = wire.indexOf(0xc3, bodyStart);
  assert.ok(split > bodyStart, "fixture must contain a multi-byte character");
  const first = extractMessages(wire.subarray(0, split));
  assert.equal(first.messages.length, 0);
  assert.ok(first.rest.length > 0);
  const second = extractMessages(Buffer.concat([first.rest, wire.subarray(split)]));
  assert.equal(second.messages.length, 1);
  assert.equal(second.messages[0].result, "résumé");
  assert.equal(second.rest.length, 0);
});

test("extractMessages does not swallow the frame after a non-ASCII body", () => {
  // Regression (task-075 review): an over-long code-unit slice used to eat
  // the next frame's header, dropping the message after any non-ASCII
  // notification (window/logMessage with non-ASCII text precedes responses
  // in the real server's stream).
  const a = encodeMessage({
    jsonrpc: "2.0",
    method: "window/logMessage",
    params: { message: "analyzing café ☕" },
  });
  const b = encodeMessage({ jsonrpc: "2.0", id: 9, result: "ok" });
  const { messages } = extractMessages(
    Buffer.concat([Buffer.from(a, "utf8"), Buffer.from(b, "utf8")])
  );
  assert.equal(messages.length, 2);
  assert.equal(messages[0].params.message, "analyzing café ☕");
  assert.equal(messages[1].result, "ok");
});

// ---------------------------------------------------------------------------
// Qualified names (must match Pass 1 — typescript_extractor.rs)
// ---------------------------------------------------------------------------

test("moduleQualifiedName strips extension and keeps path separators", () => {
  assert.equal(
    moduleQualifiedName("/repo", "/repo/src/components/UserCard.tsx"),
    "src/components/UserCard"
  );
  assert.equal(moduleQualifiedName("/repo", "/repo/api.ts"), "api");
});

test("uriToPath decodes percent escapes", () => {
  assert.equal(uriToPath(["file", "///tmp/a%20b.ts"].join(":")), path.join("/tmp", "a b.ts"));
  assert.equal(uriToPath("not-a-uri"), null);
});

// ---------------------------------------------------------------------------
// Symbol classification
// ---------------------------------------------------------------------------

test("variableLooksLikeFunction detects arrow and function initializers", () => {
  assert.equal(variableLooksLikeFunction("f = (x) => x + 1"), true);
  assert.equal(variableLooksLikeFunction("f = function () { return 1; }"), true);
  assert.equal(variableLooksLikeFunction("config = { a: 1 }"), false);
  assert.equal(variableLooksLikeFunction("n = 42"), false);
});

test("collectQueryTargets selects top-level functions, classes, interfaces, function variables", () => {
  const symbols = [
    { name: "compute", kind: SymbolKind.Function, selectionRange: { start: { line: 2, character: 16 } } },
    { name: "UserService", kind: SymbolKind.Class, selectionRange: { start: { line: 5, character: 6 } } },
    { name: "Repo", kind: SymbolKind.Interface, selectionRange: { start: { line: 9, character: 10 } } },
    { name: "makeGreeting", kind: SymbolKind.Variable, range: { start: { line: 5, character: 13 }, end: { line: 5, character: 58 } }, selectionRange: { start: { line: 5, character: 13 } } },
    { name: "MAX", kind: SymbolKind.Variable, range: { start: { line: 1, character: 13 }, end: { line: 1, character: 20 } }, selectionRange: { start: { line: 1, character: 13 } } },
    { name: "innerHelper", kind: SymbolKind.Method, selectionRange: { start: { line: 3, character: 2 } } },
  ];
  const text = [
    "const MAX = 42;",
    "const other = 7;",
    "class Filler {}",
    "let x = 1;",
    "const plain = 'str';",
    "export const makeGreeting = (name) => `hi ${name}`",
  ].join("\n");
  const targets = collectQueryTargets(symbols, text);
  assert.deepEqual(
    targets.map((t) => t.name),
    ["compute", "UserService", "Repo", "makeGreeting"]
  );
  assert.deepEqual(targets[0].position, { line: 2, character: 16 });
});

// ---------------------------------------------------------------------------
// Enclosing-function resolution at reference sites
// ---------------------------------------------------------------------------

const FILE_TEXT = [
  "import { add } from './math';",
  "export class UserService {",
  "  findUser(id: string) {",
  "    return add(1, id.length);",
  "  }",
  "}",
  "export const makeGreeting = (name: string) => `hi ${name}`;",
  "export function greetTwice() {",
  "  return makeGreeting('a') + makeGreeting('b');",
  "}",
].join("\n");

const FILE_SYMBOLS = [
  {
    name: "UserService",
    kind: SymbolKind.Class,
    range: { start: { line: 1, character: 0 }, end: { line: 5, character: 1 } },
    children: [
      {
        name: "findUser",
        kind: SymbolKind.Method,
        range: { start: { line: 2, character: 2 }, end: { line: 4, character: 3 } },
        children: [],
      },
    ],
  },
  {
    name: "makeGreeting",
    kind: SymbolKind.Variable,
    range: { start: { line: 6, character: 13 }, end: { line: 6, character: 60 } },
    children: [],
  },
  {
    name: "greetTwice",
    kind: SymbolKind.Function,
    range: { start: { line: 7, character: 0 }, end: { line: 9, character: 1 } },
    children: [],
  },
];

test("reference inside a method resolves to Class.method chain", () => {
  // add(1, ...) at line 3, char 11
  assert.deepEqual(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 3, 11), [
    "UserService",
    "findUser",
  ]);
});

test("reference inside a function resolves to that function", () => {
  assert.deepEqual(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 8, 10), ["greetTwice"]);
});

test("top-level import reference has no enclosing function", () => {
  assert.equal(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 0, 9), null);
});

test("reference directly in a class body (no method) has no enclosing function", () => {
  assert.equal(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 1, 30), null);
});

test("position at a range's end boundary is excluded", () => {
  // greetTwice's range ends at line 9 char 1 — a reference there is outside.
  assert.equal(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 9, 1), null);
});

test("reference inside an arrow-function variable resolves to the variable", () => {
  assert.deepEqual(findEnclosingChain(FILE_SYMBOLS, FILE_TEXT, 6, 40), ["makeGreeting"]);
});

// ---------------------------------------------------------------------------
// Dedup
// ---------------------------------------------------------------------------

test("dedupEdges collapses duplicate from-to pairs keeping first line", () => {
  const edges = [
    { from: "a.f", to: "b.g", line: 1 },
    { from: "a.f", to: "b.g", line: 9 },
    { from: "a.h", to: "b.g", line: 3 },
  ];
  assert.deepEqual(dedupEdges(edges), [
    { from: "a.f", to: "b.g", line: 1 },
    { from: "a.h", to: "b.g", line: 3 },
  ]);
});
