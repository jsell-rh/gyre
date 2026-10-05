// Tasks view: dense filterable table of every tracked task.
import { el, syncKeyed, fmtAge, badge } from "../dom.js";

const FILTERS = [
  { id: "active+attention", label: "Active + Attention", test: (r) => r.runtime === "active" || r.runtime === "stalled" || r.attention !== "none" },
  { id: "active", label: "Active", test: (r) => r.runtime === "active" },
  { id: "attention", label: "Attention", test: (r) => r.attention !== "none" },
  { id: "needs-revision", label: "Needs revision", test: (r) => r.result === "needs-revision" },
  { id: "all", label: "All", test: () => true },
];

export function renderTasks(container, state, data, actions) {
  container.replaceChildren();

  // filter chips
  const chips = el("div", { class: "filter-chips" });
  for (const f of FILTERS) {
    chips.append(el("button", {
      class: state.filter === f.id ? "on" : "",
      text: f.label,
      onclick: () => actions.setFilter(f.id),
    }));
  }
  container.append(chips);

  // rows: filter + search + sort (attention first, then freshness, then id)
  const q = (state.search || "").toLowerCase();
  const filter = FILTERS.find((f) => f.id === state.filter) || FILTERS[0];
  let rows = (data.rows || []).filter(filter.test);
  if (q) {
    rows = rows.filter((r) =>
      r.id.includes(q) || (r.title || "").toLowerCase().includes(q) ||
      (r.specRef || "").toLowerCase().includes(q));
  }
  rows = rows.slice().sort((a, b) =>
    rank(b) - rank(a) || (a.lastActivityAgeMs ?? 1e18) - (b.lastActivityAgeMs ?? 1e18) || (a.id < b.id ? -1 : 1));

  const table = el("table", { class: "tasks" });
  table.append(el("thead", {}, el("tr", {},
    el("th", { text: "Task" }), el("th", { text: "Title" }), el("th", { text: "Runtime" }), el("th", { text: "Result" }),
    el("th", { text: "Round" }), el("th", { text: "Last activity" }), el("th", { text: "Age" }),
  )));
  const tbody = el("tbody");
  table.append(tbody);
  container.append(table);

  if (!rows.length) {
    container.append(el("div", { class: "empty-note", text: "No tasks match this filter." }));
  }

  syncKeyed(tbody, rows.map((r) => ({ key: r.id, row: r })), (item) => tr(item.row, actions), (node, item) => {
    node.classList.toggle("selected", state.selectedTask === item.row.id);
    const act = node.querySelector(".act");
    if (act) act.textContent = item.row.lastActivity ? item.row.lastActivity.summary : "—";
  });
}

function rank(r) {
  return (r.attention === "critical" ? 4 : r.attention === "warning" ? 3 : 0) +
    (r.runtime === "active" ? 2 : r.runtime === "stalled" ? 1 : 0);
}

function tr(r, actions) {
  const node = el("tr", { onclick: () => actions.openTask(r.id) },
    el("td", { class: "id", text: r.id }),
    el("td", {}, el("span", { class: "title", text: r.title })),
    el("td", {}, badge(r.runtime, r.runtime)),
    el("td", {}, badge(r.result, r.result)),
    el("td", { text: r.round != null ? `r${r.round}/${r.roundsTotal ?? "?"}` : "—" }),
    el("td", { class: "act", text: r.lastActivity ? r.lastActivity.summary : "—" }),
    el("td", { text: fmtAge(r.lastActivityAgeMs) }),
  );
  node.dataset.key = r.id;
  return node;
}
