#!/usr/bin/env node
// Loop dashboard: watch every loop worker's live agent output in one web page.
//
// Design: PatternFly v6 design language (Red Hat Text/Mono, dark palette,
// 4px spacing grid, 16px card radius, pill badges) rendered from local CSS
// tokens — zero dependencies, Node >= 18. Layout follows Apple HIG
// (clarity/deference/depth) and Steve Krug (each card self-evidently answers
// what it is, whether it's active, and what it's doing; page-level status is
// always visible; auto-scroll loss is signposted with an actionable button).
//
// Operator situational-awareness features (review-driven):
//   - Per-task `progress` state as a color-coded badge (needs-revision=red,
//     complete=green, ...) — the authoritative state, not the log tail.
//   - Alert banner: loop/worker failure markers (`!!!` lines) promoted from
//     the logs, sticky for the dashboard's lifetime, severity-colored.
//   - Tab-title + favicon mutation (and optional beep) when the loop dies or
//     a critical alert fires — visible without focusing the tab.
//   - Coverage subhead: spec-coverage progress bar + task histogram, the
//     "are spec gaps closing?" answer, from specs/coverage/SUMMARY.md.
//   - Event timeline card: milestones (spawns, merges, phases, aborts) that
//     survive worktree cleanup — "what happened while I was away".
//   - Per-worker stall (amber) / wedge (red, max-rounds) detection + round
//     counter (r2/6) as an early warning for max-rounds exhaustion.
//   - WIP-guard chip: shows when the operator's uncommitted work is stashed
//     by the loop's wt-guard, and red if a restore ever failed.
//   - Link-outs: task / review / spec files open in a modal via a
//     path-sandboxed /api/file route (specs/ only).
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
import { readFile, readdir, stat, open, writeFile } from "node:fs/promises";
import { join, normalize, resolve, sep } from "node:path";
import { execFile, spawn } from "node:child_process";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);
const LOG_PATH = process.env.GYRE_LOOP_LOG || "/tmp/gyre-loop.log";
const WORKER_DIR = process.env.GYRE_WORKER_DIR || "worktrees/workers";
// Sandbox fleet (scripts/fleet-sandbox.sh): per-task status dirs written by
// the local driver processes + the parallelism lever file.
const SBX_STATUS_DIR = process.env.GYRE_SBX_STATUS_DIR || "/tmp/gyre-sandbox/status";
const SBX_PARALLELISM_FILE = process.env.GYRE_SBX_PARALLELISM || "/tmp/gyre-sandbox/parallelism";
const SBX_FLEET_JSON = process.env.GYRE_SBX_FLEET_JSON || "/tmp/gyre-sandbox/fleet.json";
const TAIL_LINES = Number(process.env.GYRE_DASH_TAIL || 120);
const REFRESH_MS = Number(process.env.GYRE_DASH_REFRESH_MS || 2000);
const ROOT = process.cwd();

// Parse `title:` / `spec_ref:` / `progress:` out of a task file's YAML frontmatter.
function taskMeta(frontmatter) {
  const out = {};
  const title = frontmatter.match(/^title:\s*"?(.*?)"?\s*$/m);
  if (title) out.title = title[1];
  const ref = frontmatter.match(/^spec_ref:\s*"?(.*?)"?\s*$/m);
  if (ref) out.specRef = ref[1];
  const prog = frontmatter.match(/^progress:\s*"?(.*?)"?\s*$/m);
  if (prog) out.progress = prog[1];
  const depMatch = frontmatter.match(/^depends_on:\s*\n((?:\s*-\s*.+\n?)+)/m);
  if (depMatch) out.deps = depMatch[1].split("\n").map((l) => l.replace(/^\s*-\s*/, "").trim()).filter(Boolean);
  return out;
}

// Tail a file by reading only its last `bytes` bytes (logs grow unbounded;
// reading whole files every refresh does not scale). Returns null if absent.
async function tailBytes(path, bytes) {
  let fh;
  try {
    fh = await open(path, "r");
  } catch {
    return null;
  }
  try {
    const size = (await fh.stat()).size;
    const len = Math.min(size, bytes);
    const buf = Buffer.alloc(len);
    await fh.read(buf, 0, len, size - len);
    let text = buf.toString("utf8");
    if (len < size) text = text.slice(text.indexOf("\n") + 1); // drop partial first line
    return text;
  } finally {
    await fh.close();
  }
}

async function tailFile(path, lines) {
  const text = await tailBytes(path, 128 * 1024);
  if (text == null) return null;
  return text.split("\n").slice(-lines).join("\n").trimEnd();
}

