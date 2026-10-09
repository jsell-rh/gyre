// Read the controller's durable ledger through its Python CLI. Node 18 has no
// SQLite API, so this keeps the database schema owned by the controller.
import { open, readFile, readdir, rename, stat, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { join } from "node:path";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

export function controllerPaths(root, env = process.env) {
  const state = env.GYRE_PIPELINE_STATE || join(root, ".gyre-pipeline");
  return { root, state, db: join(state, "pipeline.sqlite3"), lock: join(state, "supervisor.lock"),
    slots: join(state, "slots"), attempts: join(state, "attempts"),
    command: join(root, "scripts", "dev-pipeline.py") };
}

export async function controllerAlive(paths) {
  try {
    const pid = Number((await readFile(paths.lock, "utf8")).trim());
    if (!Number.isInteger(pid) || pid <= 0) return false;
    const cmd = await readFile(`/proc/${pid}/cmdline`, "utf8");
    return cmd.includes("dev-pipeline.py") && cmd.includes("serve");
  } catch { return false; }
}

export async function collectController(paths) {
  try { await stat(paths.db); } catch { return { present: false, online: false }; }
  try {
    const [{ stdout }, online, dbStat] = await Promise.all([
      execFileP("python3", [paths.command, "--state", paths.state, "status", "--json"],
        { cwd: paths.root, timeout: 5000, maxBuffer: 4 * 1024 * 1024 }),
      controllerAlive(paths), stat(paths.db),
    ]);
    const ledger = JSON.parse(stdout);
    return { present: true, online, updated: dbStat.mtimeMs, ...ledger };
  } catch (error) {
    return { present: true, online: await controllerAlive(paths), error: String(error.message || error) };
  }
}

let titlesCache = { key: "", value: {} };
export async function taskTitles(root) {
  const revision = await coverageRevision(root);
  if (revision) {
    const key = `${revision.cwd}:${revision.sha}`;
    if (titlesCache.key === key) return titlesCache.value;
    const { stdout } = await execFileP("git", ["grep", "-n", "-E", "^(title|spec_ref):", revision.ref, "--", "specs/tasks"],
      { cwd: revision.cwd, timeout: 10000, maxBuffer: 1024 * 1024 });
    const titles = {};
    for (const line of stdout.split("\n")) {
      const match = line.match(/^origin\/main:specs\/tasks\/(task-\d+)\.md:\d+:(title|spec_ref):\s*(.*)$/);
      if (!match) continue;
      const [, name, field, value] = match;
      (titles[name] ||= { title: name, specRef: "" })[field === "title" ? "title" : "specRef"] = value.replace(/^"|"$/g, "");
    }
    titlesCache = { key, value: titles };
    return titles;
  }
  const dir = join(root, "specs", "tasks");
  const files = (await readdir(dir)).filter((name) => /^task-\d+\.md$/.test(name));
  const rows = await Promise.all(files.map(async (name) => {
    const body = await readFile(join(dir, name), "utf8");
    const front = body.match(/^---\n([\s\S]*?)\n---/)?.[1] || "";
    return [name.slice(0, -3), {
      title: front.match(/^title:\s*"?(.+?)"?\s*$/m)?.[1] || name.slice(0, -3),
      specRef: front.match(/^spec_ref:\s*"?(.+?)"?\s*$/m)?.[1] || "",
    }];
  }));
  return Object.fromEntries(rows);
}

async function coverageRevision(root) {
  const source = join(process.env.GYRE_PIPELINE_STATE || join(root, ".gyre-pipeline"), "source");
  try {
    const { stdout } = await execFileP("git", ["rev-parse", "origin/main"],
      { cwd: source, timeout: 5000 });
    return { cwd: source, ref: "origin/main", sha: stdout.trim() };
  } catch { return null; }
}

function countCoverage(body, result) {
  for (const line of body.split("\n")) {
    if (!/^\| [0-9]/.test(line)) continue;
    const status = line.split("|")[4]?.trim();
    if (status === "n/a") continue;
    result.applicable++;
    if (status === "implemented") result.implemented++;
    else if (status === "verified") result.verified++;
    else if (status === "task-assigned" || status === "assigned") result.assigned++;
    else result.notStarted++;
  }
}

let metricsCache = { sha: "", value: null };
export async function coverageMetrics(root) {
  const result = { applicable: 0, implemented: 0, verified: 0, assigned: 0, notStarted: 0, pct: 0 };
  const revision = await coverageRevision(root);
  const cacheKey = revision ? `${revision.cwd}:${revision.sha}` : "";
  if (revision && metricsCache.sha === cacheKey) return metricsCache.value;
  try {
    if (revision) {
      const { stdout } = await execFileP("git", ["ls-tree", "-r", "--name-only", revision.ref, "specs/coverage/system"],
        { cwd: revision.cwd, timeout: 5000, maxBuffer: 1024 * 1024 });
      const files = stdout.split("\n").filter((name) => name.endsWith(".md"));
      const bodies = await Promise.all(files.map(async (file) => {
        const { stdout: body } = await execFileP("git", ["show", `${revision.ref}:${file}`],
          { cwd: revision.cwd, timeout: 10000, maxBuffer: 1024 * 1024 });
        return body;
      }));
      for (const body of bodies) countCoverage(body, result);
    } else {
      const dir = join(root, "specs", "coverage", "system");
      const files = (await readdir(dir)).filter((name) => name.endsWith(".md"));
      for (const file of files) {
        countCoverage(await readFile(join(dir, file), "utf8"), result);
      }
    }
  } catch { return result; }
  result.pct = result.applicable ? Math.round(10000 * (result.implemented + result.verified) / result.applicable) / 100 : 0;
  if (revision) metricsCache = { sha: cacheKey, value: result };
  return result;
}

let historyCache = { key: "", points: [] };
export async function coverageHistory(root, current) {
  const revision = await coverageRevision(root);
  const cwd = revision?.cwd || root;
  const ref = revision?.ref || "HEAD";
  let key;
  try { key = revision ? `${cwd}:${revision.sha}` : `${root}:${(await stat(join(root, "specs", "coverage", "SUMMARY.md"))).mtimeMs}`; }
  catch { return []; }
  if (key !== historyCache.key) {
    const { stdout } = await execFileP("git", ["log", "-60", "--format=%H%x00%cI", ref, "--", "specs/coverage/SUMMARY.md"],
      { cwd, timeout: 10000, maxBuffer: 1024 * 1024 });
    const commits = stdout.trim().split("\n").filter(Boolean).map((line) => {
      const [sha, at] = line.split("\0");
      return { sha, at };
    });
    const points = await Promise.all(commits.map(async ({ sha, at }) => {
      try {
        const { stdout: body } = await execFileP("git", ["show", `${sha}:specs/coverage/SUMMARY.md`],
          { cwd, timeout: 10000, maxBuffer: 1024 * 1024 });
        const total = body.split("\n").find((line) => /^\|\s*\*\*TOTAL\*\*/.test(line));
        const cells = total?.split("|").map((cell) => cell.trim().replace(/\*/g, ""));
        const pct = Number(cells?.[8]?.replace("%", ""));
        return Number.isFinite(pct) ? { at, pct } : null;
      } catch { return null; }
    }));
    historyCache = { key, points: points.filter(Boolean).reverse() };
  }
  const points = historyCache.points.slice();
  if (points.length && current?.pct != null) points.push({ at: new Date().toISOString(), pct: current.pct, current: true });
  return points;
}

export function associatePullRequests(rows) {
  const grouped = {};
  for (const pr of rows) {
    if (!Number.isInteger(pr.number) || !/^https:\/\/github\.com\/[^/]+\/[^/]+\/pull\/\d+$/.test(pr.url || "")) continue;
    const names = new Set();
    for (const source of [pr.headRefName || "", pr.title || ""]) {
      for (const match of source.matchAll(/\btask[-_ ](\d{1,4})\b/gi)) names.add(`task-${match[1].padStart(3, "0")}`);
    }
    for (const match of (pr.body || "").matchAll(/(?:^|\n)\s*(?:closes?|fixes?|task)\s*:?\s*task[-_ ](\d{1,4})\b/gim))
      names.add(`task-${match[1].padStart(3, "0")}`);
    for (const name of names) (grouped[name] ||= []).push({ number: pr.number, title: pr.title, url: pr.url,
      state: pr.state, updatedAt: pr.updatedAt });
  }
  return grouped;
}

let prsCache = { expires: 0, value: {}, error: "" };
export async function taskPullRequests(root) {
  if (Date.now() < prsCache.expires) return prsCache;
  try {
    const { stdout } = await execFileP("gh", ["pr", "list", "--state", "all", "--limit", "1000", "--json",
      "number,title,url,body,headRefName,updatedAt,state"],
    { cwd: root, timeout: 30000, maxBuffer: 16 * 1024 * 1024 });
    prsCache = { expires: Date.now() + 60000, value: associatePullRequests(JSON.parse(stdout)), error: "" };
  } catch (error) {
    prsCache = { expires: Date.now() + 60000, value: prsCache.value, error: String(error.message || error) };
  }
  return prsCache;
}

export async function setSlots(paths, value) {
  if (!Number.isInteger(value) || value < 0 || value > 1000) throw new Error("slots must be 0–1000");
  await stat(paths.db); // never create a controller state directory from the UI
  await execFileP("python3", [paths.command, "--state", paths.state, "slots", String(value)],
    { cwd: paths.root, timeout: 5000 });
  return value;
}

export async function retryTask(paths, task) {
  if (!/^task-\d+$/.test(task)) throw new Error("invalid task");
  const { stdout } = await execFileP("python3", [paths.command, "--state", paths.state, "retry", task],
    { cwd: paths.root, timeout: 5000, maxBuffer: 1024 * 1024 });
  return stdout.trim();
}

export async function retryAllTasks(paths) {
  const { stdout } = await execFileP("python3", [paths.command, "--state", paths.state, "retry-all"],
    { cwd: paths.root, timeout: 120000, maxBuffer: 1024 * 1024 });
  const match = stdout.trim().match(/retried (\d+) work items$/);
  if (!match) throw new Error("retry-all did not report a task count");
  return Number(match[1]);
}

export async function attemptLog(paths, id) {
  const file = attemptLogPath(paths, id);
  const fh = await open(file, "r");
  try {
    const size = (await fh.stat()).size;
    const len = Math.min(size, 256 * 1024);
    const buf = Buffer.alloc(len);
    await fh.read(buf, 0, len, size - len);
    return buf.toString("utf8");
  } finally { await fh.close(); }
}

export function attemptLogPath(paths, id) {
  const match = /^([a-f0-9]{32})-([1-9]\d*)$/.exec(id);
  if (!match) throw new Error("invalid attempt id");
  return join(paths.attempts, match[1], match[2], "output.log");
}
