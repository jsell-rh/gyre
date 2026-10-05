// decodeAgentEvents tests: JSONL folding carried over from the original.
import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeAgentEvents } from "../server/agent-events.mjs";

test("plain lines pass through", () => {
  assert.equal(decodeAgentEvents("hello\nworld"), "hello\nworld");
});

test("text_delta accumulates into prose", () => {
  const out = decodeAgentEvents(
    JSON.stringify({ type: "text_delta", delta: "Hello " }) + "\n" +
    JSON.stringify({ type: "text_delta", delta: "world" }) + "\n");
  assert.match(out, /Hello world/);
});

test("tool_execution_start renders $ command", () => {
  const out = decodeAgentEvents(
    JSON.stringify({ type: "tool_execution_start", toolName: "bash", args: { command: "cargo test" } }) + "\n");
  assert.match(out, /^\$ cargo test$/m);
});

test("tool_execution_end renders -> result tail", () => {
  const out = decodeAgentEvents(
    JSON.stringify({ type: "tool_execution_end", result: { content: [{ text: "ok\nline2" }] } }) + "\n");
  assert.match(out, /^-> ok/m);
});

test("thinking folds to single bracketed line", () => {
  const out = decodeAgentEvents(
    JSON.stringify({ type: "thinking_delta", delta: "I am " }) + "\n" +
    JSON.stringify({ type: "thinking_delta", delta: "thinking" }) + "\n");
  assert.match(out, /\[think\] I am thinking/);
});

test("worker log marker passes through as separator", () => {
  assert.match(decodeAgentEvents("===WORKER-LOG==="), /--- worker log ---/);
  assert.equal(decodeAgentEvents("===STREAM-OPEN==="), "");
});

test("null bytes survive to output (cleaned downstream by classifier)", () => {
  const out = decodeAgentEvents("a\x00\x00b");
  assert.ok(out.includes("a"));
});
