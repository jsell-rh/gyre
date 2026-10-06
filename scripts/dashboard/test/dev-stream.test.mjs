import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

test("OMP relay streams text and tool progress without prompts or thinking", async () => {
  const dir = await mkdtemp(join(tmpdir(), "gyre-stream-"));
  const verdict = join(dir, "verdict.txt");
  const child = spawn("node", [resolve("scripts/dev-stream.mjs"), "integration-review", verdict],
    { stdio: ["pipe", "pipe", "pipe"] });
  let output = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { output += chunk; });
  const write = (value) => child.stdin.write(JSON.stringify(value) + "\n");
  write({ type: "message_start", message: { role: "user", content: [{ type: "text", text: "secret prompt" }] } });
  write({ type: "message_end", message: { role: "user", content: [{ type: "text", text: "secret prompt" }] } });
  write({ type: "message_start", message: { role: "assistant" } });
  write({ type: "message_update", assistantMessageEvent: { type: "thinking_delta", delta: "private thought" } });
  write({ type: "message_update", assistantMessageEvent: { type: "text_delta", delta: "Checking code\n" } });
  write({ type: "tool_execution_start", toolCallId: "1", toolName: "bash", intent: "Run tests", args: { command: "cargo test" } });
  write({ type: "tool_execution_update", toolCallId: "1", toolName: "bash", partialResult: { content: [{ type: "text", text: "one" }] } });
  write({ type: "tool_execution_update", toolCallId: "1", toolName: "bash", partialResult: { content: [{ type: "text", text: "one two" }] } });
  write({ type: "tool_execution_end", toolCallId: "1", toolName: "bash", result: { content: [{ type: "text", text: "one two" }] }, isError: false });
  write({ type: "message_update", assistantMessageEvent: { type: "text_delta", delta: "VERDICT: PASS" } });
  child.stdin.end();
  const exit = await new Promise((resolve) => child.on("close", resolve));
  assert.equal(exit, 0);
  assert.match(output, /Checking code/);
  assert.match(output, /"type":"tool_start"/);
  assert.match(output, /"text":" two"/);
  assert.doesNotMatch(output, /secret prompt|private thought/);
  assert.equal(await readFile(verdict, "utf8"), "Checking code\nVERDICT: PASS");
});
