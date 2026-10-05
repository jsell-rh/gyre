// Data collection: read fleet/status files, task frontmatter, liveness,
// disk, WIP guard, orchestrator log. Pure I/O — no snapshot shaping here.
import { readFile, readdir, stat, open } from "node:fs/promises";
import { join } from "node:path";
import { execFile, execFileSync, spawn } from "node:child_process";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

// Build the paths object from repo root + env (single source of truth).
export function dashboardPaths(root) {
  return {
    root,
    workerDir: process.env.GYRE_WORKER_DIR || join(root, "worktrees/workers"),
    sbxStatusDir: process.env.GYRE_SBX_STATUS_DIR || "/tmp/gyre-sandbox/status",
    sbxParallelismFile: process.env.GYRE_SBX_PARALLELISM || "/tmp/gyre-sandbox/parallelism",
    sbxFleetJson: process.env.GYRE_SBX_FLEET_JSON || "/tmp/gyre-sandbox/fleet.json",
    loopLog: process.env.GYRE_LOOP_LOG || "/tmp/gyre-loop.log",
    loopLock: process.env.GYRE_LOOP_LOCK || "/tmp/gyre-loop.lock",
    coverageSummary: join(root, "specs/coverage/SUMMARY.md"),
    coverageDir: join(root, "specs/coverage"),
    tasksDir: join(root, "specs/tasks"),
  };
}

// Age of a file's last write (ms before `now`), or null if unreadable.
export async function ageMs(path, now = Date.now()) {
  try {
    const s = await stat(path);
    return now - s.mtimeMs;
  } catch {
    return null;
  }
}

// Tail a file by reading only its last `bytes` bytes. Returns null if absent.
export async function tailBytes(path, bytes) {
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
    if (len < size) text = text.slice(text.indexOf("\n") + 1);
    return text;
  } finally {
    await fh.close();
  }
}

export async function tailFile(path, lines) {
  const text = await tailBytes(path, 128 * 1024);
  if (text == null) return null;
  return text.split("\n").slice(-lines).join("\n").trimEnd();
}

// Read just the YAML frontmatter of a task file ("" if absent).
export async function readFrontmatter(path) {
  try {
    const text = await readFile(path, "utf8");
    const m = text.match(/^---\n([\s\S]*?)\n---/);
    return m ? m[1] : "";
  } catch {
    return "";
  }
}

