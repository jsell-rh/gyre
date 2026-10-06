#!/usr/bin/env node
// Turn OMP's live JSON events into bounded, readable events in the local
// attempt log. The dashboard ignores prompts and private thinking blocks.
import { createInterface } from "node:readline";
import { writeFileSync } from "node:fs";

const role = process.argv[2] || "agent";
const textOutput = process.argv[3];
const toolOutput = new Map();
let textBuffer = "";
let completeText = "";
let hadTextDelta = false;

function emit(type, fields = {}) {
  process.stdout.write(`GYRE_AGENT_EVENT ${JSON.stringify({ at: Date.now(), role, type, ...fields })}\n`);
}
function flushText() {
  if (!textBuffer) return;
  emit("text", { text: textBuffer });
  textBuffer = "";
}
function appendText(value) {
  if (!value) return;
  completeText += value;
  textBuffer += value;
  if (textBuffer.length >= 300 || textBuffer.includes("\n")) flushText();
}
function outputText(result) {
  return (result?.content || []).filter((part) => part?.type === "text").map((part) => part.text || "").join("\n");
}
function updateTool(event) {
  const id = event.toolCallId || event.toolName || "tool";
  const full = outputText(event.partialResult || event.result);
  const previous = toolOutput.get(id) || "";
  const delta = full.startsWith(previous) ? full.slice(previous.length) : full;
  toolOutput.set(id, full);
  if (delta) emit("tool_output", { name: event.toolName || "tool", text: delta.slice(0, 4096) });
}

const flushTimer = setInterval(flushText, 150);
flushTimer.unref();
try {
  for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    let event;
    try { event = JSON.parse(line); } catch { continue; }
    switch (event.type) {
      case "message_start":
        hadTextDelta = false;
        break;
      case "message_update": {
        const update = event.assistantMessageEvent || {};
        if (update.type === "text_delta") { hadTextDelta = true; appendText(update.delta); }
        break;
      }
      case "message_end":
        if (event.message?.role === "assistant" && !hadTextDelta) {
          appendText(outputText(event.message));
        }
        flushText();
        break;
      case "tool_execution_start": {
        flushText();
        const args = event.args || {};
        const summary = String(event.intent || args.command || args.path || args.file_path || "").slice(0, 240);
        emit("tool_start", { name: event.toolName || "tool", text: summary });
        break;
      }
      case "tool_execution_update":
        updateTool(event);
        break;
      case "tool_execution_end":
        updateTool(event);
        emit("tool_end", { name: event.toolName || "tool", error: Boolean(event.isError) });
        toolOutput.delete(event.toolCallId || event.toolName || "tool");
        break;
    }
  }
} finally {
  clearInterval(flushTimer);
  flushText();
  if (textOutput) writeFileSync(textOutput, completeText);
}
