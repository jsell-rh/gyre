#!/usr/bin/env node
// Loop dashboard: watch every loop worker's live agent output in one web page.
//
// Serves a dark tiled dashboard. Cards:
//   - one per worker worktree (worktrees/workers/task-NNN): tails that
//     worker's .agent.log (formatted omp event stream) + .worker.log
//   - one for the orchestrator log (/tmp/gyre-loop.log)
//
// Data source is files, NOT tmux capture-pane — cheap to poll. The snapshot
// is refreshed on a server-side timer (2s) and served from cache, so a slow
// filesystem read never blocks page interaction. Read-only.
//
// Usage:
//   node scripts/loop-dashboard.mjs
//   GYRE_DASHBOARD_PORT=7690 GYRE_LOOP_LOG=/tmp/gyre-loop.log node scripts/loop-dashboard.mjs
// Then open http://127.0.0.1:7690
//
// No dependencies — Node >= 18.

import { createServer } from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);
const LOG_PATH = process.env.GYRE_LOOP_LOG || "/tmp/gyre-loop.log";
const WORKER_DIR = process.env.GYRE_WORKER_DIR || "worktrees/workers";
const TAIL_LINES = Number(process.env.GYRE_DASH_TAIL || 120);
const REFRESH_MS = Number(process.env.GYRE_DASH_REFRESH_MS || 2000);

// Parse `title:` / `spec_ref:` out of a task file's YAML frontmatter.
function taskMeta(frontmatter) {
  const out = {};
  const title = frontmatter.match(/^title:\s*"?(.*?)"?\s*$/m);
  if (title) out.title = title[1];
  const ref = frontmatter.match(/^spec_ref:\s*"?(.*?)"?\s*$/m);
  if (ref) out.specRef = ref[1];
  return out;
}

 async function tailFile(path, lines) {
   try {
     const content = await readFile(path, "utf8");
     return content.split("\n").slice(-lines).join("\n").trimEnd();
   } catch {
     return null;
   }
 }

let lastSnapshot = { workers: [], log: "", error: null, when: 0 };

async function refreshSnapshot() {
  try {
    const names = (await readdir(WORKER_DIR)).filter((n) => /^task-/.test(n)).sort();
    const workers = await Promise.all(
      names.map(async (name) => {
        const dir = join(WORKER_DIR, name);
        const agentLog = await tailFile(join(dir, ".agent.log"), TAIL_LINES);
        const workerLog = await tailFile(join(dir, ".worker.log"), 60);
        const { title, specRef } = taskMeta(await readFrontmatter(`specs/tasks/${name}.md`));
        return { name, title, specRef, agentLog, workerLog };
      })
    );
    const log = await tailFile(LOG_PATH, TAIL_LINES);
    lastSnapshot = { workers, log: log ?? "", error: null, when: Date.now() };
  } catch (e) {
    lastSnapshot = { workers: [], log: lastSnapshot.log, error: String(e.message || e), when: Date.now() };
  }
}

// Read just the YAML frontmatter (leading `--- ... ---` block) of a task file.
async function readFrontmatter(path) {
  try {
    const text = await readFile(path, "utf8");
    const m = text.match(/^---\n([\s\S]*?)\n---/);
    return m ? m[1] : "";
  } catch {
    return "";
  }
}

refreshSnapshot();
setInterval(refreshSnapshot, REFRESH_MS);

