// HTTP server: routes, static assets, snapshot cache, SSE wiring.
// createDashboardServer({port, root, here}) -> http.Server
import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { readFile, writeFile, stat } from "node:fs/promises";
import { join, normalize, resolve, sep, extname } from "node:path";
import { fileURLToPath } from "node:url";
import { dashboardPaths, collectFleet, collectSbxWorkers, collectLocalWorkers,
         collectTaskMetadata, diskFreePct, loopAlive, wipGuarded } from "./sources.mjs";
import { collectCoverage, coverageHistory, collectCoveragePrecise } from "./coverage.mjs";
import { buildSnapshot } from "./snapshot.mjs";
import { createStreamHub } from "./streams.mjs";

const REFRESH_MS = Number(process.env.GYRE_DASH_REFRESH_MS || 2000);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

export function createDashboardServer({ port, root, here }) {
  const paths = dashboardPaths(root);
  // public/ sits next to server/ (scripts/dashboard/public) — resolve from
  // this module's own location so the entry point's `here` can't drift.
  const publicDir = join(fileURLToPath(new URL(".", import.meta.url)), "..", "public");

  const hub = createStreamHub({ sbxStatusDir: paths.sbxStatusDir });
  hub.start(1000);

  let snapshot = null;
  let refreshing = false;

  async function refreshSnapshot() {
    if (refreshing) return;
    refreshing = true;
    try {
      const [taskMeta, fleet, sbxWorkers, localWorkers] = await Promise.all([
        collectTaskMetadata(paths),
        collectFleet(paths),
        collectSbxWorkers(paths),
        collectLocalWorkers(paths),
      ]);
      snapshot = buildSnapshot({
        taskMeta, fleet, sbxWorkers, localWorkers,
        coverage: await collectCoverage(paths),
        coveragePrecise: await collectCoveragePrecise(paths),
        coverageTrend: await coverageHistory(paths),
        diskFree: diskFreePct(),
        loopAlive: await loopAlive(paths),
        wipGuarded: await wipGuarded(paths),
        orchestratorLog: "",
        activity: hub.activityRing,
      });
    } catch (e) {
      if (snapshot) snapshot = { ...snapshot, error: String(e.message || e), when: Date.now() };
      else snapshot = { error: String(e.message || e), when: Date.now(), rows: [], overview: [], incidents: [], queue: {}, health: {}, coverage: null };
    } finally {
      refreshing = false;
    }
  }

  refreshSnapshot();
  setInterval(refreshSnapshot, REFRESH_MS);

  // Static assets with ETag revalidation (no-cache = validate every time).
  async function serveStatic(req, res, rel) {
    const abs = normalize(resolve(publicDir, rel));
    if (!abs.startsWith(normalize(publicDir) + sep) && abs !== normalize(publicDir)) {
      res.writeHead(404); res.end("not found"); return;
    }
    try {
      const data = await readFile(abs);
      const etag = `"${createHash("sha1").update(data).digest("base64url")}"`;
      if (req.headers["if-none-match"] === etag) {
        res.writeHead(304); res.end(); return;
      }
      res.writeHead(200, {
        "content-type": MIME[extname(abs)] || "application/octet-stream",
        "cache-control": "no-cache",
        etag,
      });
      res.end(data);
    } catch {
      res.writeHead(404); res.end("not found");
    }
  }

  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");
    const p = url.pathname;

    if (p === "/api/snapshot") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(snapshot));
      return;
    }

    if (p === "/api/activity") {
      const task = url.searchParams.get("task") || "";
      if (!/^task-[a-z0-9-]+$/.test(task)) {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "bad task" }));
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ events: hub.activityRing.get(task) }));
      return;
    }

    if (p === "/api/raw") {
      const task = url.searchParams.get("task") || "";
      if (!/^task-[a-z0-9-]+$/.test(task)) {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "bad task" }));
        return;
      }
      const file = join(paths.sbxStatusDir, task, "stream.log");
      readFile(file, "utf8")
        .then((text) => {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ content: text.slice(-256 * 1024) }));
        })
        .catch(() => {
          res.writeHead(404, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: "no raw log" }));
        });
      return;
    }

    if (p === "/api/parallelism" && req.method === "POST") {
      let body = "";
      req.on("data", (c) => { body += c; if (body.length > 1024) req.destroy(); });
      req.on("end", () => {
        try {
          const n = Number(JSON.parse(body).value);
          if (!Number.isInteger(n) || n < 1 || n > 100) throw new Error("out of range");
          writeFile(paths.sbxParallelismFile, String(n) + "\n").then(() => {
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: true, value: n }));
          });
        } catch (e) {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: String(e.message || e) }));
        }
      });
      return;
    }

    if (p === "/api/streams") {
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
        "x-accel-buffering": "no",
      });
      const detach = hub.attach(res);
      req.on("close", detach);
      return;
    }

    if (p === "/api/file") {
      const rel = url.searchParams.get("path") || "";
      const abs = normalize(resolve(root, rel));
      if (!abs.startsWith(root + sep + "specs" + sep)) {
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
      return;
    }

    if (p === "/") return serveStatic(req, res, "index.html");
    if (!p.startsWith("/api/")) return serveStatic(req, res, p.slice(1));

    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  });

  server.refreshSnapshot = refreshSnapshot;
  return server;
}
