// Agent event decoding: fold raw agent JSONL streams (omp .agent.jsonl
// mirror / tail -F relay) into readable transcript lines:
//   [think] ...   — folded thinking text
//   agent prose    — text_delta accumulation
//   $ cmd          — tool calls
//   -> result tail — tool execution results
// Ported unchanged from the original dashboard; feeds the classifier.
export function decodeAgentEvents(text) {
  let out = "";
  let thinkBuf = "", textBuf = "", toolBuf = "", toolName = "";
  const flushThink = () => {
    if (thinkBuf) {
      out += "[think] " + thinkBuf.replace(/\s+/g, " ").slice(-2000) + "\n";
      thinkBuf = "";
    }
  };
  const flushText = () => { if (textBuf) { out += textBuf + "\n"; textBuf = ""; } };
  const flushTool = () => {
    if (toolBuf) {
      let cmd = toolBuf;
      try { cmd = JSON.parse(toolBuf).command || toolName + " " + toolBuf; } catch {}
      out += "$ " + String(cmd).slice(0, 500) + "\n";
      toolBuf = "";
    }
  };
  for (const line of text.split("\n")) {
    if (line === "===STREAM-OPEN===" || line === "===NEXT-FILE===" || line === "===WORKER-LOG===") {
      flushThink(); flushText(); flushTool();
      if (line !== "===WORKER-LOG===") continue;
      out += "--- worker log ---\n";
      continue;
    }
    if (!line.startsWith("{")) { flushThink(); flushText(); flushTool(); out += line + "\n"; continue; }
    let d; try { d = JSON.parse(line); } catch { out += line + "\n"; continue; }
    const ev = d.assistantMessageEvent || d;
    switch (ev.type || d.type) {
      case "thinking_delta": thinkBuf += ev.delta || ""; break;
      case "thinking_start": case "text_start": case "toolcall_start": break;
      case "text_delta": flushThink(); textBuf += ev.delta || ""; break;
      case "text_end": flushThink(); if (ev.content) textBuf += ev.content; break;
      case "toolcall_delta": flushThink(); flushText(); toolBuf += ev.delta || ""; break;
      case "toolcall_end":
        flushThink(); flushText();
        if (ev.toolCall && ev.toolCall.name) {
          toolName = ev.toolCall.name;
          const a = ev.toolCall.arguments || {};
          out += "$ " + (a.command || a.path || JSON.stringify(a)).toString().slice(0, 500) + "\n";
          toolBuf = "";
        }
        break;
      case "tool_execution_start":
        flushThink(); flushText(); flushTool();
        if (ev.args && ev.toolName) {
          out += "$ " + (ev.args.command || ev.args.path || JSON.stringify(ev.args)).toString().slice(0, 500) + "\n";
        }
        break;
      case "tool_execution_end":
        flushThink(); flushText(); flushTool();
        if (ev.result && ev.result.content) {
          const txt = ev.result.content.map((c) => c.text || "").join("").trim();
          if (txt) out += "-> " + txt.split("\n").slice(0, 6).join("\n   ").slice(0, 1500) + "\n";
        }
        break;
      case "message_end": case "turn_end": case "message_start": case "turn_start":
      case "tool_execution_update": default: break;
    }
  }
  flushThink(); flushText(); flushTool();
  return out.trimEnd();
}
