#!/usr/bin/env node
// Loop dashboard: watch every loop worker's live agent output in one web page.
//
// Design: PatternFly v6 design language (Red Hat Text/Mono, dark palette,
// 4px spacing grid, 16px card radius, pill badges) rendered from local CSS
// tokens — zero dependencies, Node >= 18. Layout follows Apple HIG
// (clarity/deference/depth: content-first cards, receding chrome, hover
// elevation) and Steve Krug (each card self-evidently answers what it is,
// whether it's active, and what it's doing; page-level status is always
// visible; auto-scroll loss is signposted with an actionable button).
//
// Serves one card per worker worktree (worktrees/workers/task-NNN), tailing
// that worker's .agent.log (formatted omp event stream) + .worker.log, plus
// one card for the orchestrator log (/tmp/gyre-loop.log).
//
// Data source is files, NOT tmux capture-pane — cheap to poll. The snapshot
// is refreshed on a server-side timer (2s) and served from cache, so a slow
// filesystem read never blocks page interaction. Read-only.
//
// Usage:
//   node scripts/loop-dashboard.mjs
//   GYRE_DASHBOARD_PORT=7690 GYRE_LOOP_LOG=/tmp/gyre-loop.log node scripts/loop-dashboard.mjs
// Then open http://127.0.0.1:7690

import { createServer } from "node:http";
import { readFile, readdir, stat } from "node:fs/promises";
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

// Age of a file's last write (ms before `now`), or null if unreadable.
// Lets the UI distinguish "receiving output right now" from "idle".
async function ageMs(path, now) {
  try {
    const s = await stat(path);
    return now - s.mtimeMs;
  } catch {
    return null;
  }
}

let lastSnapshot = { workers: [], log: "", error: null, when: 0 };

