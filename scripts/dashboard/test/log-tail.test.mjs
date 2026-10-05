// Log tailer tests: partial lines, truncation, rotation, offset correctness.
import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFile, mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createLogTailer } from "../server/log-tail.mjs";

const dir = join(tmpdir(), `gyre-tail-test-${process.pid}`);
const file = join(dir, "stream.log");

test.before(async () => {
  await rm(dir, { recursive: true, force: true });
  await mkdir(dir, { recursive: true });
});

test.after(async () => {
  await rm(dir, { recursive: true, force: true });
});

test("first sighting seeds backfill and starts live tail at EOF", async () => {
  await writeFile(file, "line1\nline2\n");
  const tailer = createLogTailer();
  const seen = [];
  await tailer.pumpFile(file, (chunk, offset, backfill) => {
    if (backfill) seen.push(["backfill", chunk]);
  }, { backfill: 1024 });
  assert.equal(seen.length, 1);
  assert.match(seen[0][1], /line2/);
});

test("partial line without newline is not emitted and not re-read", async () => {
  await writeFile(file, "a\nb\n");
  const tailer = createLogTailer();
  const out = [];
  const pump = () => tailer.pumpFile(file, (chunk) => { out.push(chunk); });
  await pump(); // first sighting: no live output
  // write "partial" without newline
  const fh = await import("node:fs/promises").then((m) => m.open(file, "a"));
  await fh.write("partial-without-newline");
  await fh.close();
  await pump();
  assert.equal(out.length, 0, "no complete line yet");
  // complete the line; must appear exactly once (carry not duplicated)
  const fh2 = await import("node:fs/promises").then((m) => m.open(file, "a"));
  await fh2.write(" tail\n");
  await fh2.close();
  await pump();
  assert.equal(out.length, 1);
  assert.equal(out[0], "partial-without-newline tail\n");
});

test("truncation resets the tailer to the new file start", async () => {
  await writeFile(file, "old1\nold2\nold3\n");
  const tailer = createLogTailer();
  await tailer.pumpFile(file, () => {}); // first sighting
  // truncate + rewrite smaller
  await writeFile(file, "new1\n");
  const out = [];
  await tailer.pumpFile(file, (chunk) => { out.push(chunk); });
  assert.equal(out.length, 1);
  assert.equal(out[0], "new1\n", "reads new content from start after truncate");
});

test("many lines across pumps arrive in order without duplication", async () => {
  await writeFile(file, "");
  const tailer = createLogTailer();
  const out = [];
  await tailer.pumpFile(file, () => {}); // first sighting at EOF (size 0)
  for (let i = 0; i < 5; i++) {
    const fh = await import("node:fs/promises").then((m) => m.open(file, "a"));
    await fh.write(`line-${i}\n`);
    await fh.close();
    await tailer.pumpFile(file, (chunk) => { out.push(chunk); });
  }
  const joined = out.join("");
  assert.equal(joined, "line-0\nline-1\nline-2\nline-3\nline-4\n");
  assert.equal(out.length, 5, "one chunk per pump, no re-reads");
});
