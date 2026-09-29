#!/usr/bin/env node
// Loop dashboard: watch every gyre-loop tmux pane in one web page.
//
// Serves a single page that polls /api/panes every 1.5s. Each pane is
// captured with `tmux capture-pane` (last N lines) and rendered as a
// terminal card; the orchestrator log (/tmp/gyre-loop.log) gets a card too.
//
// Usage:
//   node scripts/loop-dashboard.mjs
//   GYRE_DASHBOARD_PORT=7690 GYRE_TMUX_SESSION=gyre-loop node scripts/loop-dashboard.mjs
// Then open http://localhost:7690
//
// No dependencies — Node >= 18. Read-only against tmux; it never sends input.

import { createServer } from "node:http";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);
const SESSION = process.env.GYRE_TMUX_SESSION || "gyre-loop";

const LOG_PATH = process.env.GYRE_LOOP_LOG || "/tmp/gyre-loop.log";
const CAPTURE_LINES = 80;

function run(args) {
  return execFileSync("tmux", args, { encoding: "utf8", timeout: 5000 });
}
function listPanes() {
  // One line per pane across all windows of the session.
  // NOTE: list-panes -a ignores -t and lists every session, so capture
  // session_name and filter here.
  const fmt = [
    "#{session_name}", "#{window_index}", "#{window_name}", "#{pane_id}",
    "#{pane_current_command}", "#{pane_dead}", "#{pane_active}",
    "#{pane_width}", "#{pane_height}",
  ].join("\t");
  const out = run(["list-panes", "-a", "-F", fmt]);
  return out
    .split("\n")
    .filter(Boolean)
    .map((line) => line.split("\t"))
    .filter((f) => f[0] === SESSION)
    .map(([sess, widx, wname, pid, cmd, dead, active, w, h]) => {
      // %N pane ids are globally unique — unambiguous capture target.
      return { target: pid, widx: +widx, wname, cmd, dead: dead === "1", active: active === "1", w: +w, h: +h };
    })
    .sort((a, b) => a.widx - b.widx);
}

function capturePane(target) {
  try {
    const out = run(["capture-pane", "-p", "-t", target, "-S", `-${CAPTURE_LINES}`, "-E", "-"]);
    return out.replace(/\n+$/, ""); // trim trailing blank screen lines
  } catch {
    return "(pane vanished)";
  }
}

function tailLog() {
  try {
    const content = readFileSync(LOG_PATH, "utf8");
    return content.split("\n").slice(-80).join("\n").trimEnd();
  } catch {
    return `(${LOG_PATH} not found)`;
  }
}

function snapshot() {
  try {
    const panes = listPanes();
    return {
      session: SESSION,
      error: null,
      log: tailLog(),
      panes: panes.map((p) => ({ ...p, content: capturePane(p.target) })),
    };
  } catch (e) {
    return { session: SESSION, error: `tmux error: ${e.message.split("\n")[0]}`, log: tailLog(), panes: [] };
  }
}

