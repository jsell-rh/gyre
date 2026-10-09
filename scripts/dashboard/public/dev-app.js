// Durable controller cockpit. All state comes from the controller ledger.
const $ = (id) => document.getElementById(id);
const nodes = {
  health: $("health"), overview: $("view-overview"), tasks: $("view-tasks"),
  dependencies: $("view-dependencies"), events: $("view-incidents"),
  drawer: $("drawer"), drawerBody: $("drawer-body"),
  streamBottom: $("stream-bottom"), streamUnread: $("stream-unread"),
  streamControls: $("stream-controls"),
};
let snapshot = null;
let view = new URL(location.href).searchParams.get("v") || "overview";
let selected = new URL(location.href).searchParams.get("t");
let tab = "stream";
let filter = "active";
let search = "";
let logAttempt = null;
let streamSource = null;
let streamAttempt = null;
let streamFallback = null;
let streamHasEvents = false;
let fallbackLoading = false;
let streamFollowing = true;
let streamUnread = 0;
let streamExpansion = "default";
let errorText = "";
let retryAllPending = false;

function e(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = String(value);
    else if (key === "onclick") node.addEventListener("click", value);
    else node.setAttribute(key, value);
  }
  for (const child of children.flat()) if (child != null) node.append(child);
  return node;
}
function badge(text, kind = text) { return e("span", { class: `badge ${kind}`, text: text || "—" }); }
function title(task) { return snapshot?.titles?.[task.name]?.title || task.name; }
function spec(task) { return snapshot?.titles?.[task.name]?.specRef || ""; }
function prs(task) {
  const found = snapshot?.prs?.[task.name] || [];
  return task.pr_url && !found.some((pr) => pr.url === task.pr_url)
    ? [{ url: task.pr_url, number: task.pr_number, state: task.state === "merged" ? "MERGED" : "OPEN" }, ...found] : found;
}
function latestAttempt(name) { return (snapshot?.attempts || []).find((a) => a.task === name) || null; }
function taskEvents(name) { return (snapshot?.events || []).filter((event) => event.task === name); }
function ago(seconds) {
  if (!seconds) return "—";
  const n = Math.max(0, Math.floor(Date.now() / 1000 - seconds));
  return n < 60 ? `${n}s` : n < 3600 ? `${Math.floor(n / 60)}m` : n < 86400 ? `${Math.floor(n / 3600)}h` : `${Math.floor(n / 86400)}d`;
}
function until(seconds) {
  const remaining = Math.max(0, Math.ceil(seconds - Date.now() / 1000));
  return remaining < 60 ? `${remaining}s` : remaining < 3600 ? `${Math.ceil(remaining / 60)}m` : `${Math.ceil(remaining / 3600)}h`;
}
const relativeTime = new Intl.RelativeTimeFormat(undefined, { numeric: "always" });
function agentTime(at) {
  const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
  if (seconds < 60) return "just now";
  for (const [unit, size] of [["year", 31536000], ["month", 2592000], ["day", 86400], ["hour", 3600], ["minute", 60]]) {
    if (seconds >= size) return relativeTime.format(-Math.floor(seconds / size), unit);
  }
  return "just now";
}
function updateAgentTimes() {
  nodes.drawerBody.querySelectorAll(".dev-agent-event-head time[data-at]").forEach((node) => {
    node.textContent = agentTime(Number(node.dataset.at));
  });
}
function missingDeps(task) {
  const merged = new Set((snapshot?.tasks || []).filter((t) => t.state === "merged").map((t) => t.name));
  return (task.deps || []).filter((dep) => !merged.has(dep));
}
function state(task) { return task.state === "ready" && missingDeps(task).length ? "blocked" : task.state; }
function infrastructureQueueReason(task) {
  if (/^(MissingProviders:|ProviderCheckUnavailable:)/.test(snapshot?.health?.condition || "")) return "Waiting for required gateway providers";
  if (missingDeps(task).length) return "Waiting for prerequisites";
  if (task.retry_at > Date.now() / 1000) return `Backoff · retry in ${until(task.retry_at)}`;
  return snapshot?.health?.failures ? "Ready to retry · queued behind capacity probe" : "Ready to retry · queued for admitted slot";
}
function setURL(key, value) {
  const u = new URL(location.href);
  if (value) u.searchParams.set(key, value); else u.searchParams.delete(key);
  history.replaceState(null, "", u);
}
async function post(path, body) {
  const r = await fetch(path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const result = await r.json();
  if (!r.ok) throw new Error(result.error || `HTTP ${r.status}`);
  return result;
}
async function slots(value) {
  try { await post("/api/slots", { value }); errorText = ""; await poll(); }
  catch (error) { errorText = String(error.message || error); render(); }
}
async function retry(name) {
  try { await post("/api/retry", { task: name }); errorText = ""; await poll(); }
  catch (error) { errorText = String(error.message || error); render(); }
}
async function retryAll() {
  if (retryAllPending) return;
  retryAllPending = true;
  renderOverview();
  try { await post("/api/retry-all", {}); errorText = ""; await poll(); }
  catch (error) { errorText = String(error.message || error); render(); }
  finally { retryAllPending = false; renderOverview(); }
}

function renderHealth() {
  const s = snapshot || {};
  const online = s.online;
  const value = s.slots ?? 0;
  const gate = s.health || {};
  nodes.health.hidden = false;
  nodes.health.replaceChildren(
    e("span", {}, e("span", { class: `dot ${online ? "ok" : "down"}` }), online ? " controller online" : " controller offline"),
    e("span", { class: "sep", text: "|" }),
    e("span", { text: `${s.running ?? 0} running / ${gate.effective_slots ?? value} admitted / ${value} desired` }),
    e("span", { class: "sep", text: "|" }),
    e("span", { text: `${s.eligible ?? 0} dependency-eligible · ${s.counts?.deferred ?? 0} queued · ${s.counts?.failed ?? 0} failed · ${s.counts?.merged ?? 0} upstream complete · ${s.confirmed_merges ?? 0} shipped by controller` }),
    e("span", { class: "health-spacer" }),
    e("span", { class: "stepper" },
      e("span", { text: "sandboxes " }),
      e("button", { onclick: () => slots(Math.max(0, value - 1)), text: "−" }),
      e("span", { class: "val", text: value }),
      e("button", { onclick: () => slots(Math.min(1000, value + 1)), text: "+" }),
      e("button", { onclick: () => slots(value === 0 ? 4 : 0), text: value === 0 ? "Resume" : "Drain" })),
    errorText || s.error ? e("span", { class: "dev-health-error", text: errorText || s.error }) : "",
  );
}

function taskButton(task, details = "") {
  return e("button", { class: "dev-task-button", onclick: () => openTask(task.name) },
    e("span", { class: "mono", text: task.name }),
    e("span", { class: "dev-task-title", text: title(task) }),
    badge(state(task)),
    details ? e("span", { class: "dev-detail", text: details }) : null);
}

function prLinks(task) {
  const links = prs(task);
  return links.length ? e("span", { class: "dev-pr-links" }, links.map((pr) =>
    e("a", { href: pr.url, target: "_blank", rel: "noopener noreferrer", title: pr.title,
      onclick: (event) => event.stopPropagation(), text: `PR #${pr.number}` }))) : e("span", { class: "muted", text: "—" });
}
function mergeLink(task) {
  const sha = task.merge_sha;
  return /^[0-9a-f]{40}$/.test(sha || "") && snapshot?.repositoryUrl ?
    e("a", { href: `${snapshot.repositoryUrl}/commit/${sha}`, target: "_blank", rel: "noopener noreferrer",
      text: sha.slice(0, 12) }) : e("span", { class: "muted", text: "—" });
}

function coverageChart(trend) {
  const wrap = e("div", { class: "dev-coverage-chart" });
  if (!trend.length) return wrap;
  const W = 260, H = 66, P = 4;
  const values = trend.map((p) => p.pct);
  const lo = Math.min(...values), hi = Math.max(...values), span = Math.max(1, hi - lo);
  const times = trend.map((p) => Date.parse(p.at));
  const first = times[0], elapsed = times.at(-1) - first;
  const points = values.map((v, i) => [P + ((Number.isFinite(elapsed) && elapsed > 0 ? (times[i] - first) / elapsed : i / Math.max(1, values.length - 1))) * (W - 2 * P), H - P - (v - lo) / span * (H - 2 * P)]);
  const path = points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`); svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `Coverage over ${trend.length} revisions, ${values[0]} to ${values.at(-1)} percent`);
  const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
  line.setAttribute("d", path); line.setAttribute("fill", "none");
  line.setAttribute("stroke", "currentColor"); line.setAttribute("stroke-width", "2");
  svg.append(line);
  const delta = values.at(-1) - values[0];
  const from = new Date(trend[0].at).toLocaleDateString();
  const to = new Date(trend.at(-1).at).toLocaleDateString();
  wrap.append(svg, e("span", { class: "dev-coverage-delta", text: `${from}–${to} · ${delta >= 0 ? "+" : ""}${delta.toFixed(1)} points` }));
  return wrap;
}

function renderOverview() {
  const s = snapshot || {};
  const tasks = s.tasks || [];
  const active = tasks.filter((t) => ["running", "checking", "promoting"].includes(t.state));
  const failed = tasks.filter((t) => t.state === "failed");
  const deferred = tasks.filter((t) => t.state === "deferred");
  const gate = s.health || {};
  const candidate = tasks.filter((t) => t.state === "candidate");
  const blocked = tasks.filter((t) => t.state === "blocked");
  const published = tasks.filter((t) => t.state === "published");
  const cards = [
    ["Dependency-eligible", s.eligible ?? 0], ["Running", s.running ?? 0],
    ["Candidates", candidate.length + (s.counts?.checking || 0)],
    ["Infra queue", deferred.length], ["Failed", failed.length], ["Shipped by controller", s.confirmed_merges || 0],
    ["Sandboxes charged", s.resources?.used ?? s.running ?? 0], ["Deletion pending", s.resources?.deletion_pending || 0],
  ];
  const coverage = s.coverage || {};
  const covered = (coverage.implemented || 0) + (coverage.verified || 0);
  const coverageBar = e("section", { class: "dev-coverage" },
    e("div", { class: "dev-coverage-number", text: `${(coverage.pct || 0).toFixed(1)}%` }),
    e("div", {}, e("strong", { text: "Spec coverage" }),
      e("div", { class: "muted", text: `${covered} of ${coverage.applicable || 0} applicable sections · ${coverage.verified || 0} verified · ${coverage.assigned || 0} assigned` })),
    e("div", { class: "dev-coverage-visual" }, coverageChart(coverage.trend || []),
      e("div", { class: "dev-coverage-track" }, e("div", { class: "dev-coverage-fill", style: `width:${Math.min(100, Math.max(0, coverage.pct || 0))}%` } ))));
  const metrics = e("div", { class: "dev-metrics" }, cards.map(([label, count]) =>
    e("div", { class: "dev-metric" }, e("div", { class: "label", text: label }), e("div", { class: "number", text: count }))));
  const control = e("section", { class: "dev-panel" },
    e("h2", { text: "Controller" }),
    e("p", { text: s.online ? `Reconciling specs to code · ${gate.condition || "Healthy"}` : s.present ? "Stopped. Run: python3 scripts/dev-controller.py run --slots 8" : "No ledger yet. Run: python3 scripts/dev-controller.py sync" }),
    e("p", { class: "muted", text: s.resources?.inventory_ready ? `Remote resource inventory current · ${s.resources.used} charged · ${s.resources.deletion_pending} awaiting deletion` : "Admission waits for a complete remote sandbox inventory." }),
    gate.inventory_error ? e("p", { class: "muted", text: `Inventory: ${gate.inventory_error}` }) : null,
    e("p", { class: "muted", text: `${(s.metrics?.deliveries_per_hour || 0).toFixed(2)} recorded deliveries/hour · ${s.metrics?.candidate_backlog || 0} candidates awaiting delivery · ${s.metrics?.automatic_repairs || 0} automatic repairs` }),
    s.dispatch?.prerequisite_repairs?.length ? e("p", { text: `Main-baseline repair has integration priority: ${s.dispatch.prerequisite_repairs.join(", ")}. Independent implementation continues; other candidates wait for the repair to ship.` }) : null,
    (s.metrics?.candidate_backlog || 0) >= (s.dispatch?.candidate_limit || 8) ? e("p", { class: "muted", text: `Implementation admission paused: candidate backlog reached ${s.dispatch?.candidate_limit || 8}. Integration and CI continue reconciling.` }) : null,
    s.dispatch?.only_task ? e("p", { text: `Dispatch restricted to ${s.dispatch.only_task}. Eligible counts include tasks excluded by this restriction.` }) : null,
    s.dispatch?.publication === "pr" ? e("p", { class: "muted", text: "PR review mode: checks reconcile automatically; passing PRs wait for merge mode." }) : null,
    e("p", { class: "muted", text: /^(MissingProviders:|ProviderCheckUnavailable:)/.test(gate.condition || "") ? "Admission paused: required gateway providers are missing or unavailable. Setup is checked again each cycle." : gate.condition === "ConfigurationInvalid" ? "Admission paused: repair gateway configuration, then retry the failed task." : gate.retry_at > Date.now() / 1000 ? `Gateway backoff: next admission probe in ${until(gate.retry_at)} · ${gate.failures} consecutive infrastructure failures` : gate.failures ? `Gateway capacity probe ${gate.effective_slots > s.running ? "due on next cycle" : "running"} · existing sandboxes continue working` : `Gateway admission: ${gate.effective_slots ?? 0} of ${s.slots ?? 0} desired slots · expands as sandboxes become Ready` }),
    e("p", { class: "muted", text: "Set sandboxes to 0 to drain. Active attempts finish and keep their checkpoints." }),
    e("div", { class: "dev-control-line", text: `${(s.attempts || []).filter((a) => a.state === "running" && a.kind === "worker").length} implementing · ${(s.attempts || []).filter((a) => a.state === "running" && a.kind === "check").length} checking · ${s.slots ?? "—"} slots` }));
  const live = e("section", { class: "dev-panel" }, e("h2", { text: `Active attempts · ${active.length}` }),
    active.length ? active.map((task) => {
      const a = latestAttempt(task.name);
      return taskButton(task, a ? `${a.phase || a.kind} · ${a.reason || task.condition || ""} · ${ago(a.started)} · ${a.branch || a.sha?.slice(0, 12) || ""}` : "starting");
    }) : e("p", { class: "muted", text: s.online ? s.dispatch?.only_task ? `No worker active for ${s.dispatch.only_task}. Other eligible tasks are excluded by the dispatch restriction.` : "No attempt is active. Dispatch waits for admission, prerequisites, or candidate backlog capacity." : "Controller is offline." }));
  const attention = e("section", { class: "dev-panel" },
    e("div", { class: "dev-panel-heading" }, e("h2", { text: `Needs attention · ${failed.length}` }),
      failed.length ? e("button", { class: "dev-action", onclick: retryAll,
        text: retryAllPending ? "Retrying…" : `Retry all ${failed.length}` }) : null),
    failed.length ? failed.slice(0, 12).map((task) => e("div", { class: "dev-attention-row" }, taskButton(task, latestAttempt(task.name)?.detail || ""),
      e("button", { class: "dev-action", onclick: () => retry(task.name), text: "Retry" })))
      : e("p", { class: "muted", text: "No failed tasks." }));
  const waiting = e("section", { class: "dev-panel" }, e("h2", { text: `Infrastructure retry queue · ${deferred.length}` }),
    deferred.length ? deferred.slice(0, 12).map((task) => taskButton(task, infrastructureQueueReason(task)))
      : e("p", { class: "muted", text: "No task is queued after an infrastructure failure." }));
  nodes.overview.replaceChildren(coverageBar, metrics, errorText || s.error ? e("div", { class: "dev-error", text: errorText || s.error }) : "",
    e("div", { class: "dev-overview-grid" }, e("div", {}, control, live, waiting), attention),
    blocked.length ? e("section", { class: "dev-panel" }, e("h2", { text: `Blocked delivery · ${blocked.length}` }),
      blocked.map((task) => taskButton(task, task.condition || "Waiting for prerequisite repair"))) : null,
    published.length ? e("section", { class: "dev-panel" }, e("h2", { text: `Pull requests under reconciliation · ${published.length}` }),
      published.map((task) => e("div", {}, taskButton(task, task.condition || "Waiting for GitHub checks"), prLinks(task)))) : null,
    e("section", { class: "dev-panel" }, e("h2", { text: `Waiting candidates · ${candidate.length}` }),
      candidate.length ? candidate.slice(0, 10).map((task) => taskButton(task, `candidate ${task.candidate?.slice(0, 12) || ""}`))
        : e("p", { class: "muted", text: "No candidate waiting for verification." })));
}

const FILTERS = ["active", "eligible", "deferred", "blocked", "failed", "candidate", "published", "merged", "all"];
function matches(task) {
  const value = state(task);
  if (filter === "active" && !["running", "checking", "promoting"].includes(value)) return false;
  if (filter === "eligible" && !["ready", "candidate"].includes(value)) return false;
  if (!["active", "eligible", "all"].includes(filter) && value !== filter) return false;
  const q = search.toLowerCase();
  return !q || task.name.includes(q) || title(task).toLowerCase().includes(q) || spec(task).toLowerCase().includes(q);
}
function renderTasks() {
  const tasks = (snapshot?.tasks || []).filter(matches).sort((a, b) => a.name.localeCompare(b.name));
  const filters = e("div", { class: "dev-filters" }, FILTERS.map((name) => e("button", {
    class: filter === name ? "selected" : "", text: name,
    onclick: () => { filter = name; renderTasks(); },
  })));
  const table = e("table", { class: "tasks" },
    e("thead", {}, e("tr", {}, ["Task", "Title", "Controller state", "Progress", "PRs", "Attempts", "Last event"].map((name) => e("th", { text: name })))),
    e("tbody", {}, tasks.map((task) => {
      const event = taskEvents(task.name)[0];
      const attempt = latestAttempt(task.name);
      return e("tr", { onclick: () => openTask(task.name) },
        e("td", { class: "id", text: task.name }), e("td", { class: "title", text: title(task) }),
        e("td", {}, badge(state(task))), e("td", {}, badge(task.progress)),
        e("td", {}, prLinks(task)),
        e("td", { text: task.attempts || 0 }),
        e("td", { class: "dev-last", text: event ? `${event.message} · ${ago(event.at)}` : attempt ? `${attempt.kind} ${attempt.state} · ${ago(attempt.started)}` : "—" }));
    })));
  nodes.tasks.replaceChildren(e("h2", { class: "section", text: `Tasks · ${tasks.length} shown / ${(snapshot?.tasks || []).length} total` }), filters, e("div", { class: "dev-table-wrap" }, table));
}
function renderDeps() {
  const tasks = snapshot?.tasks || [];
  const task = tasks.find((t) => t.name === selected) || tasks.find((t) => t.state === "running") || tasks[0];
  if (!task) { nodes.dependencies.replaceChildren(e("p", { class: "muted", text: "No tasks in ledger." })); return; }
  const byName = new Map(tasks.map((t) => [t.name, t]));
  const deps = (task.deps || []).map((name) => byName.get(name)).filter(Boolean);
  const dependents = tasks.filter((t) => (t.deps || []).includes(task.name));
  const col = (label, rows) => e("div", { class: "dev-dep-col" }, e("h2", { text: `${label} · ${rows.length}` }),
    rows.length ? rows.map((row) => taskButton(row)) : e("p", { class: "muted", text: "None" }));
  nodes.dependencies.replaceChildren(e("p", { class: "muted", text: "Select a task to inspect its prerequisites and dependents." }),
    e("div", { class: "dev-deps" }, col("Prerequisites", deps), col("Selected", [task]), col("Dependents", dependents)));
}
function renderEvents() {
  nodes.events.replaceChildren(e("h2", { class: "section", text: "Controller events" }),
    ...(snapshot?.events || []).map((event) => e("button", { class: "dev-event", onclick: () => event.task && openTask(event.task) },
      e("span", { class: "mono", text: event.task || "controller" }),
      e("span", { text: event.message }), e("time", { text: ago(event.at) }))));
}

async function showLog(id) {
  logAttempt = id;
  const body = nodes.drawerBody;
  const same = body.dataset.logAttempt === id && body.querySelector("pre.rawlog");
  const pre = same || e("pre", { class: "rawlog", text: "Loading attempt log…" });
  const atBottom = body.scrollTop + body.clientHeight >= body.scrollHeight - 24;
  const oldScroll = body.scrollTop;
  if (!same) { body.dataset.logAttempt = id; body.replaceChildren(pre); }
  try {
    const response = await fetch(`/api/log?attempt=${encodeURIComponent(id)}`);
    const result = await response.json();
    if (!response.ok) throw new Error(result.error);
    if (logAttempt === id && tab === "raw") {
      pre.textContent = result.content || "No output yet.";
      body.scrollTop = atBottom ? body.scrollHeight : oldScroll;
    }
  } catch (error) { if (logAttempt === id && tab === "raw") pre.textContent = String(error.message || error); }
}
function closeStream() {
  streamSource?.close();
  streamSource = null;
  streamAttempt = null;
  streamFallback = null;
  streamHasEvents = false;
  streamFollowing = true;
  streamUnread = 0;
  streamExpansion = "default";
  nodes.streamControls.hidden = true;
  updateStreamBottom();
}
function nearStreamBottom() {
  const body = nodes.drawerBody;
  return body.scrollHeight - body.scrollTop - body.clientHeight <= 60;
}
function updateStreamBottom() {
  nodes.streamBottom.hidden = tab !== "stream" || !streamAttempt || streamFollowing;
  nodes.streamUnread.hidden = streamUnread === 0;
  nodes.streamUnread.textContent = streamUnread > 99 ? "99+ new" : `${streamUnread} new`;
}
function followStream() {
  streamFollowing = true;
  streamUnread = 0;
  nodes.drawerBody.querySelectorAll("[data-unread]").forEach((node) => delete node.dataset.unread);
  nodes.drawerBody.scrollTop = nodes.drawerBody.scrollHeight;
  updateStreamBottom();
}
function noteStreamUpdate(node) {
  if (streamFollowing) {
    nodes.drawerBody.scrollTop = nodes.drawerBody.scrollHeight;
  } else if (node && !node.dataset.unread) {
    node.dataset.unread = "true";
    streamUnread++;
    updateStreamBottom();
  }
}
async function refreshStreamFallback(id) {
  if (streamHasEvents || fallbackLoading || streamAttempt !== id || !streamFallback) return;
  fallbackLoading = true;
  try {
    const response = await fetch(`/api/log?attempt=${encodeURIComponent(id)}`, { cache: "no-store" });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error);
    if (streamAttempt === id && !streamHasEvents && streamFallback) {
      const next = (result.content || "No output yet.").slice(-40000);
      const changed = streamFallback.textContent !== next;
      const first = streamFallback.textContent === "Loading attempt log…";
      streamFallback.textContent = next;
      if (changed) {
        if (first || streamFollowing) nodes.drawerBody.scrollTop = nodes.drawerBody.scrollHeight;
        else { streamUnread++; updateStreamBottom(); }
      }
    }
  } catch (error) {
    if (streamAttempt === id && streamFallback) streamFallback.textContent = String(error.message || error);
  } finally { fallbackLoading = false; }
}
function showStream(id) {
  if (streamAttempt === id && streamSource) { refreshStreamFallback(id); return; }
  closeStream();
  streamAttempt = id;
  nodes.streamControls.hidden = false;
  updateStreamBottom();
  const body = nodes.drawerBody;
  const status = e("p", { class: "muted", text: "No structured agent events yet. This round may have started before streaming was enabled; showing its attempt log below." });
  streamFallback = e("pre", { class: "rawlog", text: "Loading attempt log…" });
  const feed = e("div", { class: "dev-agent-feed" });
  body.replaceChildren(status, streamFallback, feed);
  refreshStreamFallback(id);
  const source = new EventSource(`/api/attempt-stream?attempt=${encodeURIComponent(id)}`);
  streamSource = source;
  let activeGroup = null;
  let activeKind = null;
  function addGroup(kind, name, summary, at) {
    const stamp = new Date(at);
    const time = at && !Number.isNaN(stamp.getTime())
      ? e("time", { text: agentTime(stamp.getTime()), datetime: stamp.toISOString(), title: stamp.toLocaleString(), "data-at": stamp.getTime() })
      : e("time");
    const heading = e("summary", { class: "dev-agent-event-head" },
      e("span", { class: "dev-agent-caret", text: "▸" }),
      e("strong", { text: name }), e("span", { class: "dev-agent-summary", text: summary }),
      time);
    const content = e("pre", { class: kind === "text" ? "dev-agent-text" : "dev-agent-output" });
    const group = e("details", { class: "dev-agent-event" }, heading, content);
    group.open = streamExpansion === "all" || (streamExpansion === "default" && kind === "text");
    feed.append(group);
    activeGroup = group;
    activeKind = kind;
    return group;
  }
  source.addEventListener("agent", (message) => {
    if (streamAttempt !== id || tab !== "stream") return;
    let event;
    try { event = JSON.parse(message.data); } catch { return; }
    streamHasEvents = true;
    status.remove();
    streamFallback?.remove();
    streamFallback = null;
    let changed = null;
    if (event.type === "text") {
      const group = activeKind === "text" ? activeGroup : addGroup("text", event.role || "Assistant", "Response", event.at);
      group.querySelector("pre").textContent += event.text || "";
      changed = group;
    } else if (event.type === "tool_start") {
      changed = addGroup("tool", event.name || "tool", event.text || "running", event.at);
    } else if (event.type === "tool_output") {
      const group = activeKind === "tool" ? activeGroup : addGroup("tool", event.name || "tool", "Output", event.at);
      group.querySelector("pre").textContent += event.text || "";
      changed = group;
    } else if (event.type === "tool_end") {
      if (activeKind === "tool" && activeGroup) {
        if (event.error) activeGroup.classList.add("failed");
        activeGroup.querySelector(".dev-agent-summary").textContent += event.error ? " · failed" : " · done";
        changed = activeGroup;
      }
      activeKind = null;
      activeGroup = null;
    }
    while (feed.childElementCount > 400) {
      const first = feed.firstElementChild;
      const height = first.getBoundingClientRect().height + 8;
      if (first.dataset.unread) streamUnread = Math.max(0, streamUnread - 1);
      first.remove();
      if (!streamFollowing) body.scrollTop = Math.max(0, body.scrollTop - height);
    }
    if (changed) noteStreamUpdate(changed);
  });
  source.onerror = () => {
    if (streamAttempt === id && !streamHasEvents) status.textContent = "Agent event connection is reconnecting; showing the attempt log below.";
  };
}
function renderDrawer() {
  const task = (snapshot?.tasks || []).find((t) => t.name === selected);
  if (!task) { if (snapshot?.tasks?.length) closeTask(); return; }
  $("drawer-id").textContent = task.name;
  $("drawer-title").textContent = title(task);
  $("drawer-badges").replaceChildren(badge(state(task)), badge(task.progress));
  nodes.drawer.classList.add("open"); $("drawer-backdrop").classList.add("open");
  for (const button of $("drawer-tabs").querySelectorAll("button")) button.classList.toggle("on", button.dataset.tab === tab);
  if (tab === "stream") {
    const id = logAttempt || latestAttempt(task.name)?.id;
    if (id) showStream(id); else nodes.drawerBody.replaceChildren(e("p", { class: "muted", text: "No attempt yet." }));
    return;
  }
  closeStream();
  if (tab === "raw") {
    const id = logAttempt || latestAttempt(task.name)?.id;
    if (id) showLog(id); else nodes.drawerBody.replaceChildren(e("p", { class: "muted", text: "No attempt log yet." }));
    return;
  }
  const attempts = (snapshot?.attempts || []).filter((a) => a.task === task.name);
  nodes.drawerBody.replaceChildren(...[
    e("div", { class: "dev-detail-grid" },
      e("span", { text: "Spec" }), e("span", { text: spec(task) || "—" }),
      e("span", { text: "Pull requests" }), prLinks(task),
      e("span", { text: "Shipped commit" }), mergeLink(task),
      e("span", { text: "Candidate" }), e("span", { class: "mono", text: task.candidate || "—" }),
      e("span", { text: "Seed" }), e("span", { class: "mono", text: task.seed || "—" }),
      e("span", { text: "Prerequisites" }), e("span", { text: (task.deps || []).join(", ") || "none" })),
    task.state === "deferred" ? e("p", { text: `${task.condition || "Infrastructure unavailable"}. ${infrastructureQueueReason(task)}.` }) : null,
    task.feedback ? e("p", { text: `${task.condition === "RepairLimitExceeded" ? "Repair limit reached" : "Integration repair"} · ${task.repairs || 0}/3 automatic repairs. Gate findings are retained and supplied to the next implementation and review rounds.` }) : null,
    task.generation ? e("p", { class: "muted", text: `Desired generation ${task.generation.slice(0, 12)} · observed ${task.observed_generation?.slice(0, 12) || "pending"}` }) : null,
    task.state === "failed" ? e("button", { class: "dev-action", onclick: () => retry(task.name), text: "Retry task" }) : null,
    e("h3", { text: `Attempts · ${attempts.length} recent` }),
    ...attempts.map((a) => e("button", { class: "dev-history", onclick: () => { tab = "raw"; logAttempt = a.id; renderDrawer(); } },
      e("span", { class: "mono", text: a.id.slice(0, 8) }), badge(a.kind), badge(a.state),
      e("span", { text: ago(a.started) }), e("span", { class: "mono", text: a.branch || a.sha?.slice(0, 12) || "" }))),
    e("h3", { text: "Recent events" }),
    ...taskEvents(task.name).slice(0, 20).map((event) => e("div", { class: "dev-drawer-event", text: `${ago(event.at)} · ${event.message}` }))].filter(Boolean));
}
function openTask(name) { selected = name; tab = "stream"; logAttempt = null; closeStream(); setURL("t", name); renderDrawer(); if (view === "dependencies") renderDeps(); }
function closeTask() { selected = null; logAttempt = null; closeStream(); setURL("t", null); nodes.drawer.classList.remove("open"); $("drawer-backdrop").classList.remove("open"); }
function changeView(next) {
  view = next; setURL("v", next === "overview" ? null : next);
  for (const button of document.querySelectorAll("[data-nav]")) button.classList.toggle("active", button.dataset.nav === view);
  for (const name of ["overview", "tasks", "dependencies", "incidents"]) $("view-" + name).hidden = name !== view;
  render();
}
function render() {
  renderHealth();
  if (view === "overview") renderOverview();
  if (view === "tasks") renderTasks();
  if (view === "dependencies") renderDeps();
  if (view === "incidents") renderEvents();
  if (selected) renderDrawer();
}
async function poll() {
  try {
    const response = await fetch("/api/snapshot", { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    snapshot = await response.json();
    errorText = "";
    render();
  } catch (error) { errorText = String(error.message || error); render(); }
}
document.querySelectorAll("[data-nav]").forEach((button) => button.addEventListener("click", () => changeView(button.dataset.nav)));
$("global-search").addEventListener("input", (event) => { search = event.target.value; changeView("tasks"); });
$("drawer-close").addEventListener("click", closeTask);
$("drawer-backdrop").addEventListener("click", closeTask);
nodes.drawerBody.addEventListener("scroll", () => {
  if (tab !== "stream" || !streamAttempt) return;
  if (nearStreamBottom()) followStream();
  else if (streamFollowing) { streamFollowing = false; updateStreamBottom(); }
});
nodes.streamBottom.addEventListener("click", followStream);
$("stream-expand").addEventListener("click", () => {
  streamExpansion = "all";
  nodes.drawerBody.querySelectorAll(".dev-agent-event").forEach((group) => { group.open = true; });
  if (streamFollowing) followStream();
});
$("stream-collapse").addEventListener("click", () => {
  streamExpansion = "none";
  nodes.drawerBody.querySelectorAll(".dev-agent-event").forEach((group) => { group.open = false; });
  if (streamFollowing) followStream();
});
$("drawer-tabs").addEventListener("click", (event) => { const button = event.target.closest("[data-tab]"); if (button) { tab = button.dataset.tab; if (tab === "activity") logAttempt = null; if (tab !== "stream") closeStream(); renderDrawer(); } });
document.addEventListener("keydown", (event) => { if (event.key === "Escape") closeTask(); });
changeView(["overview", "tasks", "dependencies", "incidents"].includes(view) ? view : "overview");
poll(); setInterval(poll, 2000);
setInterval(updateAgentTimes, 15000);
