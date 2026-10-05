// Coverage collection: SUMMARY.md TOTAL row + per-task progress histogram +
// pct-over-time trend from SUMMARY.md git history + precise recompute.
import { readFile, readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readFrontmatter } from "./sources.mjs";

const execFileP = promisify(execFile);

function parseTotalRow(row) {
  const cells = row.split("|").map((c) => c.replace(/[^0-9]/g, "")).filter(Boolean);
  // cells: total, n/a, not-started, assigned, implemented, verified, pct
  if (cells.length >= 7) {
    return {
      total: +cells[0], na: +cells[1], notStarted: +cells[2], assigned: +cells[3],
      implemented: +cells[4], verified: +cells[5], pct: +cells[6],
    };
  }
  return null;
}

export async function collectCoverage(paths) {
  const out = { summary: null, tasks: {} };
  try {
    const text = await readFile(paths.coverageSummary, "utf8");
    const row = text.split("\n").find((l) => /^\|\s*\*\*TOTAL\*\*/.test(l));
    if (row) out.summary = parseTotalRow(row);
  } catch { /* summary absent */ }
  try {
    const files = (await readdir(paths.tasksDir)).filter((f) => /^task-\d+\.md$/.test(f));
    for (const f of files) {
      const fm = await readFrontmatter(join(paths.tasksDir, f));
      const m = fm.match(/^progress:\s*"?(.*?)"?\s*$/m);
      const p = m ? m[1].trim() : "unknown";
      out.tasks[p] = (out.tasks[p] || 0) + 1;
    }
  } catch { /* tasks dir absent */ }
  return out;
}

// Trend: walk SUMMARY.md git history (one `git show` per commit). Cached on
// the file's mtime — the walk is expensive and must not run every refresh.
let histCache = { mtimeMs: 0, points: null };
export async function coverageHistory(paths) {
  try {
    const s = await stat(paths.coverageSummary);
    if (histCache.points && histCache.mtimeMs === s.mtimeMs) return histCache.points;
    const { stdout } = await execFileP("git",
      ["log", "--reverse", "--format=%H%x00%cI", "--", "specs/coverage/SUMMARY.md"],
      { cwd: paths.root, timeout: 10000, maxBuffer: 16 * 1024 * 1024 });
    const commits = stdout.trim().split("\n").filter(Boolean).map((l) => {
      const [hash, date] = l.split("\0");
      return { hash, date };
    });
    const points = [];
    await Promise.all(commits.map(async (c) => {
      try {
        const { stdout: body } = await execFileP("git",
          ["show", `${c.hash}:specs/coverage/SUMMARY.md`],
          { cwd: paths.root, timeout: 10000, maxBuffer: 8 * 1024 * 1024 });
        const row = body.split("\n").find((l) => /^\|\s*\*\*TOTAL\*\*/.test(l));
        if (!row) return;
        const cells = row.split("|").map((x) => x.replace(/[^0-9]/g, "")).filter(Boolean);
        if (cells.length >= 7) points.push({ t: c.date, pct: +cells[6], verified: +cells[5] });
      } catch { /* commit dropped the file */ }
    }));
    points.sort((a, b) => (a.t < b.t ? -1 : a.t > b.t ? 1 : 0));
    histCache = { mtimeMs: s.mtimeMs, points };
    return points;
  } catch {
    return histCache.points || [];
  }
}

// Precise recompute: scan specs/coverage/system/*.md status column exactly
// like the fleet's awk check (column 5 when splitting rows on `|`, rows
// starting with `| <digit>`). n/a rows are excluded from the denominator.
// The redesign requires the headline percent and its supporting arithmetic
// to agree — so we recompute instead of trusting SUMMARY.md's cached TOTAL.
export async function collectCoveragePrecise(paths) {
  const totals = { total: 0, na: 0, notStarted: 0, assigned: 0, implemented: 0, verified: 0 };
  try {
    const files = (await readdir(join(paths.coverageDir, "system"))).filter((f) => f.endsWith(".md"));
    for (const f of files) {
      let text;
      try { text = await readFile(join(paths.coverageDir, "system", f), "utf8"); } catch { continue; }
      for (const line of text.split("\n")) {
        if (!/^\| [0-9]/.test(line)) continue;
        const cells = line.split("|").map((c) => c.replace(/^ +| +$/g, ""));
        // awk -F"|" $5 (1-indexed) === JS cells[4] (0-indexed, leading empty)
        const status = cells[4];
        totals.total++;
        if (status === "implemented") totals.implemented++;
        else if (status === "verified") totals.verified++;
        else if (status === "n/a") { totals.na++; totals.total--; }
        else if (status === "task-assigned" || status === "assigned") totals.assigned++;
        else totals.notStarted++;
      }
    }
  } catch { /* coverage dir absent */ }
  const applicable = totals.total; // n/a already excluded
  const pct = applicable > 0 ? ((totals.implemented + totals.verified) / applicable) * 100 : 0;
  return { ...totals, applicable, pct: Math.round(pct * 100) / 100 };
}
