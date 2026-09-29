#!/usr/bin/env node
// omp jsonl event stream -> human-readable progress lines (one line per event).
// Filters to the meaningful events: tool calls (with short arg summaries),
// assistant text, errors. Read stdin, write stdout, line-buffered.
import { createInterface } from "node:readline";

const rl = createInterface({ input: process.stdin });

function fmtArgs(args) {
  if (!args || typeof args !== "object") return "";
  const parts = [];
  for (const [k, v] of Object.entries(args).slice(0, 4)) {
    const s = typeof v === "string" ? v : JSON.stringify(v);
    if (s === undefined) continue;
    parts.push(`${k}=${s.length > 60 ? s.slice(0, 57) + "…" : s.replace(/\n/g, " ")}`);
  }
  return parts.join(" ");
}

function fmtResult(result) {
  if (!result) return "";
  if (typeof result === "string") return result.slice(0, 80).replace(/\n/g, " ");
  if (result.error) return `error: ${String(result.error).slice(0, 80)}`;
  const c = result.content;
  if (Array.isArray(c)) {
    const text = c.filter((x) => x.type === "text").map((x) => x.text).join(" ");
    return text.slice(0, 80).replace(/\n/g, " ");
  }
  return "";
}

rl.on("line", (line) => {
  if (!line.trim()) return;
  let ev;
  try {
    ev = JSON.parse(line);
  } catch {
    return;
  }
  switch (ev.type) {
    case "message_update":
    case "message_end": {
      const m = ev.message;
      if (!m || m.role !== "assistant") return;
      for (const part of m.content || []) {
        if (part.type === "text" && part.text.trim()) {
          process.stdout.write(`\n[assistant]\n${part.text.trim()}\n`);
        } else if (part.type === "thinking" && part.thinking) {
          process.stdout.write(`(thinking…) ${part.thinking.slice(0, 100).replace(/\n/g, " ")}\n`);
          return; // only first thinking chunk per message — updates stream in
        }
        break; // one line per event, not per content block
      }
      return;
    }
    case "tool_execution_start":
      process.stdout.write(`→ ${ev.toolName}(${fmtArgs(ev.args)})\n`);
      return;
    case "tool_execution_end":
      process.stdout.write(`✓ ${ev.toolName}: ${fmtResult(ev.result) || "ok"}\n`);
      return;
    case "turn_end":
      return;
    default:
      return;
  }
});