// Decode a sandbox agent event stream (omp .agent.jsonl mirror or the
// tail -F relay stream) into a readable transcript. Raw JSONL deltas are
// unreadable in a card body; this folds them into:
//   [think] ...   — folded thinking text (dimmed client-side via marker)
//   agent prose    — text_delta accumulation
//   $ cmd          — tool calls (bash etc.)
//   -> result tail — tool execution results
// Also passes through plain non-JSON lines (worker loop logs etc.).
function decodeAgentEvents(text) {
  let out = "";
  let thinkBuf = "", textBuf = "", toolBuf = "", toolName = "";
  const flushThink = () => {
    if (thinkBuf) {
      // fold whitespace, cap: thinking is context, not content
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

// Age of a file's last write (ms before `now`), or null if unreadable.
async function ageMs(path, now) {
  try {
    const s = await stat(path);
    return now - s.mtimeMs;
  } catch {
    return null;
  }
}

let lastSnapshot = { workers: [], log: "", error: null, when: 0 };

// True if the loop orchestrator process is alive. loop.sh holds
// /tmp/gyre-loop.lock (flock) and writes its PID into it. Liveness is
// kill(pid,0) AND a cmdline check — a recycled PID running something else
// must not read as "alive".
import { execFileSync } from "node:child_process";
// Disk free % for the filesystem holding the repo (same check the loop's
// disk guard uses). Null when df is unavailable — UI hides the gauge then.
function diskFreePct() {
  try {
    const out = execFileSync("df", ["-P", process.cwd()], { encoding: "utf8" });
    const line = out.split("\n")[1] || "";
    const pctUsed = Number(line.trim().split(/\s+/)[4].replace("%", ""));
    return Number.isFinite(pctUsed) ? 100 - pctUsed : null;
  } catch {
    return null;
  }
}

async function loopAlive() {
  const lockPath = process.env.GYRE_LOOP_LOCK || "/tmp/gyre-loop.lock";
  let pid;
  try {
    const text = await readFile(lockPath, "utf8");
    pid = Number(text.split("\n")[0].trim());
  } catch {
    return false; // no lock file — loop not started
  }
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    const cmd = await readFile(`/proc/${pid}/cmdline`, "utf8");
    return /loop\.sh/.test(cmd);
  } catch (e) {
    if (e.code === "ENOENT") return false; // process gone
    // /proc unreadable (permissions / non-Linux) — fall back to signal probe
    try {
      process.kill(pid, 0);
      return true;
    } catch (err) {
      return err.code === "EPERM"; // exists but owned by someone else
    }
  }
}

// --- Alerts: failure markers promoted out of the logs ----------------------
// loop.sh / worker.sh emit `!!!`-prefixed lines on failure. Alerts are
// rebuilt fresh from the current log tails each refresh, and cleared by
// a later recovery line: a rebase-unresolved alert is always followed by
// worker.sh relaunching implementation ("Rebase unresolved … aborting to
// pre-rebase base" → next line ">>> Implementation (round N)"), and merge
// aborts by a retry. Without this, a self-recovered failure showed a
// CRITICAL banner for hours.
const ALERT_MARKERS = [
  { re: /!!! WARNING: failed to restore stashed WIP/, sev: "danger", label: "WIP restore failed" },
  { re: /!!! Unresolvable conflicts in: (.+)/, sev: "danger", label: "merge conflicts" },
  { re: /Merge aborted for (task-\S+)/, sev: "danger", label: "merge aborted" },
  { re: /!!! Failed to create worktree for (task-\S+)/, sev: "danger", label: "worktree spawn failed" },
  { re: /!!! Rebase unresolved after resolver agent/, sev: "danger", label: "rebase unresolved — worker aborted to pre-rebase base" },
  { re: /!!! Unknown status: (\S+)/, sev: "warning", label: "unknown status" },
];

// An alert line is superseded (recovered) when a later line in the same
// log matches one of these.
const RECOVERY_MARKERS = [
  />>> Implementation \(round \d+/,   // rebase abort → next round relaunched
  /--- Rebasing onto main HEAD \(round \d+\)/, // retried the rebase
  /<<< Merging (task-\S+) back to main/, // merge-retry eventually merged
];

function scanAlerts(source, lines, resultMap) {
  const lineRe = /^\[([\d:]+)\] (?:\[[^\]]+\] )?(.*)$/;
  const alerts = [];
  let recovered = 0; // count of recovery lines seen
  for (const line of lines) {
    const m = line.match(lineRe);
    if (!m) continue;
    for (const r of RECOVERY_MARKERS) if (r.test(m[2])) recovered++;
    for (const a of ALERT_MARKERS) {
      if (a.re.test(m[2])) {
        alerts.push({ sev: a.sev, ts: m[1], text: (source ? `[${source}] ` : "") + m[2].replace(/^!!! /, "!!! ") });
      }
    }
  }
  // Any recovery line after an alert clears it: the loop moved on.
  for (const a of alerts) {
    if (recovered === 0) resultMap.set(a.sev + ":" + a.text, a);
  }
  // bounded, insertion-ordered; oldest dropped
  while (resultMap.size > 30) resultMap.delete(resultMap.keys().next().value);
}

// --- Events: milestone lines for the timeline ------------------------------
const EVENT_RES = [
  />>> Spawning worker: (task-\S+)/,
  /<<< Merging (task-\S+) back to main/,
  /Cleaned up worktree for (task-\S+)/,
  /Merge aborted for (task-\S+)/,
  /--- Orchestrator cycle (\d+)/,
  />>> Spec-Fidelity Auditor/,
  /<<< Auditor done/,
  />>> Project Manager/,
  /Loop exiting/,
  /=== All specs covered/,
  /Reusing branch (worker\/\S+) \(unmerged commits/,
  />>> Rebase resolver \(round \d+\) for (task-\S+)|>>> Rebase resolver \(round \d+\)/,
  /Pre-flight: verifying (task-\S+)/,
];

function extractEvents(logText) {
  const events = [];
  for (const line of (logText || "").split("\n")) {
    const m = line.match(/^\[([\d:]+)\] \[orchestrator\] (.*)$/);
    if (!m) continue;
    if (!EVENT_RES.some((re) => re.test(m[2]))) continue;
    events.push({ ts: m[1], text: m[2].replace(/^[<>-]+ /, "").replace(/^=+ /, "").replace(/[ =-]+$/, "") });
  }
  return events.slice(-200);
}

// --- Coverage: the "are gaps closing?" numbers ------------------------------
// specs/coverage/SUMMARY.md carries a **TOTAL** row; specs/tasks/*.md carry
// per-task progress frontmatter. Both are the loop's own definition of done.
async function coverageStats() {
  const out = { summary: null, tasks: {} };
  try {
    const text = await readFile(join(ROOT, "specs/coverage/SUMMARY.md"), "utf8");
    const row = text.split("\n").find((l) => /^\|\s*\*\*TOTAL\*\*/.test(l));
    if (row) {
      const cells = row.split("|").map((c) => c.replace(/[^0-9]/g, "")).filter(Boolean);
      // cells: total, n/a, not-started, assigned, implemented, verified, pct
      if (cells.length >= 7) {
        out.summary = {
          total: +cells[0], na: +cells[1], notStarted: +cells[2], assigned: +cells[3],
          implemented: +cells[4], verified: +cells[5], pct: +cells[6],
        };
      }
    }
  } catch { /* summary absent */ }
  try {
    const files = (await readdir(join(ROOT, "specs/tasks"))).filter((f) => /^task-\d+\.md$/.test(f));
    for (const f of files) {
      const fm = await readFrontmatter(join(ROOT, "specs/tasks", f));
      const m = fm.match(/^progress:\s*"?(.*?)"?\s*$/m);
      const p = m ? m[1].trim() : "unknown";
      out.tasks[p] = (out.tasks[p] || 0) + 1;
    }
  } catch { /* tasks dir absent */ }
  return out;
}

// --- Coverage history: the pct-over-time series from SUMMARY.md git history
// The TOTAL row is rewritten by every audit/promotion commit; walking the
// file's git log recovers the full trajectory (including audit demotions).
// Cached on the file's mtime — the walk spawns one `git show` per commit
// (76+ commits on a busy day) and must not run every 2s refresh.
let covHistCache = { mtimeMs: 0, points: null };
async function coverageHistory() {
  try {
    const s = await stat(join(ROOT, "specs/coverage/SUMMARY.md"));
    if (covHistCache.points && covHistCache.mtimeMs === s.mtimeMs) return covHistCache.points;
    const { stdout } = await execFileP("git",
      ["log", "--reverse", "--format=%H%x00%cI", "--", "specs/coverage/SUMMARY.md"],
      { cwd: ROOT, timeout: 10000, maxBuffer: 16 * 1024 * 1024 });
    const commits = stdout.trim().split("\n").filter(Boolean).map((l) => {
      const [hash, date] = l.split("\0");
      return { hash, date };
    });
    const points = [];
    await Promise.all(commits.map(async (c) => {
      try {
        const { stdout: body } = await execFileP("git",
          ["show", `${c.hash}:specs/coverage/SUMMARY.md`],
          { cwd: ROOT, timeout: 10000, maxBuffer: 8 * 1024 * 1024 });
        const row = body.split("\n").find((l) => /^\|\s*\*\*TOTAL\*\*/.test(l));
        if (!row) return;
        const cells = row.split("|").map((x) => x.replace(/[^0-9]/g, "")).filter(Boolean);
        if (cells.length >= 7) points.push({ t: c.date, pct: +cells[6], verified: +cells[5] });
      } catch { /* commit dropped the file; skip */ }
    }));
    points.sort((a, b) => (a.t < b.t ? -1 : a.t > b.t ? 1 : 0));
    covHistCache = { mtimeMs: s.mtimeMs, points };
    return points;
  } catch {
    return covHistCache.points || [];
  }
}

// --- WIP guard: is the operator's uncommitted work stashed right now? -------
async function wipGuarded() {
  try {
    const { stdout } = await execFileP("git", ["stash", "list"], { cwd: ROOT, timeout: 5000 });
    return stdout.split("\n").some((l) => /loop-wt-guard/.test(l));
  } catch {
    return false;
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

// Fleet status: reads the parallelism lever + fleet heartbeat. Returns
// null when no fleet is running (standalone worker-sandbox.sh runs still
// show their cards via sbxWorkers).
async function sbxFleetStatus() {
  const out = { parallelism: null, active: null, updated: null, leverPath: SBX_PARALLELISM_FILE };
  try { out.parallelism = Number((await readFile(SBX_PARALLELISM_FILE, "utf8")).trim()) || null; } catch {}
  try {
    const f = JSON.parse(await readFile(SBX_FLEET_JSON, "utf8"));
    out.active = f.active; out.updated = f.updated;
  } catch {}
  return out;
}

let refreshing = false;
async function refreshSnapshot() {
  if (refreshing) return; // never overlap refreshes
  refreshing = true;
  const prevWorkers = lastSnapshot.workers; // survive transient FS errors
  try {
    const now = Date.now();
    const names = (await readdir(WORKER_DIR)).filter((n) => /^task-/.test(n)).sort();
    const workers = await Promise.all(
      names.map(async (name) => {
        const dir = join(WORKER_DIR, name);
        const agentLog = await tailFile(join(dir, ".agent.log"), TAIL_LINES);
        const workerLog = await tailFile(join(dir, ".worker.log"), 60);
        const { title, specRef, progress } = taskMeta(await readFrontmatter(`specs/tasks/${name}.md`));
        let done = false;
        try { await stat(join(dir, ".done")); done = true; } catch { /* not done */ }
        return { name, title, specRef, progress, done, agentLog, workerLog, agentAgeMs: await ageMs(join(dir, ".agent.log"), now) };
      })
    );
    // --- Sandbox fleet workers (driver-mirrored status dirs) ---
    const sbxWorkers = [];
    // One pgrep for all driver liveness checks (was: one spawn per worker).
    const runningDrivers = await new Promise((res) => {
      const out = [];
      const p = spawn("pgrep", ["-af", "worker-sandbox.sh .*specs/tasks/task-"]);
      p.stdout.on("data", (d) => out.push(d.toString()));
      p.on("close", () => {
        const set = new Set();
        for (const line of out.join("").split("\n")) {
          const m = line.match(/specs\/tasks\/(task-[a-z0-9-]+)\.md/);
          if (m) set.add(m[1]);
        }
        res(set);
      });
      p.on("error", () => res(new Set()));
    });
    try {
      const sbxNames = (await readdir(SBX_STATUS_DIR)).filter((n) => /^task-/.test(n)).sort();
      for (const name of sbxNames) {
        const dir = join(SBX_STATUS_DIR, name);
        const st = await readFile(join(dir, "status.json"), "utf8").then(JSON.parse).catch(() => null);
        const mirror = decodeAgentEvents(await tailFile(join(dir, "agent-mirror.txt"), TAIL_LINES) || "");
        const driverLog = await tailFile(join(dir, "driver.log"), 60);
        const fm = await readFrontmatter(`specs/tasks/${name}.md`);
        const meta = taskMeta(fm);
        // Driver liveness: a live driver updates status.json / driver.log.
        const age = await ageMs(join(dir, "driver.log"), now);
        // True liveness = a worker-sandbox.sh driver process exists for this
        // task (state dirs persist after exit; file age alone can't tell
        // "running round" from "dead driver"). One pgrep for ALL tasks —
        // spawning one per worker hammered the box at 12 workers.
        const driverAlive = runningDrivers.has(name);
        sbxWorkers.push({
          name, title: meta.title, specRef: meta.specRef,
          progress: meta.progress,
          sbx: true, sbxState: st ? st.state : "unknown",
          round: st ? st.round : null, roundsTotal: st ? st.total_rounds : null,
          sandbox: st ? st.sandbox : null, extra: st ? st.extra : "",
          agentLog: mirror, workerLog: driverLog,
          agentAgeMs: age, driverAlive,
        });
      }
    } catch { /* no sandbox fleet running */ }
    const logText = await tailBytes(LOG_PATH, 256 * 1024);
    // Alerts from orchestrator + worker logs (worker markers carry task
    // names), rebuilt fresh each refresh so recovered failures clear.
    var log = logText == null ? "" : logText.split("\n").slice(-TAIL_LINES).join("\n").trimEnd();
    const tasks = [];
    try {
      const files = await readdir(join(ROOT, "specs/tasks"));
      for (const f of files.filter((n) => /^task-/.test(n) && n.endsWith(".md"))) {
        const name = f.replace(/\.md$/, "");
        const meta = taskMeta(await readFrontmatter(join(ROOT, "specs/tasks", f)));
        tasks.push({ name, title: meta.title || "", deps: meta.deps || [], progress: meta.progress || "unknown" });
      }
    } catch { /* no tasks dir */ }
    // Alerts from orchestrator + worker logs (worker markers carry task
    // names), rebuilt fresh each refresh so recovered failures clear.
    const alerts = new Map();
    scanAlerts(null, (logText || "").split("\n"), alerts);
    for (const w of workers) scanAlerts(w.name, (w.workerLog || "").split("\n"), alerts);
    lastSnapshot = {
      sbxWorkers,
      fleet: await sbxFleetStatus(),
      workers, log, logAgeMs: await ageMs(LOG_PATH, now), tasks,
      alerts: Array.from(alerts.values()),
      events: extractEvents(logText),
      coverage: await coverageStats(), coverageHistory: await coverageHistory(),
      wipGuarded: await wipGuarded(),
      error: null, when: now,
    };
  } catch (e) {
    lastSnapshot = {
      workers: prevWorkers, sbxWorkers: lastSnapshot.sbxWorkers || [],
      fleet: lastSnapshot.fleet || null,
      log: lastSnapshot.log, logAgeMs: null,
      loopAlive: await loopAlive(), diskFree: diskFreePct(), alerts: lastSnapshot.alerts || [],
      events: lastSnapshot.events || [], coverage: lastSnapshot.coverage || null,
      wipGuarded: lastSnapshot.wipGuarded || false,
      error: String(e.message || e), when: Date.now(),
    };
  } finally {
    refreshing = false;
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
<link id="favicon" rel="icon" href="">
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
    color: var(--pf-text-muted); font-size: 13px; min-width: 0; flex: 1;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .chip {
    display: inline-flex; align-items: center; gap: var(--sp-1);
    padding: 2px var(--sp-3); border-radius: var(--pf-radius-pill);
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
  }
  .covspark {
    flex: none; cursor: pointer; display: inline-flex; align-items: center;
    border: 1px solid var(--pf-border); border-radius: 6px;
    background: var(--pf-surface-2); padding: 2px 4px;
  }
  .covspark:hover { border-color: var(--pf-border-strong); }
  .covspark svg { display: block; }
  .covtext {
    color: var(--pf-text-muted); font-size: 12px; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  #filedlg .covchart-title {
    font: 600 14px var(--pf-font); color: var(--pf-text);
    padding: var(--sp-3) var(--sp-4) 0;
  }
  #filedlg .covchart-sub {
    font: 12px var(--pf-font-mono); color: var(--pf-text-muted);
    padding: 0 var(--sp-4) var(--sp-2);
    display: flex; align-items: center; gap: var(--sp-2);
    padding: 2px var(--sp-3);
    background: var(--pf-surface-2);
    border: 1px solid var(--pf-border);
    border-radius: var(--pf-radius-pill);
    font-size: 12px; color: var(--pf-text-muted);
  }
  .par-lever input[type="range"] {
    width: 110px; accent-color: var(--pf-brand); cursor: pointer;
  }
  .par-lever .par-value {
    min-width: 2ch; text-align: center;
    font-family: var(--pf-font-mono); color: var(--pf-text);
    font-weight: 600;
  }
  .par-lever.pending { opacity: 0.6; }
  .mute-btn {
    flex: none; padding: 2px var(--sp-3); border-radius: var(--pf-radius-pill);
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
    color: var(--pf-text-muted); font-size: 12px; cursor: pointer;
  }
  .mute-btn:hover { color: var(--pf-text); border-color: var(--pf-border-strong); }

  /* Coverage subhead: the "are gaps closing?" strip. */
  .subhead {
    flex: none; display: flex; align-items: center; gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-5);
    background: var(--pf-surface); border-bottom: 1px solid var(--pf-border);
  }
  .covbar {
    width: 160px; height: 6px; flex: none; border-radius: 3px;
    background: var(--pf-surface-2); border: 1px solid var(--pf-border); overflow: hidden;
  }
  .covfill {
    height: 100%; width: 0%;
    background: linear-gradient(90deg, var(--pf-info), var(--pf-success));
    transition: width 400ms ease;
  }
  .covtext {
    color: var(--pf-text-muted); font-size: 12px; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  /* Promoted alerts (role=alert). */
  #alerts { flex: none; display: flex; flex-direction: column; gap: var(--sp-2); margin: var(--sp-3) var(--sp-5) 0; }
  .alert {
    padding: var(--sp-2) var(--sp-4);
    background: var(--pf-surface); border: 1px solid var(--pf-border);
    border-left: 3px solid var(--pf-warning); border-radius: 8px;
    font: 12px/1.5 var(--pf-font-mono); color: var(--pf-text);
  }
  .alert.danger { border-left-color: var(--pf-danger); }
  .alert .sev { font-weight: 700; margin-right: var(--sp-2); }
  .alert.warning .sev { color: var(--pf-warning); }
  .alert.danger .sev { color: var(--pf-danger); }

  /* Error alert (server exception). */
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
  .card.stalled { border-color: var(--pf-warning); }
  .card.wedged { border-color: var(--pf-danger); }
  .card.wedged .dot { background: var(--pf-danger); }

  .card-head {
    flex: none;
    padding: var(--sp-3) var(--sp-4) var(--sp-2);
    background: var(--pf-surface-2); border-bottom: 1px solid var(--pf-border);
  }
  .card-head .row1 {
    display: flex; align-items: center; gap: var(--sp-2);
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
  .linkchip {
    background: none; border: none; padding: 0 2px; cursor: pointer;
    color: var(--pf-link); font: 600 11px var(--pf-font-mono);
    text-decoration: underline dotted;
  }
  .linkchip:hover { color: var(--pf-text); }
  .linkchip.name { font-size: 13px; color: var(--pf-link); }
  .roundchip {
    flex: none; color: var(--pf-text-faint); font: 11px var(--pf-font-mono);
  }
  .card-head .title {
    display: block;
    margin-top: var(--sp-1);
    color: var(--pf-text-muted); font-size: 12px; line-height: 1.4;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .badge {
    flex: none; margin-left: auto; max-width: 55%;
    padding: 2px var(--sp-3); border-radius: var(--pf-radius-pill);
    background: color-mix(in srgb, var(--pf-info) 15%, var(--pf-surface-2));
    border: 1px solid color-mix(in srgb, var(--pf-info) 35%, var(--pf-border));
    color: var(--pf-info); font-size: 11px; white-space: nowrap;
    overflow: hidden; text-overflow: ellipsis;
  }
  .badge.p-not-started {
    color: var(--pf-text-faint);
    background: color-mix(in srgb, var(--pf-text-faint) 12%, var(--pf-surface-2));
    border-color: color-mix(in srgb, var(--pf-text-faint) 30%, var(--pf-border));
  }
  .badge.p-in-progress {
    color: var(--pf-info);
    background: color-mix(in srgb, var(--pf-info) 15%, var(--pf-surface-2));
    border-color: color-mix(in srgb, var(--pf-info) 35%, var(--pf-border));
  }
  .badge.p-ready-for-review {
    color: var(--pf-warning);
    background: color-mix(in srgb, var(--pf-warning) 15%, var(--pf-surface-2));
    border-color: color-mix(in srgb, var(--pf-warning) 35%, var(--pf-border));
  }
  .badge.p-needs-revision {
    color: var(--pf-danger);
    background: color-mix(in srgb, var(--pf-danger) 15%, var(--pf-surface-2));
    border-color: color-mix(in srgb, var(--pf-danger) 35%, var(--pf-border));
  }
  .badge.p-complete {
    color: var(--pf-success);
    background: color-mix(in srgb, var(--pf-success) 15%, var(--pf-surface-2));
    border-color: color-mix(in srgb, var(--pf-success) 35%, var(--pf-border));
  }
  /* Live-driver pill: unmistakable "a sandbox agent is on this NOW" signal.
     Green, filled, pulsing — visually distinct from every state badge. */
  .badge.p-live {
    color: #fff;
    background: var(--pf-success);
    border-color: var(--pf-success);
    font-weight: 700;
    animation: pulse 2s infinite;
  }

  .card-body {
    flex: 1; margin: 0; padding: var(--sp-3) var(--sp-4);
    overflow: auto; background: var(--pf-surface);
    font: 12px/1.55 var(--pf-font-mono); color: var(--pf-text);
    white-space: pre-wrap; overflow-wrap: anywhere;
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

  /* File-view modal (link-outs). */
  dialog#filedlg {
    background: var(--pf-surface); color: var(--pf-text);
    border: 1px solid var(--pf-border); border-radius: var(--pf-radius-card);
    width: min(760px, 92vw); max-height: 82vh; padding: 0;
  }
  dialog#filedlg::backdrop { background: rgba(0,0,0,0.55); }
  .dlg-head {
    display: flex; justify-content: space-between; align-items: center;
    gap: var(--sp-3); padding: var(--sp-3) var(--sp-4);
    border-bottom: 1px solid var(--pf-border);
    font: 600 13px var(--pf-font-mono); color: var(--pf-text-muted);
  }
  .dlg-head button {
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
    color: var(--pf-text); border-radius: var(--pf-radius-pill);
    padding: 2px var(--sp-3); cursor: pointer; font-size: 12px;
  }
  #dlg-body {
    margin: 0; padding: var(--sp-3) var(--sp-4); overflow: auto; max-height: 68vh;
    font: 12px/1.5 var(--pf-font-mono); color: var(--pf-text);
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  #dlg-body.rendered { white-space: normal; overflow-wrap: anywhere; }
  #dlg-body.rendered pre { white-space: pre-wrap; }

  /* Rendered markdown inside the modal (tasks/reviews/specs are .md files). */
  .md h1, .md h2, .md h3, .md h4 { margin: var(--sp-4) 0 var(--sp-2); line-height: 1.25; }
  .md h1 { font-size: 17px; }
  .md h2 { font-size: 15px; padding-bottom: var(--sp-1); border-bottom: 1px solid var(--pf-border); }
  .md h3 { font-size: 13.5px; }
  .md h4 { font-size: 12.5px; color: var(--pf-text-muted); }
  .md p  { margin: var(--sp-2) 0; }
  .md ul, .md ol { margin: var(--sp-2) 0; padding-left: var(--sp-5); }
  .md li { margin: var(--sp-1) 0; }
  .md pre {
    margin: var(--sp-2) 0; padding: var(--sp-3); overflow-x: auto;
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
    border-radius: 8px; white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .md pre code { border: 0; background: none; padding: 0; font-size: 11px; }
  .md blockquote {
    margin: var(--sp-2) 0; padding: var(--sp-1) var(--sp-3);
    border-left: 3px solid var(--pf-border-strong); color: var(--pf-text-muted);
  }
  .md hr { border: 0; border-top: 1px solid var(--pf-border); margin: var(--sp-3) 0; }
  .md a { color: var(--pf-link); }
  .md table { border-collapse: collapse; margin: var(--sp-2) 0; font-size: 11.5px; }
  .md th, .md td { border: 1px solid var(--pf-border); padding: 2px 8px; text-align: left; }
  .md th { background: var(--pf-surface-2); }
  .fm {
    margin: 0 0 var(--sp-3); padding: var(--sp-2) var(--sp-3);
    background: var(--pf-surface-2); border: 1px dashed var(--pf-border);
    border-radius: 8px; font: 11px var(--pf-font-mono); color: var(--pf-text-muted);
  }
  .fm .fm-k { color: var(--pf-info); }

  .md th { background: var(--pf-surface-2); }
  .source-toggle {
    background: var(--pf-surface-2); border: 1px solid var(--pf-border);
    color: var(--pf-text); border-radius: var(--pf-radius-pill);
    padding: 2px var(--sp-3); cursor: pointer; font-size: 12px;
  }

  /* --- DAG view --- */
  #dag-wrap { display: flex; gap: var(--sp-3); padding: 0 var(--sp-4) var(--sp-6); min-height: 60vh; }
  #dag-wrap[hidden] { display: none; }
  #dag { flex: 1; overflow: auto; border: 1px solid var(--pf-border); border-radius: 8px; background: var(--pf-bg); }
  #dag svg { display: block; }
  .dag-node { cursor: pointer; }
  .dag-node rect { fill: var(--pf-bg); stroke-width: 1.5; }
  .dag-node.n-complete rect { stroke: var(--pf-border); }
  .dag-node.n-complete text { fill: var(--pf-text-muted); }
  .dag-node.n-not-started rect { stroke: var(--pf-border); stroke-dasharray: 3 3; }
  .dag-node.n-ready rect { stroke: var(--pf-accent); }
  .dag-node.n-needs-revision rect { stroke: var(--pf-warn, #ec7311); }
  .dag-node.n-ready-for-review rect { stroke: var(--pf-info, #2b9af3); }
  .dag-node.n-live rect { stroke: var(--pf-success, #3da539); stroke-width: 2.5; }
  .dag-node.n-live rect { filter: drop-shadow(0 0 6px rgba(61,165,57,.7)); }
  .dag-node text { font: 11px var(--pf-font-mono); fill: var(--pf-text); }
  .dag-node .dag-sub { font-size: 9px; fill: var(--pf-text-muted); }
  .dag-edge { stroke: var(--pf-border); fill: none; stroke-width: 1.2; }
  .dag-edge.e-done { stroke: var(--pf-success, #3da539); }
  #dag-side { width: 40%; min-width: 320px; display: flex; flex-direction: column; border: 1px solid var(--pf-border); border-radius: 8px; background: var(--pf-bg); }
  #dag-side[hidden] { display: none; }
  .dag-side-head { display: flex; justify-content: space-between; align-items: center; padding: var(--sp-2) var(--sp-3); border-bottom: 1px solid var(--pf-border); font: 12px var(--pf-font-mono); }
  #dag-side-log { flex: 1; margin: 0; padding: var(--sp-3); overflow: auto; font: 11px var(--pf-font-mono); white-space: pre-wrap; max-height: 65vh; }
  @media (prefers-reduced-motion: reduce) {
    .card.receiving .dot { animation: none; }
    .card { transition: none; }
    .covfill { transition: none; }
  }
</style>
</head>
<body>
<header class="masthead">
  <span class="wordmark">Gyre loop</span>
  <span class="conn-dot" id="conn"></span>
  <span class="status" id="status" role="status">Connecting…</span>
  <span class="chip info" id="wip-chip" hidden></span>
  <span class="chip" id="loop-chip" hidden></span>
  <span class="chip" id="disk-chip" hidden></span>
  <span class="chip info" id="fleet-chip" hidden></span>
  <span class="par-lever" id="par-lever" hidden title="Live lever: concurrent sandbox workers. The fleet re-reads this every cycle — raise to add workers, lower to drain.">
    <span class="par-label">agents</span>
    <input type="range" id="par-slider" min="1" max="100" step="1" value="2">
    <span class="par-value" id="par-value">2</span>
  </span>
  <button class="mute-btn" id="dag-btn" title="Toggle task DAG view">DAG</button>
  <button class="mute-btn" id="mute" title="Toggle alert sound">Muted</button>
</header>
<div class="subhead" id="subhead">
  <div class="covbar"><div class="covfill" id="cov-fill"></div></div>
  <span class="covspark" id="cov-spark" title="Spec coverage over time — click for detail" hidden></span>
  <span class="covtext" id="cov-text">coverage unknown</span>
</div>
<div id="alerts" role="alert"></div>
<div id="err" role="alert"><span class="label">Error</span><span id="err-msg"></span></div>
<div id="empty">
  <h2>No workers detected</h2>
  <p>Start the loop, then reload this page:</p>
  <p><code>bash scripts/loop.sh</code></p>
</div>
<div id="dag-wrap" hidden>
  <div id="dag-side" hidden>
    <div class="dag-side-head">
      <span id="dag-side-title">task</span>
      <button id="dag-side-close">Close</button>
    </div>
    <pre id="dag-side-log"></pre>
  </div>
  <div id="dag"></div>
</div>
<div id="grid"></div>
<dialog id="filedlg">
  <div class="dlg-head">
    <span id="dlg-title"></span>
    <span style="display:flex; gap:var(--sp-2);">
      <button class="source-toggle" id="dlg-src" hidden>Source</button>
      <button id="dlg-close">Close</button>
    </span>
  </div>
  <div id="dlg-body"></div>
</dialog>
<script>
var statusEl = document.getElementById("status");
var conn = document.getElementById("conn");
var loopChip = document.getElementById("loop-chip");
  var diskChip = document.getElementById("disk-chip");
var wipChip = document.getElementById("wip-chip");
var errBox = document.getElementById("err");
var errText = document.getElementById("err-msg");
var empty = document.getElementById("empty");
var alertsBox = document.getElementById("alerts");
var covFill = document.getElementById("cov-fill");
var covSpark = document.getElementById("cov-spark");
var covText = document.getElementById("cov-text");
var fleetChip = document.getElementById("fleet-chip");
var parLever = document.getElementById("par-lever");
var parSlider = document.getElementById("par-slider");
var parValue = document.getElementById("par-value");
var muteBtn = document.getElementById("mute");
var favicon = document.getElementById("favicon");
var dlg = document.getElementById("filedlg");
var dlgTitle = document.getElementById("dlg-title");
var dlgBody = document.getElementById("dlg-body");
var sticks = new Map(); // card key -> following (stick to bottom)
var muxEs = null;            // single multiplexed EventSource (/api/streams)
var muxBodies = {};          // task name -> card body element
var muxBufs = {};            // task name -> text buffer

var ACTIVE_MS = 15000;        // agent output fresher than this = receiving
var LOOP_QUIET_MS = 120000;   // orchestrator log older than this = quiet
var WORKER_STALL_MS = 360000; // worker agent output older than this = stalled

// --- Phase: derive from known phase lines, not the raw log tail ----------
function phaseOf(w) {
  var lines = (w.workerLog || "").split("\\n").filter(Boolean);
  for (var i = lines.length - 1; i >= 0; i--) {
    var l = lines[i].replace(/^\\[[0-9:]+\\] \\[task-[^\\]]+\\] /, "");
    var m = l.match(/^>>> (Implementation|Verifier|Process Revision|Rebase resolver)/);
    if (m) return m[1].toLowerCase();
    if (/^--- Rebasing/.test(l)) return "rebasing";
  }
  return w.done ? "awaiting merge" : "";
}

function roundOf(w) {
  var rounds = (w.workerLog || "").match(/round \\d+/g);
  if (!rounds) return 0;
  return parseInt(rounds[rounds.length - 1].slice(6), 10);
}

// --- Card factory ---------------------------------------------------------
function card(item) {
  var el = document.createElement("div");
  el.className = "card";
  el.dataset.key = item.key;
  el.tabIndex = 0;
  var head = document.createElement("div");
  head.className = "card-head";
  var row1 = document.createElement("div");
  row1.className = "row1";
  var dot = document.createElement("span");
  dot.className = "dot";
  var nameEl = document.createElement("button");
  nameEl.className = "linkchip name";
  nameEl.textContent = item.name;
  if (item.taskPath) nameEl.addEventListener("click", function () { loadFile(item.taskPath); });
  row1.append(dot, nameEl);
  (item.links || []).forEach(function (lk) {
    var b = document.createElement("button");
    b.className = "linkchip";
    b.textContent = lk.label;
    b.title = lk.path;
    b.addEventListener("click", function () { loadFile(lk.path); });
    row1.appendChild(b);
  });
  var roundEl = document.createElement("span");
  roundEl.className = "roundchip";
  var badge = document.createElement("span");
  badge.className = "badge";
  row1.append(roundEl, badge);
  var titleEl = document.createElement("span");
  titleEl.className = "title";
  head.append(row1, titleEl);
  var body = document.createElement("pre");
  body.className = "card-body";
  var foot = document.createElement("pre");
  foot.className = "card-foot";
  var jump = document.createElement("button");
  jump.className = "jump";
  jump.textContent = "Jump to latest";
  jump.addEventListener("click", function () {
    sticks.set(item.key, true);
    body.scrollTop = body.scrollHeight;
    el.classList.remove("unfollowed");
  });
  el.append(head, body, foot, jump);
  return { el: el, body: body, foot: foot, titleEl: titleEl, badge: badge, roundEl: roundEl };
}

function fmtDuration(ms) {
  var s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return s + "s";
  var m = Math.round(s / 60);
  if (m < 60) return m + "m";
  return Math.round(m / 60) + "h";
}

// --- Tab title / favicon / beep: glanceable without focusing the tab ------
var prevDanger = false;
var muted = localStorage.getItem("gyre-dash-muted") !== "0"; // default: muted
var audioCtx = null;

function faviconData(color) {
  return "data:image/svg+xml," + encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">' +
    '<circle cx="8" cy="8" r="7" fill="' + color + '"/></svg>');
}

function beep() {
  if (muted) return;
  try {
    audioCtx = audioCtx || new (window.AudioContext || window.webkitAudioContext)();
    var t = audioCtx.currentTime;
    [0, 0.25].forEach(function (d) {
      var o = audioCtx.createOscillator();
      var g = audioCtx.createGain();
      o.type = "sine"; o.frequency.value = 880;
      g.gain.setValueAtTime(0.0001, t + d);
      g.gain.exponentialRampToValueAtTime(0.2, t + d + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, t + d + 0.18);
      o.connect(g); g.connect(audioCtx.destination);
      o.start(t + d); o.stop(t + d + 0.2);
    });
  } catch (e) { /* audio unavailable */ }
}

function updateMuteBtn() {
  muteBtn.textContent = muted ? "Muted" : "Sound on";
}
muteBtn.addEventListener("click", function () {
  muted = !muted;
  localStorage.setItem("gyre-dash-muted", muted ? "1" : "0");
  updateMuteBtn();
});
updateMuteBtn();

function setDangerState(danger, reason) {
  favicon.href = faviconData(danger ? "#ff5c5c" : "#69c665");
  document.title = danger ? "!! " + reason + " — Gyre loop" : "Gyre loop dashboard";
  if (danger && !prevDanger) beep();
  prevDanger = danger;
}

// --- Minimal markdown renderer (zero deps, dashboard-scope) ----------------
// Adapted from web/src/lib/markdown.js: headers, bold/italic, code spans and
// blocks, lists, blockquotes, hr, links (protocol-whitelisted). Content is
// escaped before inline transforms; only renderer-generated tags survive.
function mdEscape(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function mdInline(line) {
  return line
    .replace(/\\x60([^\\x60]+)\\x60/g, function (_, c) { return '<code class="md-code">' + c + "</code>"; })
    .replace(/\\*\\*\\*(.+?)\\*\\*\\*/g, "<strong><em>$1</em></strong>")
    .replace(/\\*\\*(.+?)\\*\\*/g, "<strong>$1</strong>")
    .replace(/\\*(.+?)\\*/g, "<em>$1</em>")
    .replace(/\\[([^\\]]+)\\]\\(([^)]+)\\)/g, function (_, text, url) {
      var u = url.trim().toLowerCase().replace(/[\\s\\x00-\\x1f]+/g, "");
      if (/^(javascript|data|vbscript):/.test(u)) return text;
      if (u.includes(":") && !/^(https?|mailto):/.test(u)) return text;
      return '<a href="' + url + '" target="_blank" rel="noopener">' + text + "</a>";
    });
}
function renderMarkdown(md) {
  var lines = String(md || "").split("\\n");
  var out = [];
  var inCode = false, codeLines = [];
  var inList = null; // "ul" | "ol"
  function closeList() { if (inList) { out.push("</" + inList + ">"); inList = null; } }
  for (var i = 0; i < lines.length; i++) {
    var line = lines[i];
    if (line.indexOf("\\x60\\x60\\x60") === 0) {
      if (inCode) {
        out.push("<pre><code>" + mdEscape(codeLines.join("\\n")) + "</code></pre>");
        codeLines = []; inCode = false;
      } else { closeList(); inCode = true; }
      continue;
    }
    if (inCode) { codeLines.push(line); continue; }
    if (line.trim() === "") { closeList(); continue; }
    var h = line.match(/^(#{1,6})\\s+(.+)/);
    if (h) {
      closeList();
      var lv = h[1].length;
      out.push("<h" + lv + ">" + mdInline(mdEscape(h[2])) + "</h" + lv + ">");
      continue;
    }
    if (/^(-{3,}|_{3,}|\\*{3,})$/.test(line.trim())) { closeList(); out.push("<hr/>"); continue; }
    if (line.charAt(0) === ">") {
      closeList();
      out.push("<blockquote>" + mdInline(mdEscape(line.replace(/^>\\s?/, ""))) + "</blockquote>");
      continue;
    }
    var ul = line.match(/^\\s*[-*+]\\s+(.+)/);
    if (ul) {
      if (inList !== "ul") { closeList(); out.push('<ul class="md-list">'); inList = "ul"; }
      out.push("<li>" + mdInline(mdEscape(ul[1])) + "</li>");
      continue;
    }
    var ol = line.match(/^\\s*\\d+\\.\\s+(.+)/);
    if (ol) {
      if (inList !== "ol") { closeList(); out.push('<ol class="md-list">'); inList = "ol"; }
      out.push("<li>" + mdInline(mdEscape(ol[1])) + "</li>");
      continue;
    }
    closeList();
    out.push("<p>" + mdInline(mdEscape(line)) + "</p>");
  }
  if (inCode) out.push("<pre><code>" + mdEscape(codeLines.join("\\n")) + "</code></pre>");
  closeList();
  return out.join("\\n");
}

// --- File modal (path-sandboxed /api/file) --------------------------------
// Tasks, reviews, and specs are markdown: render them (with frontmatter as a
// metadata chip) instead of dumping raw source. A Source toggle shows the
// raw text for copy/paste. Everything else stays plain text.
var dlgSrcBtn = document.getElementById("dlg-src");
var dlgRendered = true;
var dlgContent = null;

function renderFileModal(path, content) {
  dlgContent = content;
  dlgTitle.textContent = path;
  dlgBody.innerHTML = "";
  dlgRendered = true;
  var isMd = path.endsWith(".md");
  dlgSrcBtn.hidden = !isMd;
  if (!isMd || content == null) {
    dlgBody.classList.remove("rendered");
    dlgBody.textContent = content == null ? "(failed to load)" : content;
    return;
  }
  dlgBody.classList.add("rendered", "md");
  var body = content.replace(/^---\\n[\\s\\S]*?\\n---\\n/, "");
  var fm = content.slice(0, content.length - body.length);
  if (fm) {
    var fmEl = document.createElement("details");
    fmEl.className = "fm";
    var sum = document.createElement("summary");
    sum.textContent = "frontmatter";
    fmEl.appendChild(sum);
    var fmPre = document.createElement("pre");
    fmPre.textContent = fm.replace(/^---\\n|\\n---\\n$/g, "");
    fmPre.style.whiteSpace = "pre-wrap";
 fmPre.style.margin = "var(--sp-2) 0 0";
    fmEl.appendChild(fmPre);
    dlgBody.appendChild(fmEl);
  }
  var mdEl = document.createElement("div");
  mdEl.innerHTML = renderMarkdown(body);
  dlgBody.appendChild(mdEl);
}

dlgSrcBtn.addEventListener("click", function () {
  dlgRendered = !dlgRendered;
  dlgSrcBtn.textContent = dlgRendered ? "Source" : "Rendered";
  var path = dlgTitle.textContent;
  if (dlgRendered) renderFileModal(path, dlgContent);
  else {
    dlgBody.classList.remove("rendered", "md");
    dlgBody.textContent = dlgContent == null ? "" : dlgContent;
  }
});

// --- Coverage-over-time sparkline + detail chart ---------------------------
// Points come from the server (git history of specs/coverage/SUMMARY.md).
// Sparkline: inline SVG polyline in the subhead; click -> detail chart in
// the existing file modal. All SVG built with createElementNS (no innerHTML
// for coordinates; data is trusted from our own git log anyway).
function covPath(points, x0, y0, w, h, ymin, ymax) {
  var span = ymax - ymin || 1;
  return points.map(function (p, i) {
    var x = x0 + (w * i) / Math.max(points.length - 1, 1);
    var y = y0 + h - ((p.pct - ymin) / span) * h;
    return x.toFixed(1) + "," + y.toFixed(1);
  }).join(" ");
}

function renderCovSpark(points) {
  if (!points || points.length < 2) { covSpark.hidden = true; return; }
  covSpark.hidden = false;
  var w = 96, h = 20, pad = 2;
  var pcts = points.map(function (p) { return p.pct; });
  var ymin = Math.min.apply(null, pcts), ymax = Math.max.apply(null, pcts);
  var ns = "http://www.w3.org/2000/svg";
  var svg = document.createElementNS(ns, "svg");
  svg.setAttribute("width", w); svg.setAttribute("height", h);
  svg.setAttribute("viewBox", "0 0 " + w + " " + h);
  var poly = document.createElementNS(ns, "polyline");
  poly.setAttribute("points", covPath(points, pad, pad, w - 2 * pad, h - 2 * pad, ymin, ymax));
  poly.setAttribute("fill", "none");
  poly.setAttribute("stroke", "var(--pf-info)");
  poly.setAttribute("stroke-width", "1.5");
  poly.setAttribute("stroke-linejoin", "round");
  var lastPt = points[points.length - 1];
  var lx = pad + ((w - 2 * pad) * (points.length - 1)) / Math.max(points.length - 1, 1);
  var ly = pad + (h - 2 * pad) - ((lastPt.pct - ymin) / (ymax - ymin || 1)) * (h - 2 * pad);
  var dot = document.createElementNS(ns, "circle");
  dot.setAttribute("cx", lx.toFixed(1)); dot.setAttribute("cy", ly.toFixed(1));
  dot.setAttribute("r", "2"); dot.setAttribute("fill", "var(--pf-success)");
  svg.append(poly, dot);
  covSpark.replaceChildren(svg);
}

function fmtCovDate(iso) {
  var d = new Date(iso);
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " + d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function renderCovChart(points) {
  if (!points || points.length < 2) return;
  var W = 720, H = 300, padL = 36, padR = 12, padT = 12, padB = 28;
  var pcts = points.map(function (p) { return p.pct; });
  var ymin = Math.floor(Math.min.apply(null, pcts)), ymax = Math.ceil(Math.max.apply(null, pcts));
  if (ymin === ymax) { ymin -= 1; ymax += 1; }
  var ns = "http://www.w3.org/2000/svg";
  var svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 " + W + " " + H);
  // y gridlines + labels
  var steps = 4;
  for (var g = 0; g <= steps; g++) {
    var v = ymin + ((ymax - ymin) * g) / steps;
    var gy = padT + (H - padT - padB) - ((v - ymin) / (ymax - ymin)) * (H - padT - padB);
    var line = document.createElementNS(ns, "line");
    line.setAttribute("x1", padL); line.setAttribute("x2", W - padR);
    line.setAttribute("y1", gy.toFixed(1)); line.setAttribute("y2", gy.toFixed(1));
    line.setAttribute("stroke", "var(--pf-border)");
    line.setAttribute("stroke-dasharray", g === 0 ? "" : "2 3");
    var lab = document.createElementNS(ns, "text");
    lab.setAttribute("x", padL - 6); lab.setAttribute("y", (gy + 3).toFixed(1));
    lab.setAttribute("text-anchor", "end");
    lab.textContent = Math.round(v) + "%";
    svg.append(line, lab);
  }
  // x labels: first, middle, last
  [0, Math.floor((points.length - 1) / 2), points.length - 1].forEach(function (i) {
    if (i < 0 || i === undefined || (i === 0 && points.length < 2)) return;
    var x = padL + ((W - padL - padR) * i) / Math.max(points.length - 1, 1);
    var lab = document.createElementNS(ns, "text");
    lab.setAttribute("x", x.toFixed(1)); lab.setAttribute("y", H - 8);
    lab.setAttribute("text-anchor", i === 0 ? "start" : i === points.length - 1 ? "end" : "middle");
    lab.textContent = fmtCovDate(points[i].t);
    svg.appendChild(lab);
  });
  // area + line
  var path = covPath(points, padL, padT, W - padL - padR, H - padT - padB, ymin, ymax);
  var area = document.createElementNS(ns, "polygon");
  var baseY = H - padB;
  area.setAttribute("points", padL + "," + baseY + " " + path + " " + (W - padR) + "," + baseY);
  area.setAttribute("fill", "color-mix(in srgb, var(--pf-info) 15%, transparent)");
  var poly = document.createElementNS(ns, "polyline");
  poly.setAttribute("points", path);
  poly.setAttribute("fill", "none");
  poly.setAttribute("stroke", "var(--pf-info)");
  poly.setAttribute("stroke-width", "2");
  poly.setAttribute("stroke-linejoin", "round");
  svg.append(area, poly);
  // hover: vertical marker + tooltip text
  var marker = document.createElementNS(ns, "line");
  marker.setAttribute("y1", padT); marker.setAttribute("y2", H - padB);
  marker.setAttribute("stroke", "var(--pf-border-strong)");
  marker.setAttribute("visibility", "hidden");
  var tip = document.createElementNS(ns, "text");
  tip.setAttribute("y", padT + 4);
  tip.setAttribute("visibility", "hidden");
  svg.append(marker, tip);
  svg.addEventListener("mousemove", function (e) {
    var rect = svg.getBoundingClientRect();
    var relX = ((e.clientX - rect.left) / rect.width) * W;
    var i = Math.round(((relX - padL) / (W - padL - padR)) * (points.length - 1));
    i = Math.max(0, Math.min(points.length - 1, i));
    var x = padL + ((W - padL - padR) * i) / Math.max(points.length - 1, 1);
    marker.setAttribute("x1", x.toFixed(1)); marker.setAttribute("x2", x.toFixed(1));
    marker.setAttribute("visibility", "visible");
    tip.textContent = fmtCovDate(points[i].t) + " \\u00b7 " + points[i].pct + "% \\u00b7 " + points[i].verified + " verified";
    tip.setAttribute("x", Math.min(x + 4, W - 200).toFixed(1));
    tip.setAttribute("visibility", "visible");
  });
  svg.addEventListener("mouseleave", function () {
    marker.setAttribute("visibility", "hidden");
    tip.setAttribute("visibility", "hidden");
  });
  // open in the existing modal, markdown styling off
  dlgContent = null;
  dlgTitle.textContent = "Spec coverage over time";
  dlgSrcBtn.hidden = true;
  dlgBody.classList.remove("rendered", "md");
  dlgBody.replaceChildren();
  var title = document.createElement("div");
  title.className = "covchart-title";
  title.textContent = points[0].pct + "% \\u2192 " + points[points.length - 1].pct + "% over " + points.length + " commits";
  var sub = document.createElement("div");
  sub.className = "covchart-sub";
  var first = points[0], last = points[points.length - 1];
  sub.textContent = last.verified - first.verified >= 0 ? "+" : "";
  sub.textContent += (last.verified - first.verified) + " sections verified \\u00b7 source: git history of specs/coverage/SUMMARY.md";
  var wrap = document.createElement("div");
  wrap.className = "covchart-wrap";
  wrap.appendChild(svg);
  dlgBody.append(title, sub, wrap);
  dlg.showModal();
}

covSpark.addEventListener("click", function () {
  fetch("/api/snapshot").then(function (r) { return r.json(); }).then(function (data) {
    renderCovChart(data.coverageHistory);
  });
});

function loadFile(path) {
  fetch("/api/file?path=" + encodeURIComponent(path))
    .then(function (r) { return r.json(); })
    .then(function (d) {
      renderFileModal(path, d.error ? null : d.content);
    })
    .catch(function () {
      renderFileModal(path, null);
      dlgSrcBtn.textContent = "Source";
      if (!dlg.open) dlg.showModal();
    });
}
document.getElementById("dlg-close").addEventListener("click", function () { dlg.close(); });

// --- Render ----------------------------------------------------------------
function render(data) {
  var hasErr = !!data.error;
  errBox.classList.toggle("show", hasErr);
  if (hasErr) errText.textContent = data.error;

  if (dagShown) renderDag(data.tasks, data.sbxWorkers);
  else if (data.tasks) renderDag.lastTasks = data.tasks; // keep for later toggle
  if (data.tasks) renderDag.lastSbx = data.sbxWorkers || [];
  var loopStopped = data.loopAlive === false;
  var items = data.workers.map(function (w) {
    var stalled = !loopStopped && w.agentAgeMs != null && w.agentAgeMs >= WORKER_STALL_MS;
    var wedged = (data.alerts || []).some(function (a) { return a.sev === "danger" && a.text.indexOf(w.name) !== -1 && /Max rounds|wedged/.test(a.text); }) || /Max rounds/.test(w.workerLog || "");
    var links = [];
    if (w.progress === "needs-revision") links.push({ label: "review", path: "specs/reviews/" + w.name + ".md" });
    if (w.specRef) links.push({ label: "spec", path: "specs/system/" + w.specRef.split(" ")[0] });
    return {
      key: w.name, name: w.name, title: w.title, specRef: w.specRef,
      taskPath: "specs/tasks/" + w.name + ".md", links: links,
      content: w.agentLog || "(no agent output yet)",
      phase: phaseOf(w), foot: (w.workerLog || "").split("\\n").filter(Boolean).pop() || "",
      receiving: w.agentAgeMs != null && w.agentAgeMs < ACTIVE_MS,
      progress: w.progress, round: roundOf(w), stalled: stalled, wedged: wedged,
    };
  });
  // Sandbox fleet workers: cards mirroring the local ones, with
  // round/sandbox info in the phase line and driver-log tail as foot.
  (data.sbxWorkers || []).forEach(function (w) {
    // A live sandbox worker outranks a stale local worktree card with the
    // same task name (old loop leftovers): drop the local one.
    var staleIdx = -1;
    items.forEach(function (i, ix) { if (i.key === w.name && !i.sandbox) staleIdx = ix; });
    if (staleIdx !== -1) items.splice(staleIdx, 1);
    var sbxLive = w.agentAgeMs != null && w.agentAgeMs < ACTIVE_MS * 3;
    var driverAlive = !!w.driverAlive;
    var phase = w.sbxState || "unknown";
    if (w.round != null) phase += " r" + w.round + "/" + (w.roundsTotal || "?");
    if (!driverAlive) phase += " \u00b7 driver exited";
    var links = [];
    if (w.progress === "needs-revision") links.push({ label: "review", path: "specs/reviews/" + w.name + ".md" });
    if (w.specRef) links.push({ label: "spec", path: "specs/system/" + w.specRef.split(" ")[0] });
    items.push({
      key: "sbx-" + w.name, name: w.name + " \u00b7 sandbox", title: w.title, specRef: w.specRef,
      taskPath: "specs/tasks/" + w.name + ".md", links: links,
      content: w.agentLog || "(no mirrored agent output yet)",
      phase: phase, foot: (w.workerLog || "").split("\\n").filter(Boolean).pop() || "",
      receiving: driverAlive || sbxLive, sandbox: true, progress: w.progress, round: w.round,
      live: true, liveTask: w.name, // SSE subscription, not snapshot polling
      driverAlive: driverAlive, // true = a sandbox agent is on this task right now
    });
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
  if (data.events && data.events.length) {
    var evLines = data.events.slice().reverse().map(function (e) {
      return e.ts + "  " + e.text;
    });
    items.push({
      key: "history", name: "history", title: "loop milestones (newest first)", specRef: "",
      content: evLines.join("\\n"), phase: data.events.length + " events",
      foot: "", receiving: false, noFollow: true,
    });
  }

  var receiving = items.filter(function (i) { return i.receiving; }).length;
  statusEl.textContent = data.workers.length + " worker" + (data.workers.length === 1 ? "" : "s") +
    " \\u00b7 " + receiving + " receiving \\u00b7 updated " +
    (data.when ? new Date(data.when).toLocaleTimeString() : "\\u2014");
  conn.classList.toggle("down", hasErr);

  // Disk chip: amber below 10% free, red below the loop's 5% soft gate.
  if (typeof data.diskFree === "number") {
    diskChip.hidden = false;
    if (data.diskFree < 5) {
      diskChip.className = "chip danger";
      diskChip.textContent = "disk " + data.diskFree + "% free";
    } else if (data.diskFree < 10) {
      diskChip.className = "chip warn";
      diskChip.textContent = "disk " + data.diskFree + "% free";
    } else {
      diskChip.hidden = true;
    }
  } else {
    diskChip.hidden = true;
  }

  // Loop chip: dead = red; alive-but-quiet = neutral, informational.
  if (loopStopped) {
    loopChip.hidden = false;
    loopChip.className = "chip danger";
    loopChip.textContent = "loop stopped";
  } else if (data.logAgeMs != null && data.logAgeMs >= LOOP_QUIET_MS) {
    loopChip.hidden = false;
    loopChip.className = "chip";
    loopChip.textContent = "quiet " + fmtDuration(data.logAgeMs) + " \\u00b7 agent mid-run";
  } else {
    loopChip.hidden = true;
  }

  // Fleet chip + live parallelism lever.
  var fleet = data.fleet;
  if (fleet && (fleet.active != null || fleet.parallelism != null)) {
    fleetChip.hidden = false;
    fleetChip.className = "chip info";
    fleetChip.textContent = "fleet " + (fleet.active != null ? fleet.active : "?") +
      " active / " + (fleet.parallelism != null ? fleet.parallelism : "?") + " lever";
    parLever.hidden = false;
    if (fleet.parallelism != null && !parSlider.matches(":active")) {
      parSlider.value = fleet.parallelism;
      parValue.textContent = fleet.parallelism;
    }
  } else if ((data.sbxWorkers || []).length) {
    // Standalone sandbox worker(s) without a fleet — show workers, no lever.
    fleetChip.hidden = true; parLever.hidden = true;
  } else {
    fleetChip.hidden = true; parLever.hidden = true;
  }

  // WIP-guard chip: is the operator's uncommitted work stashed right now?
  var wipFailed = (data.alerts || []).some(function (a) { return /failed to restore stashed WIP/.test(a.text); });
  if (wipFailed) {
    wipChip.hidden = false;
    wipChip.className = "chip danger";
    wipChip.textContent = "WIP stash NOT restored — run git stash list";
  } else if (data.wipGuarded) {
    wipChip.hidden = false;
    wipChip.className = "chip info";
    wipChip.textContent = "WIP stashed \\u00b7 will be restored";
  } else {
    wipChip.hidden = true;
  }

  // Coverage subhead.
  var cov = data.coverage && data.coverage.summary;
  if (cov) {
    covFill.style.width = cov.pct + "%";
    var actionable = cov.total - cov.na;
    var t = data.coverage.tasks || {};
    covText.textContent = cov.pct + "% spec coverage \\u00b7 " + cov.verified + "/" + actionable +
      " sections verified \\u00b7 " + cov.assigned + " assigned \\u00b7 " + cov.notStarted + " not-started" +
      " \\u00b7 tasks: " + (t["not-started"] || 0) + " open, " + (t["needs-revision"] || 0) +
      " needs-revision, " + (t["ready-for-review"] || 0) + " ready, " + (t.complete || 0) + " done";
  } else {
    covText.textContent = "coverage unknown";
  }
  renderCovSpark(data.coverageHistory);

  // Promoted alerts.
  var alerts = data.alerts || [];
  while (alertsBox.firstChild) alertsBox.removeChild(alertsBox.firstChild);
  alerts.forEach(function (a) {
    var el = document.createElement("div");
    el.className = "alert " + (a.sev === "danger" ? "danger" : "warning");
    var sev = document.createElement("span");
    sev.className = "sev";
    sev.textContent = (a.sev === "danger" ? "CRITICAL" : "WARNING") + (a.ts ? " " + a.ts : "");
    el.append(sev, document.createTextNode(a.text));
    alertsBox.appendChild(el);
  });

  // Danger state: dead loop or any critical alert -> title/favicon/beep.
  var anyDanger = loopStopped || alerts.some(function (a) { return a.sev === "danger"; });
  setDangerState(anyDanger, loopStopped ? "loop stopped" : "critical alert");

  var anyContent = data.workers.length > 0 || data.log;
  empty.classList.toggle("show", !anyContent);
  grid.style.display = anyContent ? "" : "none";

  // Incremental grid sync: add new cards, remove gone ones, keep order.
  // Never rebuild existing cards — scroll and follow state must survive.
  var want = items.map(function (i) { return i.key; });
  var haveMap = {};
  Array.prototype.forEach.call(grid.children, function (c) { haveMap[c.dataset.key] = c; });
  Object.keys(haveMap).forEach(function (k) {
    if (want.indexOf(k) === -1) { haveMap[k].remove(); sticks.delete(k); }
  });
  // Insert only when the node's position actually changes: moving a DOM
  // node — even to its current slot — resets scrollTop on its scrollable
  // children, wiping the user's scroll every 2s cycle. Walk forward,
  // compare with what's already in place, and skip no-op moves.
  var cursor = grid.firstChild;
  for (var k = 0; k < items.length; k++) {
    var item = items[k];
    var el = haveMap[item.key];
    if (!el) {
      el = card(item).el;
      if (!item.noFollow) sticks.set(item.key, true);
    }
    if (cursor === el) { cursor = el.nextSibling; continue; } // already in place
    grid.insertBefore(el, cursor);
    // cursor unchanged: el now occupies its slot
  }
  // --- ONE multiplexed SSE feed for all sandbox worker cards ---------------
  // Browsers allow only ~6 HTTP/1.1 connections per host; one EventSource
  // per card exhausted the pool and stalled the snapshot poll. Instead a
  // single /api/streams connection carries every task's tail; render()
  // just registers which card elements belong to which task.
  var wantedLive = {};
  items.forEach(function (i) { if (i.live && i.liveTask) wantedLive[i.key] = i.liveTask; });
  if (Object.keys(wantedLive).length && !muxEs) {
    muxEs = new EventSource("/api/streams");
    muxEs.onmessage = function (e) {
      var m;
      try { m = JSON.parse(e.data); } catch (err) { return; }
      if (!m || !m.t || typeof m.d !== "string") return;
      var body = muxBodies[m.t];
      if (!body) return;
      var b = muxBufs[m.t] || "";
      b += m.d;
      if (b.length > 65536) b = b.slice(-65536);
      muxBufs[m.t] = b;
      body.textContent = b;
      var key = body.closest(".card").dataset.key;
      if (sticks.get(key) !== false) body.scrollTop = body.scrollHeight;
    };
    // drop buffers for tasks whose cards are gone
    var seen = {};
    items.forEach(function (i) { if (i.liveTask) seen[i.liveTask] = 1; });
    Object.keys(muxBufs).forEach(function (t) { if (!seen[t]) delete muxBufs[t]; });
  } else if (!Object.keys(wantedLive).length && muxEs) {
    muxEs.close(); muxEs = null; muxBufs = {};
  }
  // (re)bind card bodies for the current live set
  Object.keys(wantedLive).forEach(function (k) {
    var el = haveMap[k] || grid.querySelector('[data-key="' + k + '"]');
    if (!el) return;
    var task = wantedLive[k];
    var body = el.querySelector(".card-body");
    if (!muxBodies[task] || muxBodies[task] !== body) {
      muxBodies[task] = body;
      if (muxBufs[task]) body.textContent = muxBufs[task];
    }
  });
  Object.keys(muxBodies).forEach(function (t) {
    if (!Object.values(wantedLive).includes(t)) delete muxBodies[t];
  });
  Array.prototype.forEach.call(grid.children, function (el) {
    var i = items.find(function (x) { return x.key === el.dataset.key; });
    if (!i) return;
    el.classList.toggle("receiving", !!i.receiving);
    el.classList.toggle("stalled", !!i.stalled);
    el.classList.toggle("wedged", !!i.wedged);
    var titleText = i.title ? i.title + (i.specRef ? " \\u00b7 " + i.specRef : "") : "";
    var titleEl = el.querySelector(".title");
    if (titleEl.textContent !== titleText) {
      titleEl.textContent = titleText;
      titleEl.title = titleText;
    }
    var badge = el.querySelector(".badge");
    var badgeText = i.progress || i.phase || "";
    var badgeClass = "badge" + (i.progress ? " p-" + i.progress : "");
    if (i.driverAlive) { badgeText = "\u25cf LIVE \u00b7 " + badgeText; badgeClass = "badge p-live"; }
    if (badge.textContent !== badgeText) badge.textContent = badgeText;
    if (badge.className !== badgeClass) badge.className = badgeClass;
    var roundEl = el.querySelector(".roundchip");
    var roundText = i.round ? "r" + i.round + "/6" : "";
    if (roundEl.textContent !== roundText) roundEl.textContent = roundText;
    var body = el.querySelector(".card-body");
    if (i.live && muxBodies[i.liveTask]) {
      if (body.textContent === "(no mirrored agent output yet)") body.textContent = "";
      if (muxBufs[i.liveTask] && body.textContent !== muxBufs[i.liveTask]) body.textContent = muxBufs[i.liveTask];
    } else if (body.textContent !== i.content) {
      body.textContent = i.content;
    }
    var foot = el.querySelector(".card-foot");
    if (foot.textContent !== i.foot) foot.textContent = i.foot;
    if (i.noFollow) {
      el.classList.remove("unfollowed");
      return;
    }
    var follow = sticks.get(el.dataset.key) !== false;
    if (follow && !(i.live && muxBodies[i.liveTask])) body.scrollTop = body.scrollHeight;
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

// --- Task DAG view --------------------------------------------------------
// Layered left-to-right layout: depth = longest dependency chain. Nodes
// colored by progress; a live sandbox worker glows green with its sandbox
// name + round. Click a node -> live log side panel (SSE stream).
var dagEl = document.getElementById("dag");
var dagSide = document.getElementById("dag-side");
var dagSideTitle = document.getElementById("dag-side-title");
var dagSideLog = document.getElementById("dag-side-log");
var dagShown = false;
var dagSelTask = null;
var dagEs = null;

document.getElementById("dag-btn").addEventListener("click", function () {
  dagShown = !dagShown;
  document.getElementById("dag-wrap").hidden = !dagShown;
  grid.hidden = dagShown;
  if (dagShown && renderDag.lastTasks) renderDag(renderDag.lastTasks, renderDag.lastSbx || []);
  if (!dagShown && dagEs) { dagEs.close(); dagEs = null; dagSide.hidden = true; }
});
document.getElementById("dag-side-close").addEventListener("click", function () {
  dagSide.hidden = true; dagSelTask = null;
  if (dagEs) { dagEs.close(); dagEs = null; }
});

function dagSelect(task) {
  dagSelTask = task;
  dagSide.hidden = false;
  dagSideTitle.textContent = task.name + " \u00b7 " + (task.sandbox || task.progress);
  dagSideLog.textContent = "streaming\u2026";
  if (dagEs) dagEs.close();
  dagEs = new EventSource("/api/stream?task=" + encodeURIComponent(task.name));
  var buf = "";
  var lastNotice = "";
  dagEs.onmessage = function (e) {
    try { var d = JSON.parse(e.data).d; } catch (err) { return; }
    // Server repeats a "no stream" notice each heartbeat; show it once.
    if (d.indexOf("(driver running but streams no log") !== -1 || d.indexOf("(no live stream") !== -1) {
      if (d === lastNotice) return;
      lastNotice = d;
    } else lastNotice = "";
    buf += d;
    dagSideLog.textContent = buf;
    dagSideLog.scrollTop = dagSideLog.scrollHeight;
  };
  dagEs.onerror = function () { if (!buf) dagSideLog.textContent = "(no live stream for this task \u2014 worker not running)"; };
}

function renderDag(tasks, sbxWorkers) {
  if (!tasks || !tasks.length) return;
  var byName = {};
  tasks.forEach(function (t) { byName[t.name] = t; });
  // sandbox worker by task name (live agents)
  var liveBy = {};
  (sbxWorkers || []).forEach(function (w) { liveBy[w.name] = w; });
  // depth: longest chain of deps
  var depth = {};
  function d(t) {
    if (depth[t.name] != null) return depth[t.name];
    depth[t.name] = 0; // cycle guard
    var m = 0;
    (t.deps || []).forEach(function (dn) { if (byName[dn]) m = Math.max(m, d(byName[dn]) + 1); });
    depth[t.name] = m;
    return m;
  }
  tasks.forEach(d);
  // columns by depth, rows within column
  var cols = [];
  tasks.forEach(function (t) {
    var c = depth[t.name];
    if (!cols[c]) cols[c] = [];
    cols[c].push(t);
  });
  var NW = 150, NH = 34, GX = 60, GY = 14, PAD = 20;
  var maxRows = Math.max.apply(null, cols.map(function (c) { return c.length; }));
  var W = PAD * 2 + cols.length * (NW + GX) - GX;
  var H = PAD * 2 + maxRows * (NH + GY) - GY;
  var pos = {};
  cols.forEach(function (c, ci) {
    c.sort(function (a, b) { return a.name < b.name ? -1 : 1; });
    c.forEach(function (t, ri) {
      pos[t.name] = { x: PAD + ci * (NW + GX), y: PAD + ri * (NH + GY) };
    });
  });
  var svg = '<svg width="' + W + '" height="' + H + '" xmlns="http://www.w3.org/2000/svg">';
  // edges first (under nodes)
  tasks.forEach(function (t) {
    (t.deps || []).forEach(function (dn) {
      if (!pos[dn] || !pos[t.name]) return;
      var a = pos[dn], b = pos[t.name];
      var x1 = a.x + NW, y1 = a.y + NH / 2, x2 = b.x, y2 = b.y + NH / 2;
      var mx = (x1 + x2) / 2;
      var done = byName[dn] && byName[dn].progress === "complete";
      svg += '<path class="dag-edge' + (done ? " e-done" : "") + '" d="M' + x1 + " " + y1 + " C" + mx + " " + y1 + " " + mx + " " + y2 + " " + x2 + " " + y2 + '"/>';
    });
  });
  // nodes
  tasks.forEach(function (t) {
    var p = pos[t.name];
    var live = liveBy[t.name];
    var liveNow = live && live.driverAlive;
    var cls = "dag-node n-" + (t.progress || "unknown") + (liveNow ? " n-live" : "");
    var sub = liveNow
      ? "\u25cf agent: " + (live.sandbox || "?") + " \u00b7 r" + (live.round || "?") + "/" + (live.roundsTotal || "?")
      : (t.progress || "no state");
    svg += '<g class="' + cls + '" data-task="' + t.name + '" transform="translate(' + p.x + "," + p.y + ')">' +
      "<rect width=" + NW + " height=" + NH + " rx=6></rect>" +
      '<text x="8" y="14">' + t.name + "</text>" +
      '<text class="dag-sub" x="8" y="26">' + String(sub).slice(0, 32) + "</text></g>";
  });
  svg += "</svg>";
  dagEl.innerHTML = svg;
  Array.prototype.forEach.call(dagEl.querySelectorAll(".dag-node"), function (n) {
    n.addEventListener("click", function () {
      var name = n.dataset.task;
      var t = byName[name];
      dagSelect({ name: name, sandbox: liveBy[name] ? liveBy[name].sandbox : null, progress: t.progress });
    });
  });
}

function poll() {
  fetch("/api/snapshot").then(function (r) { return r.json(); }).then(render)
    .catch(function () {
      conn.classList.add("down");
      statusEl.textContent = "server unreachable";
    });
}
// Live lever: debounce, POST, reflect server ack (or revert + flash).
var parTimer = null;
parSlider.addEventListener("input", function () {
  parValue.textContent = parSlider.value;
  parLever.classList.add("pending");
  if (parTimer) clearTimeout(parTimer);
  parTimer = setTimeout(function () {
    fetch("/api/parallelism", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ value: Number(parSlider.value) }),
    }).then(function (r) { return r.json(); }).then(function (j) {
      parLever.classList.remove("pending");
      if (!j.ok) { parValue.textContent = "err"; }
    }).catch(function () { parLever.classList.remove("pending"); });
  }, 400);
});

poll();
setInterval(poll, 2000);
</script>
</body>
</html>`;

const server = createServer((req, res) => {
  if (req.url === "/api/snapshot") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify(lastSnapshot));
  } else if (req.url === "/") {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(HTML);
  } else if (req.url === "/api/parallelism" && req.method === "POST") {
    // Live lever: write the parallelism file the fleet re-reads each cycle.
    let body = "";
    req.on("data", (c) => { body += c; if (body.length > 1024) req.destroy(); });
    req.on("end", () => {
      try {
        const n = Number(JSON.parse(body).value);
        if (!Number.isInteger(n) || n < 1 || n > 100) throw new Error("out of range");
        writeFile(SBX_PARALLELISM_FILE, String(n) + "\n").then(() => {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: true, value: n }));
        });
      } catch (e) {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: String(e.message || e) }));
      }
    });
  } else if (req.url.split("?")[0] === "/api/streams") {
    // MULTIPLEXED server-sent events: ONE connection streams every sandbox
    // worker's stream.log. Browsers cap HTTP/1.1 at 6 connections per host —
    // a card-per-EventSource design exhausted the pool and stalled the
    // snapshot poll. Message shape: data: {"t": "<task>", "d": "<text>"}.
    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "x-accel-buffering": "no",
    });
    // Per-task tail state, rebuilt when the set of tasks changes.
    let tails = new Map(); // task -> { offset, carry }
    let closed = false;
    const listTasks = async () => {
      try {
        const names = (await readdir(SBX_STATUS_DIR)).filter((n) => /^task-/.test(n)).sort();
        const next = new Set(names);
        for (const n of names) if (!tails.has(n)) tails.set(n, { offset: 0, carry: "" });
        for (const n of [...tails.keys()]) if (!next.has(n)) tails.delete(n);
      } catch { /* status dir gone */ }
    };
    const pumpTask = async (task) => {
      const file = join(SBX_STATUS_DIR, task, "stream.log");
      const t = tails.get(task);
      if (!t) return;
      try {
        const st = await stat(file);
        if (st.size > t.offset) {
          const fh = await open(file, "r");
          try {
            const len = Math.min(st.size - t.offset, 256 * 1024);
            const buf = Buffer.alloc(len);
            await fh.read(buf, 0, len, t.offset);
            t.carry += buf.toString("utf8");
            const lastNl = t.carry.lastIndexOf("\n");
            if (lastNl === -1) return;
            const chunk = t.carry.slice(0, lastNl + 1);
            t.carry = t.carry.slice(lastNl + 1);
            t.offset += Buffer.byteLength(chunk);
            res.write(`data: ${JSON.stringify({ t: task, d: decodeAgentEvents(chunk) })}\n\n`);
          } finally { await fh.close(); }
        } else if (st.size < t.offset) {
          t.offset = 0; t.carry = ""; // rotated/truncated
        }
      } catch { /* stream.log gone: driver between rounds */ }
    };
    const pump = async () => {
      if (closed) return;
      await listTasks();
      await Promise.all([...tails.keys()].map((t) => pumpTask(t)));
    };
    const iv = setInterval(() => { pump().catch(() => {}); res.write(": hb\n\n"); }, 1000);
    req.on("close", () => { closed = true; clearInterval(iv); });
  } else if (req.url.startsWith("/api/stream")) {
    // Single-task SSE (DAG side panel — at most one connection at a time).
    const rel = new URL(req.url, "http://localhost").searchParams.get("task") || "";
    if (!/^task-[a-z0-9-]+$/.test(rel)) {
      res.writeHead(400, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "bad task" }));
      return;
    }
    const file = join(SBX_STATUS_DIR, rel, "stream.log");
    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "x-accel-buffering": "no",
    });
    let offset = 0;
    let carry = ""; // partial line split across pump reads
    let closed = false;
    const pump = async () => {
      if (closed) return;
      try {
        const st = await stat(file);
        if (st.size > offset) {
          const fh = await open(file, "r");
          try {
            const len = Math.min(st.size - offset, 256 * 1024);
            const buf = Buffer.alloc(len);
            await fh.read(buf, 0, len, offset);
            carry += buf.toString("utf8");
            const lastNl = carry.lastIndexOf("\n");
            if (lastNl === -1) return; // wait for a full line
            const chunk = carry.slice(0, lastNl + 1);
            carry = carry.slice(lastNl + 1);
            res.write(`data: ${JSON.stringify({ d: decodeAgentEvents(chunk) })}\n\n`);
          } finally { await fh.close(); }
        } else if (st.size < offset) {
          offset = 0; // rotated/truncated
        }
      } catch { /* file gone: driver between rounds */ }
    };
    // Tell the browser when there is nothing to stream: silence looks like
    // a hang. Uses the snapshot's shared driver set — no per-task pgrep.
    let notified = false;
    const notifyState = async () => {
      try { await stat(file); notified = false; } catch {
        const alive = (lastSnapshot.sbxWorkers || []).some((w) => w.name === rel && w.driverAlive);
        const msg = alive
          ? "(driver running but streams no log \u2014 old code; will stream after respawn)"
          : "(no live stream for this task \u2014 no worker is running it)";
        if (!notified) { res.write(`data: ${JSON.stringify({ d: msg })}\n\n`); notified = true; }
      }
    };
    const iv = setInterval(() => { pump().catch(() => {}); notifyState().catch(() => {}); res.write(": hb\n\n"); }, 1000);
    req.on("close", () => { closed = true; clearInterval(iv); });
  } else if (req.url.startsWith("/api/file")) {
    // Read-only file access, sandboxed to specs/ (normalized, no traversal).
    const rel = new URL(req.url, "http://localhost").searchParams.get("path") || "";
    const abs = normalize(resolve(ROOT, rel));
    if (!abs.startsWith(ROOT + sep + "specs" + sep)) {
      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "not found" }));
      return;
    }
    readFile(abs, "utf8")
      .then((text) => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ content: text.slice(0, 512 * 1024) }));
      })
      .catch(() => {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "not found" }));
      });
  } else {
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`loop dashboard: http://127.0.0.1:${PORT}`);
});
