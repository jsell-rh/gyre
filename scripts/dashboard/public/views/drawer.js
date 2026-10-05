// Task detail drawer: header + activity tab (classified events) + raw log tab.
// Both tabs render oldest → newest and stick to the bottom (autoscroll),
// pausing when the user scrolls up to read history, resuming when they
// return to the bottom.
import { el, fmtClock } from "../dom.js";
import { fetchActivity, fetchRawLog } from "../api.js";

const BOTTOM_EPS = 24; // px from bottom that still counts as "at bottom"

export function renderDrawer(drawerEls, state, data, actions) {
  const { root, id, title, badges, body, tabs } = drawerEls;
  const row = (data.rows || []).find((r) => r.id === state.selectedTask);
  if (!row) { close(drawerEls); return; }

  const taskChanged = root.dataset.task !== row.id;
  root.dataset.task = row.id;

  root.classList.add("open");
  document.getElementById("drawer-backdrop").classList.add("open");
  id.textContent = row.id;
  title.textContent = row.title;
  badges.replaceChildren(...[
    badgeEl(row.runtime, row.runtime),
    badgeEl(row.result, row.result),
    row.round != null ? el("span", { class: "badge", text: `r${row.round}/${row.roundsTotal ?? "?"}` }) : null,
    row.sandbox ? el("span", { class: "badge", text: row.sandbox }) : null,
  ].filter(Boolean));

  for (const b of tabs.querySelectorAll("button")) {
    b.classList.toggle("on", b.dataset.tab === state.drawerTab);
  }

  // New task or tab switch: full reload. Same task+tab: incremental append
  // of new events (keeps scroll position + stick-to-bottom behavior).
  const tabChanged = root.dataset.tab !== state.drawerTab;
  root.dataset.tab = state.drawerTab;

  if (taskChanged || tabChanged) {
    body.replaceChildren(el("div", { class: "empty-note", text: "loading…" }));
    attachScrollSensor(body);
    if (state.drawerTab === "activity") renderActivity(body, row);
    else renderRaw(body, row);
  } else if (state.drawerTab === "activity") {
    appendNewEvents(body, row);
  }
}

function badgeEl(text, cls) {
  return el("span", { class: `badge ${cls || ""}`, text });
}

// Track whether the user is at the bottom; expose via dataset + custom prop.
function attachScrollSensor(body) {
  if (body.dataset.sensor === "on") return;
  body.dataset.sensor = "on";
  body.userAtBottom = true;
  body.addEventListener("scroll", () => {
    body.userAtBottom =
      body.scrollTop + body.clientHeight >= body.scrollHeight - BOTTOM_EPS;
  }, { passive: true });
}

// Events are rendered oldest → newest; autoscroll to bottom when the user
// hasn't scrolled up.
function stickToBottom(body) {
  if (body.userAtBottom !== false) body.scrollTop = body.scrollHeight;
}

async function renderActivity(body, row) {
  const { events } = await fetchActivity(row.id).catch(() => ({ events: [] }));
  const list = events || []; // oldest → newest, newest at the bottom
  body.replaceChildren();
  if (!list.length) {
    body.append(el("div", { class: "empty-note", text: "No classified activity yet — events stream in live." }));
  }
  for (const ev of list.slice(-400)) {
    body.append(evLine(ev));
  }
  body.userAtBottom = true;
  stickToBottom(body);
}

// Incremental update on poll: append events newer than the last one shown.
async function appendNewEvents(body, row) {
  const last = body.lastElementChild;
  const lastTime = last && Number(last.dataset.time) || 0;
  const { events } = await fetchActivity(row.id).catch(() => ({ events: [] }));
  const list = (events || []).filter((e) => e.time > lastTime);
  if (!list.length) return;
  for (const ev of list) body.append(evLine(ev));
  // Trim to bound DOM size
  while (body.children.length > 500) body.firstElementChild.remove();
  stickToBottom(body);
}

function evLine(ev) {
  const node = el("div", { class: "ev-line" },
    el("span", { class: "t", text: fmtClock(ev.time) }),
    el("span", { class: `k ${ev.kind}`, text: ev.kind }),
    el("span", { class: "s", text: ev.summary }),
    ev.count > 1 ? el("span", { class: "n", text: `×${ev.count}` }) : null,
  );
  node.dataset.time = ev.time || 0;
  return node;
}

async function renderRaw(body, row) {
  const raw = await fetchRawLog(row.id).catch(() => null);
  body.replaceChildren();
  if (raw == null) {
    body.append(el("div", { class: "empty-note", text: "No raw log on disk (sandbox worker only)." }));
    return;
  }
  const pre = el("pre", { class: "rawlog" });
  pre.textContent = raw.replace(/\x00+/g, "").slice(-64 * 1024);
  body.append(pre);
  body.userAtBottom = true;
  stickToBottom(body); // raw log tail: newest output at the bottom
}

export function close(drawerEls) {
  drawerEls.root.classList.remove("open");
  document.getElementById("drawer-backdrop").classList.remove("open");
  delete drawerEls.root.dataset.task;
  delete drawerEls.root.dataset.tab;
}

export function drawerOpen(drawerEls) {
  return drawerEls.root.classList.contains("open");
}
