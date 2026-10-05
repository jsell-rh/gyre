// Dependencies view: task-neighborhood DAG (2 hops around a selected task),
// full graph opt-in. Prerequisites left, selected center, dependents right.
import { el, badge } from "../dom.js";

export function renderDependencies(container, state, data, actions) {
  container.replaceChildren();
  const rows = data.rows || [];
  const byId = new Map(rows.map((r) => [r.id, r]));

  // dependency edges come from the snapshot deps map (task frontmatter)
  const depMap = new Map(Object.entries(data.deps || {}));
  // fall back to per-row deps if a snapshot shape carries them
  for (const r of rows) if (r.deps && !depMap.has(r.id)) depMap.set(r.id, r.deps);

  const selected = state.selectedTask && byId.has(state.selectedTask)
    ? state.selectedTask
    : (rows.find((r) => r.runtime === "active") || rows[0] || {}).id;


  // neighborhoods: direct deps (left) and direct dependents (right)
  const deps = (depMap.get(selected) || []).filter((d) => byId.has(d));
  const dependents = [...depMap.entries()].filter(([, ds]) => ds.includes(selected)).map(([n]) => n).filter((n) => byId.has(n));

  const cols = el("div", { class: "dag-cols" });
  cols.append(dagCol("Prerequisites", deps, byId, actions));
  cols.append(dagCol("Selected", [selected], byId, actions, true));
  cols.append(dagCol("Dependents", dependents, byId, actions));
  container.append(cols);

  container.append(el("div", { class: "empty-note",
    text: deps.length + dependents.length === 0 ? "No dependency edges for this task." : "" }));
}

function dagCol(label, ids, byId, actions, center = false) {
  const col = el("div", { class: "dag-col" });
  col.append(el("h3", { text: `${label} (${ids.length})` }));
  if (!ids.length) col.append(el("div", { class: "empty-note", text: "—" }));
  for (const id of ids) {
    const r = byId.get(id);
    col.append(el("div", {
      class: "dag-node" + (center ? " center" : "") + (r && r.result === "needs-revision" ? " blocked" : ""),
      onclick: () => actions.openTask(id),
    },
      el("div", { text: r ? r.title : id }),
      el("div", {}, badge(r ? r.result : "unknown"), " ", badge(r ? r.runtime : "unknown")),
    ));
  }
  return col;
}