async function refreshSnapshot() {
  try {
    const now = Date.now();
    const names = (await readdir(WORKER_DIR)).filter((n) => /^task-/.test(n)).sort();
    const workers = await Promise.all(
      names.map(async (name) => {
        const dir = join(WORKER_DIR, name);
        const agentLog = await tailFile(join(dir, ".agent.log"), TAIL_LINES);
        const workerLog = await tailFile(join(dir, ".worker.log"), 60);
        const { title, specRef } = taskMeta(await readFrontmatter(`specs/tasks/${name}.md`));
        return { name, title, specRef, agentLog, workerLog, agentAgeMs: await ageMs(join(dir, ".agent.log"), now) };
      })
    );
    const log = await tailFile(LOG_PATH, TAIL_LINES);
    lastSnapshot = {
      workers, log: log ?? "", logAgeMs: await ageMs(LOG_PATH, now),
      error: null, when: now,
    };
  } catch (e) {
    lastSnapshot = { workers: [], log: lastSnapshot.log, logAgeMs: null, error: String(e.message || e), when: Date.now() };
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
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Gyre loop dashboard</title>
<style>
  /* PatternFly v6 dark palette + tokens, rendered locally (no CDN). */
  :root {
    color-scheme: dark;
    --pf-canvas: #151515;
    --pf-surface: #1b1b1b;
    --pf-surface-2: #212121;
    --pf-border: #383838;
    --pf-border-strong: #4d4d4d;
    --pf-text: #f0f0f0;
    --pf-text-muted: #a3a3a3;
    --pf-text-faint: #6a6e73;
    --pf-brand: #ee0000;
    --pf-link: #92d5ff;
    --pf-success: #69c665;
    --pf-info: #73bcf0;
    --pf-warning: #f0ab00;
    --pf-danger: #ff5c5c;
    --pf-radius-card: 16px;
    --pf-radius-pill: 999px;
    --pf-font:
      "RedHatText", "Red Hat Text", "Helvetica Neue", Helvetica, Arial, sans-serif;
    --pf-font-mono:
      "RedHatTextMono", "Red Hat Text Mono", ui-monospace, "SFMono-Regular",
      Menlo, Consolas, monospace;
    /* 4px spacing grid */
    --sp-1: 4px; --sp-2: 8px; --sp-3: 12px; --sp-4: 16px; --sp-5: 24px; --sp-6: 32px;
  }
  * { box-sizing: border-box; }
  html, body { height: 100%; }
  body {
    margin: 0; background: var(--pf-canvas); color: var(--pf-text);
    font: 14px/1.5 var(--pf-font);
    display: flex; flex-direction: column;
  }

  /* Masthead: recedes (deference) — translucent, blurs content beneath it. */
  header.masthead {
    position: sticky; top: 0; z-index: 10; flex: none;
    display: flex; align-items: center; gap: var(--sp-4);
    padding: var(--sp-3) var(--sp-5);
    background: color-mix(in srgb, var(--pf-canvas) 80%, transparent);
    backdrop-filter: blur(12px);
    border-bottom: 1px solid var(--pf-border);
  }
  .wordmark {
    display: flex; align-items: center; gap: var(--sp-2);
    font-size: 16px; font-weight: 700; letter-spacing: 0.1px;
  }
  .wordmark::before {
    content: ""; width: 10px; height: 10px; border-radius: 2px;
    background: var(--pf-brand); flex: none;
  }
  .conn-dot {
    width: 8px; height: 8px; border-radius: 50%; flex: none;
    background: var(--pf-success);
  }
  .conn-dot.down { background: var(--pf-danger); }
  .status {
    color: var(--pf-text-muted); font-size: 13px; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .chip {
    display: inline-flex; align-items: center; gap: var(--sp-1);
    padding: 2px var(--sp-3); border-radius: var(--pf-radius-pill);
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
    color: var(--pf-text-muted); font-size: 12px; white-space: nowrap;
  }
  .chip.warn {
    color: var(--pf-warning); border-color: color-mix(in srgb, var(--pf-warning) 40%, var(--pf-border));
  }

  /* Error alert (PatternFly alert anatomy: colored left border). */
  #err { display: none; margin: var(--sp-3) var(--sp-5) 0; padding: var(--sp-3) var(--sp-4);
    background: var(--pf-surface); border: 1px solid var(--pf-border);
    border-left: 3px solid var(--pf-danger); border-radius: var(--pf-radius-card);
    color: var(--pf-text); font-size: 13px; }
  #err.show { display: block; }
  #err .label { color: var(--pf-danger); font-weight: 700; margin-right: var(--sp-2); }

  /* Card grid. */
  #grid {
    flex: 1; overflow: auto; padding: var(--sp-4) var(--sp-5) var(--sp-5);
    display: grid; gap: var(--sp-4);
    grid-template-columns: repeat(auto-fill, minmax(480px, 1fr));
    grid-auto-rows: minmax(280px, auto); align-content: start;
  }
  .card {
    position: relative;
    display: flex; flex-direction: column; min-height: 280px;
    background: var(--pf-surface); border: 1px solid var(--pf-border);
    border-radius: var(--pf-radius-card); overflow: hidden;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .card:hover { border-color: var(--pf-border-strong);
    box-shadow: 0 4px 16px rgba(0,0,0,0.35); } /* depth cue */
  .card:focus-within { border-color: var(--pf-info); outline: none; }

  .card-head {
    flex: none; display: flex; align-items: center; gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    background: var(--pf-surface-2); border-bottom: 1px solid var(--pf-border);
  }
  .dot {
    width: 8px; height: 8px; border-radius: 50%; flex: none;
    background: var(--pf-text-faint);
  }
  .card.receiving .dot { background: var(--pf-success); animation: pulse 2s infinite; }
  @keyframes pulse { 50% { opacity: 0.35; } }
  .card-head .name {
    font: 700 13px var(--pf-font-mono); color: var(--pf-text); flex: none;
  }
  .card-head .title {
    flex: 1; min-width: 0; color: var(--pf-text-muted); font-size: 12px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .badge {
    flex: none; max-width: 40%;
    padding: 2px var(--sp-3); border-radius: var(--pf-radius-pill);
    background: color-mix(in srgb, var(--pf-info) 15%, var(--pf-surface-2));
    border: 1px solid color-mix(in srgb, var(--pf-info) 35%, var(--pf-border));
    color: var(--pf-info); font-size: 11px; white-space: nowrap;
    overflow: hidden; text-overflow: ellipsis;
  }

  .card-body {
    flex: 1; margin: 0; padding: var(--sp-3) var(--sp-4);
    overflow: auto; background: var(--pf-surface);
    font: 12px/1.55 var(--pf-font-mono); color: var(--pf-text);
    white-space: pre-wrap; word-break: break-all;
  }
  .card-foot {
    flex: none; margin: 0; padding: var(--sp-2) var(--sp-4);
    background: var(--pf-surface); border-top: 1px solid var(--pf-border);
    font: 11px/1.4 var(--pf-font-mono); color: var(--pf-text-faint);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    max-height: 3.2em;
  }

  /* Jump-to-latest: visible affordance when auto-follow is off (Krug). */
  .jump {
    position: absolute; right: var(--sp-3); bottom: 3.2em; display: none;
    padding: var(--sp-1) var(--sp-3); border-radius: var(--pf-radius-pill);
    background: var(--pf-info); color: #151515; border: none; cursor: pointer;
    font: 600 12px var(--pf-font);
    box-shadow: 0 2px 8px rgba(0,0,0,0.4);
  }
  .jump:hover { background: var(--pf-link); }
  .jump:focus-visible { outline: 2px solid var(--pf-link); outline-offset: 2px; }
  .card.unfollowed .jump { display: inline-block; }

  /* Empty state: say what to do next, not just that nothing is here. */
  #empty {
    display: none; flex: 1; align-items: center; justify-content: center;
    flex-direction: column; gap: var(--sp-3); text-align: center; padding: var(--sp-6);
  }
  #empty.show { display: flex; }
  #empty h2 { margin: 0; font-size: 18px; font-weight: 700; }
  #empty p { margin: 0; color: var(--pf-text-muted); font-size: 13px; max-width: 46ch; }
  #empty code {
    font: 12px var(--pf-font-mono); background: var(--pf-surface-2);
    border: 1px solid var(--pf-border); border-radius: var(--pf-radius-pill);
    padding: 2px var(--sp-3);
  }

  @media (prefers-reduced-motion: reduce) {
    .card.receiving .dot { animation: none; }
    .card { transition: none; }
  }
</style>
</head>
<body>
<header class="masthead">
  <span class="wordmark">Gyre loop</span>
  <span class="conn-dot" id="conn"></span>
  <span class="status" id="status" role="status">Connecting…</span>
  <span class="chip" id="loop-chip" hidden></span>
</header>
<div id="err" role="alert"><span class="label">Error</span><span id="err-msg"></span></div>
<div id="empty">
  <h2>No workers detected</h2>
  <p>Start the loop, then reload this page:</p>
  <p><code>bash scripts/loop.sh</code></p>
</div>
<div id="grid"></div>
<script>
var grid = document.getElementById("grid");
var statusEl = document.getElementById("status");
var conn = document.getElementById("conn");
var loopChip = document.getElementById("loop-chip");
var errBox = document.getElementById("err");
var errText = document.getElementById("err-msg");
var empty = document.getElementById("empty");
var sticks = new Map(); // card key -> following (stick to bottom)

var ACTIVE_MS = 15000;      // agent output fresher than this = receiving
var LOOP_QUIET_MS = 120000; // orchestrator log older than this = loop stopped

function phaseOf(workerLog) {
  var m = (workerLog || "").split("\\n").filter(Boolean).pop() || "";
  return m.replace(/^\\[[0-9:]+\\] \\[task-[^\\]]+\\] /, "");
}

