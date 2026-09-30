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
import { readFile, readdir, stat, open } from "node:fs/promises";
import { join, normalize, resolve, sep } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

const PORT = Number(process.env.GYRE_DASHBOARD_PORT || 7690);
const LOG_PATH = process.env.GYRE_LOOP_LOG || "/tmp/gyre-loop.log";
const WORKER_DIR = process.env.GYRE_WORKER_DIR || "worktrees/workers";
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
// non-sticky: rebuilt from the current log tails each refresh. A failure that
// the loop recovered from (rebase abort → relaunch, merge abort → retry)
// scrolls out of the tail and stops alerting — stale CRITICAL banners were
// showing for hours after recovery. Worker logs are rewritten per round, so
// their alerts clear as soon as the next round starts.
const ALERT_MARKERS = [
  { re: /!!! WARNING: failed to restore stashed WIP/, sev: "danger", label: "WIP restore failed" },
  { re: /!!! Unresolvable conflicts in: (.+)/, sev: "danger", label: "merge conflicts" },
  { re: /Merge aborted for (task-\S+)/, sev: "danger", label: "merge aborted" },
  { re: /!!! Failed to create worktree for (task-\S+)/, sev: "danger", label: "worktree spawn failed" },
  { re: /!!! Rebase unresolved after resolver agent/, sev: "danger", label: "rebase unresolved — worker aborted to pre-rebase base" },
  { re: /!!! Unknown status: (\S+)/, sev: "warning", label: "unknown status" },
];
function scanAlerts(source, lines, resultMap) {
  for (const line of lines) {
    const tsM = line.match(/^\[([\d:]+)\]/);
    const ts = tsM ? tsM[1] : "";
    for (const m of ALERT_MARKERS) {
      if (m.re.test(line)) {
        const text = (source ? `[${source}] ` : "") + line.replace(/^\[[\d:]+\] \[[^\]]+\] /, "");
        const key = m.sev + ":" + text;
        resultMap.set(key, { sev: m.sev, text, ts });
      }
    }
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
    const logText = await tailBytes(LOG_PATH, 256 * 1024);
    const log = logText == null ? "" : logText.split("\n").slice(-TAIL_LINES).join("\n").trimEnd();
    // Alerts from orchestrator + worker logs (worker markers carry task
    // names), rebuilt fresh each refresh so recovered failures clear.
    const alerts = new Map();
    scanAlerts(null, (logText || "").split("\n"), alerts);
    for (const w of workers) scanAlerts(w.name, (w.workerLog || "").split("\n"), alerts);
    lastSnapshot = {
      workers, log, logAgeMs: await ageMs(LOG_PATH, now),
      loopAlive: await loopAlive(),
      alerts: Array.from(alerts.values()),
      events: extractEvents(logText),
      coverage: await coverageStats(),
      wipGuarded: await wipGuarded(),
      error: null, when: now,
    };
  } catch (e) {
    lastSnapshot = {
      workers: prevWorkers, log: lastSnapshot.log, logAgeMs: null,
      loopAlive: await loopAlive(), alerts: lastSnapshot.alerts || [],
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
    color: var(--pf-text-muted); font-size: 12px; white-space: nowrap;
  }
  .chip.warn {
    color: var(--pf-warning); border-color: color-mix(in srgb, var(--pf-warning) 40%, var(--pf-border));
  }
  .chip.danger {
    color: var(--pf-danger); border-color: color-mix(in srgb, var(--pf-danger) 40%, var(--pf-border));
  }
  .chip.info {
    color: var(--pf-info); border-color: color-mix(in srgb, var(--pf-info) 40%, var(--pf-border));
  }
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
  .md code {
    font: 11px var(--pf-font-mono); background: var(--pf-surface-2);
    border: 1px solid var(--pf-border); border-radius: 4px; padding: 1px 4px;
  }
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
  <button class="mute-btn" id="mute" title="Toggle alert sound">Muted</button>
</header>
<div class="subhead" id="subhead">
  <div class="covbar"><div class="covfill" id="cov-fill"></div></div>
  <span class="covtext" id="cov-text">coverage unknown</span>
</div>
<div id="alerts" role="alert"></div>
<div id="err" role="alert"><span class="label">Error</span><span id="err-msg"></span></div>
<div id="empty">
  <h2>No workers detected</h2>
  <p>Start the loop, then reload this page:</p>
  <p><code>bash scripts/loop.sh</code></p>
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
var grid = document.getElementById("grid");
var statusEl = document.getElementById("status");
var conn = document.getElementById("conn");
var loopChip = document.getElementById("loop-chip");
var wipChip = document.getElementById("wip-chip");
var errBox = document.getElementById("err");
var errText = document.getElementById("err-msg");
var empty = document.getElementById("empty");
var alertsBox = document.getElementById("alerts");
var covFill = document.getElementById("cov-fill");
var covText = document.getElementById("cov-text");
var muteBtn = document.getElementById("mute");
var favicon = document.getElementById("favicon");
var dlg = document.getElementById("filedlg");
var dlgTitle = document.getElementById("dlg-title");
var dlgBody = document.getElementById("dlg-body");
var sticks = new Map(); // card key -> following (stick to bottom)

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

function loadFile(path) {
  fetch("/api/file?path=" + encodeURIComponent(path))
    .then(function (r) { return r.json(); })
    .then(function (d) {
      renderFileModal(path, d.error ? null : d.content);
      dlgSrcBtn.textContent = "Source";
      if (!dlg.open) dlg.showModal();
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
    if (badge.textContent !== badgeText) badge.textContent = badgeText;
    if (badge.className !== badgeClass) badge.className = badgeClass;
    var roundEl = el.querySelector(".roundchip");
    var roundText = i.round ? "r" + i.round + "/6" : "";
    if (roundEl.textContent !== roundText) roundEl.textContent = roundText;
    var body = el.querySelector(".card-body");
    if (body.textContent !== i.content) body.textContent = i.content;
    var foot = el.querySelector(".card-foot");
    if (foot.textContent !== i.foot) foot.textContent = i.foot;
    if (i.noFollow) {
      el.classList.remove("unfollowed");
      return;
    }
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
