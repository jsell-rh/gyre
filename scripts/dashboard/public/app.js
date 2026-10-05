// App shell: startup, view routing, keyboard nav, polling, SSE wiring.
import { fetchSnapshot, connectStreams, setParallelism } from "./api.js";
import { el, fmtAge } from "./dom.js";
import { getState, setState } from "./state.js";
import { renderOverview } from "./views/overview.js";
import { renderTasks } from "./views/tasks.js";
import { renderDependencies } from "./views/dependencies.js";
import { renderIncidents } from "./views/incidents.js";
import { renderDrawer, close as closeDrawer, drawerOpen } from "./views/drawer.js";

const VIEWS = ["overview", "tasks", "dependencies", "incidents"];

let data = null;
let connected = false;
const drawerEls = {
  root: null, id: null, title: null, badges: null, body: null, tabs: null,
};
for (const [k, id] of Object.entries({
  root: "drawer", id: "drawer-id", title: "drawer-title", badges: "drawer-badges",
  body: "drawer-body", tabs: "drawer-tabs",
})) {
  drawerEls[k] = document.getElementById(id);
}

const actions = {
  setView(view) { setState({ view, focusIndex: 0 }); render(); },
  openTask(id) { setState({ selectedTask: id }); render(); },
  closeTask() { setState({ selectedTask: null }); render(); },
  setFilter(filter) { setState({ filter, focusIndex: 0 }); render(); },
};

// --- rendering -------------------------------------------------------------
function render() {
  if (!data) return;
  const state = getState();

  // nav + view containers
  for (const b of document.querySelectorAll("#nav button, .brand")) {
    b.classList.toggle("active", b.dataset.nav === state.view);
  }
  for (const v of VIEWS) {
    const node = document.getElementById(`view-${v}`);
    node.hidden = v !== state.view;
  }

  // health strip (all views)
  const health = document.getElementById("health");
  health.hidden = false;
  renderHealth(health);

  const containers = {
    overview: document.getElementById("view-overview"),
    tasks: document.getElementById("view-tasks"),
    dependencies: document.getElementById("view-dependencies"),
    incidents: document.getElementById("view-incidents"),
  };
  const pass = { overview: renderOverview, tasks: renderTasks, dependencies: renderDependencies, incidents: renderIncidents }[state.view]
    || renderOverview;
  pass(containers[state.view], state, data, actions);

  // drawer
  if (state.selectedTask) renderDrawer(drawerEls, state, data, actions);
  else closeDrawer(drawerEls);
}

function renderHealth(health) {
  const h = data.health || {};
  const loopDot = h.loopAlive ? "ok" : "down";
  const fleetTxt = h.fleet && h.fleet.active != null
    ? `fleet ${h.fleet.active} active / ${h.fleet.parallelism ?? "?"} lever`
    : "fleet —";
  health.replaceChildren(...[
    el("span", {}, el("span", { class: `dot ${connected ? "ok" : "unknown"}` }), connected ? "connected" : "disconnected"),
    el("span", { class: "sep", text: "|" }),
    el("span", { text: `${h.workersActive ?? 0} active · ${h.workersStalled ?? 0} stalled · ${h.workersKnown ?? 0} known` }),
    el("span", { class: "sep", text: "|" }),
    el("span", { text: fleetTxt }),
    el("span", { class: "sep", text: "|" }),
    h.diskFree != null ? el("span", { text: `disk ${Math.round(h.diskFree)}% free` }) : null,
    h.wipGuarded ? el("span", { text: "· WIP stashed" }) : null,
    el("span", { class: "spacer", style: "flex:1" }),
    parallelismControl(),
    el("span", { class: "sep", text: "|" }),
    el("span", { text: `updated ${fmtAge(Date.now() - data.when)}` }),
  ].filter(Boolean)); // null children must not reach replaceChildren
}

function parallelismControl() {
  const h = data.health || {};
  const cur = h.fleet?.parallelism ?? null;
  const wrap = el("span", { class: "stepper" });
  if (cur == null) {
    wrap.append(el("span", { class: "feedback", text: "no fleet lever" }));
    return wrap;
  }
  const valEl = el("span", { class: "val", text: String(cur) });
  const fb = el("span", { class: "feedback" });
  const set = async (n) => {
    fb.className = "feedback"; fb.textContent = "…";
    try {
      const r = await setParallelism(n);
      if (!r.ok) throw new Error(r.error || "rejected");
      valEl.textContent = String(n);
      fb.textContent = `limit ${n} — new workers start as slots open`;
    } catch (e) {
      valEl.textContent = String(cur);
      fb.className = "feedback err"; fb.textContent = `write failed${e?.message ? ` (${e.message})` : ""}`;
    }
  };
  wrap.append(
    el("span", { text: "concurrent sandboxes", style: "margin-right:4px" }),
    el("button", { text: "−", onclick: () => set(Math.max(1, Number(valEl.textContent) - 1)) }),
    valEl,
    el("button", { text: "+", onclick: () => set(Math.min(100, Number(valEl.textContent) + 1)) }),
    fb,
  );
  return wrap;
}

// --- data flow ---------------------------------------------------------------
async function poll() {
  try {
    data = await fetchSnapshot();
    render();
  } catch {
    connected = false;
    const health = document.getElementById("health");
    if (health && data) render();
  }
}