function card(key, name) {
  var el = document.createElement("div");
  el.className = "card";
  el.dataset.key = key;
  el.tabIndex = 0;
  var head = document.createElement("div");
  head.className = "card-head";
  var dot = document.createElement("span");
  dot.className = "dot";
  var nameEl = document.createElement("span");
  nameEl.className = "name";
  nameEl.textContent = name;
  var titleEl = document.createElement("span");
  titleEl.className = "title";
  var badge = document.createElement("span");
  badge.className = "badge";
  head.append(dot, nameEl, titleEl, badge);
  var body = document.createElement("pre");
  body.className = "card-body";
  var foot = document.createElement("pre");
  foot.className = "card-foot";
  var jump = document.createElement("button");
  jump.className = "jump";
  jump.textContent = "Jump to latest";
  jump.addEventListener("click", function () {
    sticks.set(key, true);
    body.scrollTop = body.scrollHeight;
    el.classList.remove("unfollowed");
  });
  el.append(head, body, foot, jump);
  return { el: el, body: body, foot: foot, titleEl: titleEl, badge: badge };
}

function fmtDuration(ms) {
  var s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return s + "s";
  var m = Math.round(s / 60);
  if (m < 60) return m + "m";
  return Math.round(m / 60) + "h";
}

function render(data) {
  var hasErr = !!data.error;
  errBox.classList.toggle("show", hasErr);
  if (hasErr) errText.textContent = data.error;

  var items = data.workers.map(function (w) {
    return {
      key: w.name, name: w.name, title: w.title, specRef: w.specRef,
      content: w.agentLog || "(no agent output yet)",
      phase: phaseOf(w.workerLog), foot: (w.workerLog || "").split("\\n").filter(Boolean).pop() || "",
      receiving: w.agentAgeMs != null && w.agentAgeMs < ACTIVE_MS,
    };
  });
  var logLive = data.logAgeMs != null && data.logAgeMs < LOOP_QUIET_MS;
  if (data.log || data.logAgeMs != null) {
    items.push({
      key: "log", name: "orchestrator", title: "", specRef: "",
      content: data.log || "(orchestrator log is empty)",
      phase: logLive ? "running" : "quiet " + fmtDuration(data.logAgeMs),
      foot: (data.log || "").split("\\n").filter(Boolean).pop() || "",
      receiving: logLive,
    });
  }

  var receiving = items.filter(function (i) { return i.receiving; }).length;
  statusEl.textContent = data.workers.length + " worker" + (data.workers.length === 1 ? "" : "s") +
    " \\u00b7 " + receiving + " receiving \\u00b7 updated " +
    (data.when ? new Date(data.when).toLocaleTimeString() : "\\u2014");
  conn.classList.toggle("down", hasErr);

  var loopStopped = data.logAgeMs != null && data.logAgeMs >= LOOP_QUIET_MS;
  loopChip.hidden = !loopStopped;
  if (loopStopped) {
    loopChip.className = "chip warn";
    loopChip.textContent = "loop appears stopped \\u00b7 quiet " + fmtDuration(data.logAgeMs);
  }

  var anyContent = data.workers.length > 0 || data.log;
  empty.classList.toggle("show", !anyContent);
  grid.style.display = anyContent ? "" : "none";

  // Rebuild the grid only when the set of cards changes.
  var want = items.map(function (i) { return i.key; }).join(",");
  var have = Array.prototype.map.call(grid.children, function (c) { return c.dataset.key; }).join(",");
  if (want !== have) {
    grid.innerHTML = "";
    items.forEach(function (i) {
      var c = card(i.key, i.name);
      grid.appendChild(c.el);
      sticks.set(i.key, true);
    });
  }
  Array.prototype.forEach.call(grid.children, function (el) {
    var i = items.find(function (x) { return x.key === el.dataset.key; });
    if (!i) return;
    el.classList.toggle("receiving", i.receiving);
    var titleText = i.title ? i.title + (i.specRef ? " \\u00b7 " + i.specRef : "") : "";
    var titleEl = el.querySelector(".title");
    if (titleEl.textContent !== titleText) titleEl.textContent = titleText;
    var badge = el.querySelector(".badge");
    if (badge.textContent !== i.phase) badge.textContent = i.phase;
    var body = el.querySelector(".card-body");
    if (body.textContent !== i.content) body.textContent = i.content;
    var foot = el.querySelector(".card-foot");
    if (foot.textContent !== i.foot) foot.textContent = i.foot;
    var follow = sticks.get(el.dataset.key) !== false;
    el.classList.toggle("unfollowed", !follow);
    if (follow) body.scrollTop = body.scrollHeight;
  });
}

// Stop following a card when the user scrolls it away from the bottom;
// "Jump to latest" brings it back (Krug: affordance, not silence).
grid.addEventListener("scroll", function (e) {
  var body = e.target.closest(".card-body");
  if (!body) return;
  var key = body.closest(".card").dataset.key;
  var atBottom = body.scrollTop + body.clientHeight >= body.scrollHeight - 30;
  sticks.set(key, atBottom);
  body.closest(".card").classList.toggle("unfollowed", !atBottom);
}, true);

function poll() {
  fetch("/api/snapshot").then(function (r) { return r.json(); }).then(render)
    .catch(function () {
      conn.classList.add("down");
      statusEl.textContent = "server unreachable";
    });
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
