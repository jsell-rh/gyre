import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { open, readFile } from "node:fs/promises";
import { dirname, extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { attemptLog, attemptLogPath, collectController, controllerPaths, coverageHistory, coverageMetrics, retryAllTasks, retryTask, setSlots, taskPullRequests, taskTitles } from "./dev-controller.mjs";

const MIME = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8", ".svg": "image/svg+xml" };
const execFileP = promisify(execFile);

async function githubRepository(root) {
  try {
    const { stdout } = await execFileP("git", ["remote", "get-url", "origin"], { cwd: root, timeout: 5000 });
    const match = stdout.trim().match(/^(?:https:\/\/github\.com\/|git@github\.com:)([\w.-]+\/[\w.-]+?)(?:\.git)?$/);
    return match ? `https://github.com/${match[1]}` : "";
  } catch { return ""; }
}

export async function followAttemptEvents(paths, id, req, res) {
  const fh = await open(attemptLogPath(paths, id), "r");
  const size = (await fh.stat()).size;
  const resume = String(req.headers["last-event-id"] || "");
  const validResume = /^\d+$/.test(resume) && Number(resume) <= size;
  let offset = validResume ? Number(resume) : Math.max(0, size - 1024 * 1024);
  let skipFirst = offset > 0 && !validResume;
  let pending = Buffer.alloc(0);
  let busy = false;
  let closed = false;
  res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache",
    "connection": "keep-alive", "x-accel-buffering": "no" });
  res.write(": connected\n\n");
  async function pump() {
    if (busy || closed) return;
    busy = true;
    try {
      const end = (await fh.stat()).size;
      if (end < offset) { offset = 0; pending = Buffer.alloc(0); }
      while (offset < end && !closed) {
        const chunk = Buffer.alloc(Math.min(65536, end - offset));
        const { bytesRead } = await fh.read(chunk, 0, chunk.length, offset);
        if (!bytesRead) break;
        const start = offset - pending.length;
        const data = Buffer.concat([pending, chunk.subarray(0, bytesRead)]);
        offset += bytesRead;
        let cursor = 0;
        for (let newline = data.indexOf(10, cursor); newline >= 0; newline = data.indexOf(10, cursor)) {
          const line = data.subarray(cursor, newline).toString("utf8");
          cursor = newline + 1;
          if (skipFirst) { skipFirst = false; continue; }
          const marker = line.indexOf("GYRE_AGENT_EVENT ");
          if (marker < 0) continue;
          const payload = line.slice(marker + "GYRE_AGENT_EVENT ".length).trim();
          try { JSON.parse(payload); } catch { continue; }
          res.write(`id: ${start + cursor}\nevent: agent\ndata: ${payload}\n\n`);
        }
        pending = data.subarray(cursor);
      }
    } catch { res.destroy(); }
    finally { busy = false; }
  }
  const timer = setInterval(pump, 250);
  const heartbeat = setInterval(() => { if (!closed) res.write(": heartbeat\n\n"); }, 15000);
  const cleanup = () => {
    if (closed) return;
    closed = true;
    clearInterval(timer);
    clearInterval(heartbeat);
    fh.close().catch(() => {});
  };
  res.on("close", cleanup);
  await pump();
}

export function createDashboardServer({ root }) {
  const paths = controllerPaths(root);
  const repository = githubRepository(root);
  const publicDir = resolve(dirname(fileURLToPath(import.meta.url)), "../public");
  let snapshot = { present: false, online: false, tasks: [], attempts: [], events: [], titles: {} };
  let refreshing = null;
  function refreshSnapshot() {
    if (refreshing) return refreshing;
    refreshing = (async () => {
      try {
        const [ledger, titles, coverage, prs, repositoryUrl] = await Promise.all([collectController(paths), taskTitles(root), coverageMetrics(root), taskPullRequests(root), repository]);
        const trend = await coverageHistory(root, coverage);
        snapshot = { ...ledger, titles, coverage: { ...coverage, trend }, prs: prs.value, prsError: prs.error, repositoryUrl, when: Date.now() };
      } catch (error) {
        snapshot = { ...snapshot, error: String(error.message || error), when: Date.now() };
      }
    })().finally(() => { refreshing = null; });
    return refreshing;
  }
  refreshSnapshot();
  const timer = setInterval(refreshSnapshot, 2000);
  timer.unref();

  const server = createServer(async (req, res) => {
    const url = new URL(req.url, "http://localhost");
    const json = (status, value) => { res.writeHead(status, { "content-type": "application/json" }); res.end(JSON.stringify(value)); };
    const body = async () => {
      if (req.headers["content-type"]?.split(";")[0] !== "application/json") throw new Error("JSON required");
      let input = "";
      for await (const chunk of req) {
        input += chunk;
        if (input.length > 1024) throw new Error("body too large");
      }
      return JSON.parse(input);
    };
    try {
      if (url.pathname === "/api/snapshot" && req.method === "GET") return json(200, snapshot);
      if (url.pathname === "/api/slots" && req.method === "POST") {
        const { value } = await body();
        await setSlots(paths, value);
        await refreshSnapshot();
        return json(200, { ok: true, value });
      }
      if (url.pathname === "/api/retry" && req.method === "POST") {
        const task = String((await body()).task || "");
        await retryTask(paths, task);
        await refreshSnapshot();
        return json(200, { ok: true, task });
      }
      if (url.pathname === "/api/retry-all" && req.method === "POST") {
        const retried = await retryAllTasks(paths);
        await refreshSnapshot();
        return json(200, { ok: true, retried });
      }
      if (url.pathname === "/api/log" && req.method === "GET") {
        return json(200, { content: await attemptLog(paths, url.searchParams.get("attempt") || "") });
      }
      if (url.pathname === "/api/attempt-stream" && req.method === "GET") {
        return await followAttemptEvents(paths, url.searchParams.get("attempt") || "", req, res);
      }
      if (url.pathname.startsWith("/api/")) return json(404, { error: "not found" });
      const rel = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
      const file = normalize(resolve(publicDir, rel));
      if (file !== publicDir && !file.startsWith(publicDir + sep)) return json(404, { error: "not found" });
      const data = await readFile(file);
      const etag = `"${createHash("sha1").update(data).digest("base64url")}"`;
      if (req.headers["if-none-match"] === etag) { res.writeHead(304); return res.end(); }
      res.writeHead(200, { "content-type": MIME[extname(file)] || "application/octet-stream",
        "cache-control": "no-cache", etag });
      res.end(data);
    } catch (error) {
      const status = error.code === "ENOENT" ? 404 : 400;
      json(status, { error: String(error.message || error) });
    }
  });
  server.refreshSnapshot = refreshSnapshot;
  return server;
}