// SSE: live updates between polls drive re-render of live rows.
const liveTails = new Map(); // taskId -> last summary seen
connectStreams({
  onChunk({ t, ev }) {
    if (ev && ev.summary) liveTails.set(t, ev.summary);
    if (data) {
      const row = (data.rows || []).find((r) => r.id === t);
      if (row) {
        if (!row.lastActivity || (ev && ev.kind !== "diagnostic")) {
          row.lastActivity = ev ? { summary: ev.summary, kind: ev.kind, time: ev.time } : row.lastActivity;
          row.lastActivityAgeMs = ev ? Date.now() - ev.time : row.lastActivityAgeMs;
        }
      }
      // light-touch update: only when the overview view is showing
      const state = getState();
      if (state.view === "overview") render();
    }
  },
  onOpen() { connected = true; },
  onClose() { connected = false; },
});

// --- keyboard nav -------------------------------------------------------------
document.addEventListener("keydown", (e) => {
  const typing = /input|textarea|select/i.test(document.activeElement?.tagName || "");
  if (e.key === "/" && !typing) {
    e.preventDefault();
    document.getElementById("global-search").focus();
    return;
  }
  if (e.key === "Escape") {
    if (typing) document.activeElement.blur();
    else if (drawerOpen(drawerEls)) actions.closeTask();
    return;
  }
  if (typing) return;

  const state = getState();
  const nav = { j: nextTask, k: prevTask, ArrowDown: nextTask, ArrowUp: prevTask, Enter: openFocused };
  if (nav[e.key]) { e.preventDefault(); nav[e.key](); }
  if (e.key === "[") { e.preventDefault(); shiftTask(-1); }
  if (e.key === "]") { e.preventDefault(); shiftTask(1); }
});

// The displayed list, matching what the user sees: in Tasks view the same
// filter chips + search the table applies; elsewhere the overview list.
function currentList() {
  if (!data) return [];
  const state = getState();
  if (state.view === "tasks") {
    return filterTaskRows(data.rows || [], state);
  }
  return data.overview || data.rows || [];
}

// Must stay in sync with renderTasks' filtering/sort (views/tasks.js).
function filterTaskRows(rows, state) {
  const FILTERS = {
    "active+attention": (r) => r.runtime === "active" || r.runtime === "stalled" || r.attention !== "none",
    "active": (r) => r.runtime === "active",
    "attention": (r) => r.attention !== "none",
    "needs-revision": (r) => r.result === "needs-revision",
    "all": () => true,
  };
  const test = FILTERS[state.filter] || FILTERS["active+attention"];
  const q = (state.search || "").toLowerCase();
  let out = rows.filter(test);
  if (q) {
    out = out.filter((r) =>
      r.id.includes(q) || (r.title || "").toLowerCase().includes(q) ||
      (r.specRef || "").toLowerCase().includes(q));
  }
  return out.slice().sort((a, b) =>
    rank(b) - rank(a) || (a.lastActivityAgeMs ?? 1e18) - (b.lastActivityAgeMs ?? 1e18) || (a.id < b.id ? -1 : 1));
}
function rank(r) {
  return (r.attention === "critical" ? 4 : r.attention === "warning" ? 3 : 0) +
    (r.runtime === "active" ? 2 : r.runtime === "stalled" ? 1 : 0);
}

function nextTask() { moveTask(1); }
function prevTask() { moveTask(-1); }
function moveTask(d) {
  const list = currentList();
  if (!list.length) return;
  const state = getState();
  const idx = Math.max(0, Math.min(list.length - 1, state.focusIndex + d));
  setState({ focusIndex: idx });
  actions.openTask(list[idx].id);
}
function openFocused() {
  const list = currentList();
  const state = getState();
  if (list[state.focusIndex]) actions.openTask(list[state.focusIndex].id);
}
function shiftTask(d) {
  const list = currentList();
  const state = getState();
  const cur = list.findIndex((r) => r.id === state.selectedTask);
  const next = list[(cur + d + list.length) % list.length];
  if (next) actions.openTask(next.id);
}

// --- wiring ---------------------------------------------------------------------
document.getElementById("nav").addEventListener("click", (e) => {
  const b = e.target.closest("[data-nav]");
  if (b) actions.setView(b.dataset.nav);
});
document.querySelector(".brand").addEventListener("click", () => actions.setView("overview"));
document.getElementById("drawer-close").addEventListener("click", () => actions.closeTask());
document.getElementById("drawer-backdrop").addEventListener("click", () => actions.closeTask());
document.getElementById("drawer-tabs").addEventListener("click", (e) => {
  const b = e.target.closest("[data-tab]");
  if (b) { setState({ drawerTab: b.dataset.tab }); render(); }
});
const searchEl = document.getElementById("global-search");
searchEl.addEventListener("input", debounce(() => {
  setState({ search: searchEl.value });
  if (getState().view !== "tasks") actions.setView("tasks");
  else render();
}, 200));
document.getElementById("main").addEventListener("click", (e) => {
  const a = e.target.closest("[data-nav]");
  if (a) { e.preventDefault(); actions.setView(a.dataset.nav); }
});

function debounce(fn, ms) {
  let t;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...args), ms);
  };
}

poll();
setInterval(poll, 2000);