const HTML = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>gyre loop dashboard</title>
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
    grid-template-columns: repeat(auto-fill, minmax(480px, 1fr));
    grid-auto-rows: minmax(240px, auto); align-content: start;
  }
  .card {
    border: 1px solid #232733; border-radius: 6px; background: #0e1017;
    display: flex; flex-direction: column; min-height: 240px; overflow: hidden;
  }
  .card .bar {
    flex: none; padding: 5px 10px; background: #151823; border-bottom: 1px solid #233;
    display: flex; gap: 10px; align-items: center;
  }
  .card .bar .name { color: #89b4fa; font-weight: bold; }
  .card .bar .phase { color: #a6e3a1; }
  .card .bar .title { color: #cdd6f4; font-weight: normal; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card .bar .title .spec { color: #6c7086; }
  .card pre {
    flex: 1; margin: 0; padding: 8px 10px; overflow: auto;
    white-space: pre-wrap; word-break: break-all; line-height: 1.35;
  }
  .card pre.phase-log { color: #6c7086; border-top: 1px dashed #232733; padding-top: 4px; margin-top: 4px; flex: none; max-height: 60px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: #a6e3a1; animation: pulse 2s infinite; flex: none; }
  .card.idle .dot { background: #6c7086; animation: none; }
  @keyframes pulse { 50% { opacity: 0.3; } }
  #err { padding: 10px 14px; color: #f38ba8; flex: none; }
</style>
</head>
<body>
<header>
  <h1>gyre loop</h1>
  <span class="meta" id="status">connecting…</span>
</header>
<div id="err"></div>
<div id="grid"></div>
<script>
const grid = document.getElementById("grid");
const status = document.getElementById("status");
const errBox = document.getElementById("err");
const sticks = new Map(); // card key -> stick-to-bottom

function phaseOf(workerLog) {
  // Latest worker.sh phase line, e.g. ">>> Implementation (round 2, ...)".
  const m = (workerLog || "").split("\\n").filter(Boolean).pop() || "";
  return m.replace(/^\\[[0-9:]+\\] \\[task-[^\\]]+\\] /, "");
}

function card(key, name) {
  const el = document.createElement("div");
  el.className = "card";
  el.dataset.key = key;
  const bar = document.createElement("div");
  bar.innerHTML = '<span class="dot"></span><span class="name"></span><span class="title"></span><span class="phase"></span>';
  bar.querySelector(".name").textContent = name;
  const pre = document.createElement("pre");
  el.append(bar, pre);
  return { el, pre, phaseEl: bar.querySelector(".phase") };
}

function render(data) {
  errBox.textContent = data.error || "";
  status.textContent = data.error ? "error — retrying"
    : data.workers.length + " worker(s), updated " + new Date().toLocaleTimeString();
  const items = [...data.workers.map((w) => ({
    key: w.name, name: w.name, title: w.title, specRef: w.specRef,
    content: w.agentLog || "(no agent output yet)", phase: phaseOf(w.workerLog),
    idle: !w.agentLog,
  })), { key: "log", name: "orchestrator", title: "", specRef: "", content: data.log, phase: "", idle: false }];

  // Rebuild the grid only when the set of cards changes.
  const want = items.map((i) => i.key).join(",");
  const have = [...grid.children].map((c) => c.dataset.key).join(",");
  if (want !== have) {
    grid.innerHTML = "";
    for (const i of items) {
      const c = card(i.key, i.name);
      c.el.classList.toggle("idle", i.idle);
      grid.appendChild(c.el);
    }
  }
  for (const el of grid.children) {
    const i = items.find((x) => x.key === el.dataset.key);
    if (!i) continue;
    const titleEl = el.querySelector(".title");
    const titleText = i.title ? i.title + (i.specRef ? "  ·  " + i.specRef : "") : "";
    if (titleEl.textContent !== titleText) titleEl.textContent = titleText;
    const phaseEl = el.querySelector(".phase");
    if (phaseEl.textContent !== i.phase) phaseEl.textContent = i.phase;
    const pre = el.querySelector("pre");
    if (pre.textContent !== i.content) pre.textContent = i.content;
    if (sticks.get(el.dataset.key) !== false) pre.scrollTop = pre.scrollHeight;
  }
}

// Stop auto-scrolling a card when the user scrolls it up; resume at bottom.
grid.addEventListener("scroll", (e) => {
  const pre = e.target.closest("pre");
  if (!pre) return;
  const key = pre.closest(".card").dataset.key;
  sticks.set(key, pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 30);
}, true);

async function poll() {
  try {
    const r = await fetch("/api/snapshot");
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
  if (req.url === "/api/snapshot") {
    const body = JSON.stringify(lastSnapshot);
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
  console.log(`loop dashboard: http://127.0.0.1:${PORT}`);
});