const HTML = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>gyre loop — ${SESSION}</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: #0b0d12; color: #cdd6f4;
    font: 12px ui-monospace, Menlo, Consolas, monospace;
    display: flex; flex-direction: column; height: 100vh;
  }
  header {
    padding: 8px 14px; background: #11131a; border-bottom: 1px solid #232733;
    display: flex; gap: 16px; align-items: baseline; flex: none;
  }
  header h1 { font-size: 13px; margin: 0; color: #89b4fa; }
  header .meta { color: #6c7086; }
  #grid {
    flex: 1; overflow: auto; padding: 10px; display: grid; gap: 10px;
    grid-template-columns: repeat(auto-fill, minmax(460px, 1fr));
    grid-auto-rows: minmax(220px, auto); align-content: start;
  }
  .card {
    border: 1px solid #232733; border-radius: 6px; background: #0e1017;
    display: flex; flex-direction: column; min-height: 220px; overflow: hidden;
  }
  .card .bar {
    flex: none; padding: 5px 10px; background: #151823; border-bottom: 1px solid #232733;
    display: flex; gap: 10px; align-items: center;
  }
  .card .bar .name { color: #89b4fa; font-weight: bold; }
  .card .bar .cmd { color: #a6e3a1; }
  .card .bar .dim { color: #6c7086; }
  .card .bar .dead { color: #f38ba8; }
  .card pre {
    flex: 1; margin: 0; padding: 8px 10px; overflow: auto;
    white-space: pre; line-height: 1.35;
  }
  .card.dead pre { opacity: 0.4; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: #a6e3a1; animation: pulse 2s infinite; flex: none; }
  .card.dead .dot { background: #f38ba8; animation: none; }
  @keyframes pulse { 50% { opacity: 0.3; } }
  #err { padding: 10px 14px; color: #f38ba8; flex: none; }
</style>
</head>
<body>
<header>
  <h1>gyre loop</h1>
  <span class="meta">session: ${SESSION}</span>
  <span class="meta" id="status">connecting…</span>
</header>
<div id="err"></div>
<div id="grid"></div>
<script>
const grid = document.getElementById("grid");
const status = document.getElementById("status");
const errBox = document.getElementById("err");
const sticks = new Map(); // pane target -> stick-to-bottom

function card(p) {
  const el = document.createElement("div");
  el.className = "card" + (p.dead ? " dead" : "");
  const bar = document.createElement("div");
  bar.className = "bar";
  bar.innerHTML =
    '<span class="dot"></span>' +
    '<span class="name"></span>' +
    '<span class="cmd"></span>' +
    '<span class="dim"></span>';
  bar.querySelector(".name").textContent = p.wname;
  bar.querySelector(".cmd").textContent = p.cmd;
  bar.querySelector(".dim").textContent = \`\${p.w}x\${p.h}\`;
  if (p.dead) { const d = document.createElement("span"); d.className = "dead"; d.textContent = "dead"; bar.appendChild(d); }
  const pre = document.createElement("pre");
  el.append(bar, pre);
  return { el, pre };
}

function render(data) {
  errBox.textContent = data.error || "";
  status.textContent = data.error ? "tmux unreachable"
    : \`\${data.panes.length} pane(s), updated \${new Date().toLocaleTimeString()}\`;
  const seen = new Set();
  // Update or append cards in order; keys are window index + name.
  const keys = data.panes.map((p) => p.widx + ":" + p.wname);
  // Rebuild if the set of panes changed; otherwise update in place.
  const current = [...grid.children].map((c) => c.dataset.key);
  const changed = current.length !== keys.length || current.some((k, i) => k !== keys[i]);
  if (changed) {
    grid.innerHTML = "";
    for (const p of data.panes) {
      const c = card(p);
      c.el.dataset.key = p.widx + ":" + p.wname;
      grid.appendChild(c.el);
    }
  }
  [...grid.children].forEach((el, i) => {
    const p = data.panes[i];
    const pre = el.querySelector("pre");
    const stick = sticks.get(el.dataset.key) !== false; // default stick
    if (pre.textContent !== p.content) pre.textContent = p.content;
    if (stick) pre.scrollTop = pre.scrollHeight;
    seen.add(el.dataset.key);
  });
  // Orchestrator log card — always last.
  if (!grid.querySelector('[data-key="log"]')) {
    const c = card({ wname: "orchestrator log", cmd: "tail", dead: false, w: 0, h: 0 });
    c.el.dataset.key = "log";
    grid.appendChild(c.el);
  }
  const logPre = grid.querySelector('[data-key="log"] pre');
  const logBar = grid.querySelector('[data-key="log"] .cmd');
  logBar.textContent = "${LOG_PATH}";
  if (logPre.textContent !== data.log) logPre.textContent = data.log;
  if (sticks.get("log") !== false) logPre.scrollTop = logPre.scrollHeight;
}

// Track user scroll: stop sticking when scrolled up, resume at bottom.
grid.addEventListener("scroll", (e) => {
  const pre = e.target.closest("pre");
  if (!pre) return;
  const card = pre.closest(".card");
  const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 30;
  sticks.set(card.dataset.key, atBottom);
}, true);

async function poll() {
  try {
    const r = await fetch("/api/panes");
    render(await r.json());
  } catch {
    status.textContent = "server unreachable";
  }
}
poll();
setInterval(poll, 1500);
</script>
</body>
</html>`;

const server = createServer((req, res) => {
  if (req.url === "/api/panes") {
    const body = JSON.stringify(snapshot());
    res.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store" });
    res.end(body);
  } else if (req.url === "/") {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
    res.end(HTML);
  } else {
    res.writeHead(404);
    res.end("not found");
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`loop dashboard: http://127.0.0.1:${PORT} (session '${SESSION}')`);
});