// Parse title/spec_ref/progress/depends_on out of task frontmatter.
export function taskMeta(frontmatter) {
  const out = {};
  const title = frontmatter.match(/^title:\s*"?(.*?)"?\s*$/m);
  if (title) out.title = title[1];
  const ref = frontmatter.match(/^spec_ref:\s*"?(.*?)"?\s*$/m);
  if (ref) out.specRef = ref[1];
  const prog = frontmatter.match(/^progress:\s*"?(.*?)"?\s*$/m);
  if (prog) out.progress = prog[1];
  const depInline = frontmatter.match(/^depends_on:\s*\[(.*)\]\s*$/m);
  const depMatch = frontmatter.match(/^depends_on:\s*\n((?:\s*-\s*.+\n?)+)/m);
  if (depMatch) out.deps = depMatch[1].split("\n").map((l) => l.replace(/^\s*-\s*/, "").trim()).filter(Boolean);
  else if (depInline) out.deps = depInline[1].split(",").map((s) => s.replace(/["']/g, "").trim()).filter(Boolean);
  return out;
}

// Fleet lever + heartbeat. Null fields when no fleet is running.
export async function collectFleet(paths) {
  const out = { parallelism: null, active: null, updated: null };
  try { out.parallelism = Number((await readFile(paths.sbxParallelismFile, "utf8")).trim()) || null; } catch {}
  try {
    const f = JSON.parse(await readFile(paths.sbxFleetJson, "utf8"));
    out.active = f.active; out.updated = f.updated;
  } catch {}
  return out;
}

// Driver liveness via one pgrep. Returns a Set of live task names, or null
// when the probe itself failed (pgrep missing/nonzero with no output) —
// snapshot maps that to runtime "unknown" rather than "inactive".
export function runningDrivers() {
  return new Promise((res) => {
    const out = [];
    const p = spawn("pgrep", ["-af", "worker-sandbox.sh .*specs/tasks/task-"]);
    p.stdout.on("data", (d) => out.push(d.toString()));
    p.on("close", (code) => {
      // pgrep exit 1 = no matches (a valid answer); other nonzero = failure.
      if (code !== 0 && code !== 1) { res(null); return; }
      const set = new Set();
      for (const line of out.join("").split("\n")) {
        const m = line.match(/specs\/tasks\/(task-[a-z0-9-]+)\.md/);
        if (m) set.add(m[1]);
      }
      res(set);
    });
    p.on("error", () => res(null));
  });
}

// Task metadata for every specs/tasks/*.md (title, specRef, progress, deps).
export async function collectTaskMetadata(paths) {
  const tasks = [];
  try {
    const files = (await readdir(paths.tasksDir)).filter((n) => /^task-/.test(n) && n.endsWith(".md"));
    for (const f of files.sort()) {
      const name = f.replace(/\.md$/, "");
      const meta = taskMeta(await readFrontmatter(join(paths.tasksDir, f)));
      tasks.push({ name, title: meta.title || "", specRef: meta.specRef || "", deps: meta.deps || [], progress: meta.progress || "unknown" });
    }
  } catch { /* no tasks dir */ }
  return tasks;
}

// Sandbox-fleet worker rows: status.json + decoded mirror tail + driver.log tail.
export async function collectSbxWorkers(paths, { tailLines = 120 } = {}) {
  const { decodeAgentEvents } = await import("./agent-events.mjs");
  const now = Date.now();
  const drivers = await runningDrivers();
  const taskMetaByName = new Map();
  const out = [];
  try {
    const names = (await readdir(paths.sbxStatusDir)).filter((n) => /^task-/.test(n)).sort();
    for (const name of names) {
      const dir = join(paths.sbxStatusDir, name);
      const st = await readFile(join(dir, "status.json"), "utf8").then(JSON.parse).catch(() => null);
      const mirror = decodeAgentEvents(await tailFile(join(dir, "agent-mirror.txt"), tailLines) || "");
      const driverLog = await tailFile(join(dir, "driver.log"), 60);
      let meta = taskMetaByName.get(name);
      if (!meta) {
        meta = taskMeta(await readFrontmatter(join(paths.tasksDir, `${name}.md`)));
        taskMetaByName.set(name, meta);
      }
      out.push({
        name,
        title: meta.title || "", specRef: meta.specRef || "", progress: meta.progress || "unknown",
        sbxState: st ? st.state : "unknown",
        round: st ? st.round : null, roundsTotal: st ? st.total_rounds : null,
        sandbox: st ? st.sandbox : null, extra: st ? st.extra : "",
        statusUpdated: st ? st.updated : null,
        agentLog: mirror, workerLog: driverLog || "",
        driverAgeMs: await ageMs(join(dir, "driver.log"), now),
        statusAgeMs: await ageMs(join(dir, "status.json"), now),
        streamAgeMs: await ageMs(join(dir, "stream.log"), now),
        driverAlive: drivers == null ? null : drivers.has(name),
      });
    }
  } catch { /* no sandbox fleet */ }
  return out;
}

// Legacy local-loop workers (worktrees/workers/*).
export async function collectLocalWorkers(paths, { tailLines = 120 } = {}) {
  const { decodeAgentEvents } = await import("./agent-events.mjs");
  const now = Date.now();
  const out = [];
  try {
    const names = (await readdir(paths.workerDir)).filter((n) => /^task-/.test(n)).sort();
    for (const name of names) {
      const dir = join(paths.workerDir, name);
      const agentLog = await tailFile(join(dir, ".agent.log"), tailLines);
      const workerLog = await tailFile(join(dir, ".worker.log"), 60);
      const meta = taskMeta(await readFrontmatter(join(paths.tasksDir, `${name}.md`)));
      let done = false;
      try { await stat(join(dir, ".done")); done = true; } catch { /* not done */ }
      out.push({
        name, title: meta.title || "", specRef: meta.specRef || "", progress: meta.progress || "unknown",
        done, agentLog: decodeAgentEvents(agentLog || ""), workerLog: workerLog || "",
        agentAgeMs: await ageMs(join(dir, ".agent.log"), now),
      });
    }
  } catch { /* no worker dir */ }
  return out;
}

export function diskFreePct() {
  try {
    const out = execFileSync("df", ["-P", process.cwd()], { encoding: "utf8" });
    const line = out.split("\n")[1] || "";
    const pctUsed = Number(line.trim().split(/\s+/)[4].replace("%", ""));
    return Number.isFinite(pctUsed) ? 100 - pctUsed : null;
  } catch {
    return null;
  }
}

export async function loopAlive(paths) {
  let pid;
  try {
    const text = await readFile(paths.loopLock, "utf8");
    pid = Number(text.split("\n")[0].trim());
  } catch {
    return false;
  }
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    const cmd = await readFile(`/proc/${pid}/cmdline`, "utf8");
    return /loop\.sh/.test(cmd);
  } catch (e) {
    if (e.code === "ENOENT") return false;
    try {
      process.kill(pid, 0);
      return true;
    } catch (err) {
      return err.code === "EPERM";
    }
  }
}

export async function wipGuarded(paths) {
  try {
    const { stdout } = await execFileP("git", ["stash", "list"], { cwd: paths.root, timeout: 5000 });
    return stdout.split("\n").some((l) => /loop-wt-guard/.test(l));
  } catch {
    return false;
  }
}
