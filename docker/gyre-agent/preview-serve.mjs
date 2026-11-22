#!/usr/bin/env node
// preview-serve.mjs — ephemeral preview environment server (activity-dashboard.md
// §"Try it" links).
//
// Runs INSIDE a preview environment (container or local dispatch) after the
// exact merge commit has been checked out. Serves the checkout over HTTP so
// the requesting user can interact with and test what was built — without
// touching production.
//
// Env contract (injected by the provisioning server):
//   GYRE_PREVIEW_PORT     — port to listen on (default 8080)
//   GYRE_COMMIT_SHA       — the commit this environment runs (displayed)
//   GYRE_PREVIEW_ENV_ID   — environment id (displayed)
//   GYRE_WG_MESH_IP       — allocated mesh IP, when WireGuard is enabled
//
// The server is deliberately dependency-free (node stdlib only): it serves
// the working directory (the checked-out commit) as a browsable file tree.

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const PORT = Number(process.env.GYRE_PREVIEW_PORT || 8080);
const COMMIT = process.env.GYRE_COMMIT_SHA || 'unknown';
const ENV_ID = process.env.GYRE_PREVIEW_ENV_ID || 'unknown';
const MESH_IP = process.env.GYRE_WG_MESH_IP || null;
const ROOT = process.cwd();

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm',
};

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]
  );
}

function safeJoin(root, rel) {
  const resolved = path.resolve(root, '.' + path.sep + rel);
  if (!resolved.startsWith(root + path.sep) && resolved !== root) return null;
  return resolved;
}

function listDir(dir, rel) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return null;
  }
  entries.sort((a, b) => a.name.localeCompare(b.name));
  const rows = entries
    .map((e) => {
      const href = path.posix.join(rel === '' ? '.' : rel, e.name);
      const icon = e.isDirectory() ? '&#128193;' : '&#128196;';
      return `<tr><td>${icon}</td><td><a href="${esc(href)}">${esc(e.name)}${e.isDirectory() ? '/' : ''}</a></td><td>${e.isDirectory() ? '-' : `${e.size} B`}</td></tr>`;
    })
    .join('\n');
  const parent = rel === '' ? null : `<tr><td>&#11014;</td><td><a href="${esc(path.posix.dirname(rel) === '.' ? '' : path.posix.dirname(rel))}">..</a></td><td>-</td></tr>`;
  return `<!doctype html><html><head><meta charset="utf-8"><title>gyre preview — ${esc(COMMIT.slice(0, 12))}</title>
<style>body{font-family:system-ui,sans-serif;margin:2rem;color:#222}table{border-collapse:collapse}td,th{padding:.25rem .75rem;text-align:left;border-bottom:1px solid #eee}header{margin-bottom:1.5rem}code{background:#f4f4f4;padding:.1rem .3rem;border-radius:3px}</style>
</head><body>
<header><h1>Gyre preview environment</h1>
<p>commit <code>${esc(COMMIT)}</code> &middot; environment <code>${esc(ENV_ID)}</code>${MESH_IP ? ` &middot; mesh <code>${esc(MESH_IP)}</code>` : ''}</p>
</header>
<table><tr><th></th><th>name</th><th>size</th></tr>
${parent || ''}
${rows}
</table></body></html>`;
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://localhost');
  if (url.pathname === '/__preview__/info') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ commit: COMMIT, env_id: ENV_ID, mesh_ip: MESH_IP, root: ROOT }));
    return;
  }
  const rel = decodeURIComponent(url.pathname).replace(/^\/+/, '');
  const target = safeJoin(path.resolve(ROOT), rel);
  if (!target) {
    res.writeHead(403);
    res.end('forbidden');
    return;
  }
  let stat;
  try {
    stat = fs.statSync(target);
  } catch {
    res.writeHead(404);
    res.end('not found');
    return;
  }
  if (stat.isDirectory()) {
    const page = listDir(target, rel);
    if (page === null) {
      res.writeHead(500);
      res.end('unreadable directory');
      return;
    }
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end(page);
    return;
  }
  const type = MIME[path.extname(target).toLowerCase()] || 'application/octet-stream';
  res.writeHead(200, { 'Content-Type': type });
  fs.createReadStream(target).pipe(res);
});

server.listen(PORT, '0.0.0.0', () => {
  console.log(`[preview-serve] serving ${ROOT} (commit ${COMMIT}) on 0.0.0.0:${PORT}`);
});
