// Overview view: coverage band, live work rows, attention queue, throughput,
// queue summary. The default screen — answers "healthy? coverage moved?
// what are the workers doing? what needs me?" without scrolling.
import { el, syncKeyed, fmtAge, badge } from "../dom.js";

export function renderOverview(container, state, data, actions) {
  container.replaceChildren();
  const { health, coverage, overview, queue } = data;

  // --- coverage band (visual anchor) --------------------------------------
  container.append(coverageBand(coverage));
  const grid = el("div", { class: "overview-grid" });

  const liveCol = el("div");
  const attentionCol = el("div");

  // LIVE WORK
  const live = overview.filter((r) => r.runtime === "active" || r.runtime === "stalled");
  liveCol.append(
    el("h2", { class: "section", text: `Live work` }, el("span", { class: "count", text: ` ${live.length}` })),
  );
  const liveList = el("div");
  liveCol.append(liveList);
  syncKeyed(liveList, live.map((r) => ({ key: r.id, row: r })), (item) => liveRow(item.row, actions), (node, item) => {
    node.classList.toggle("stalled", item.row.runtime === "stalled");
    node.classList.toggle("selected", state.selectedTask === item.row.id);
    const act = node.querySelector(".act");
    if (act && item.row.lastActivity) act.textContent = item.row.lastActivity.summary;
    const meta = node.querySelector(".meta");
    if (meta) meta.textContent = rowMeta(item.row, data);
  });
  if (!live.length) liveCol.append(el("div", { class: "empty-note", text: "No live workers — fleet idle." }));

  // THROUGHPUT (under live work)
  liveCol.append(
    el("h2", { class: "section", text: "Throughput" }),
    el("div", {
      class: "summary-line",
      html: throughputHTML(data),
    }),
  );

  // ATTENTION
  const attention = overview.filter((r) => r.attention !== "none");
  attentionCol.append(
    el("h2", { class: "section", text: "Attention" }, el("span", { class: "count", text: ` ${attention.length}` })),
  );
  if (!attention.length) {
    attentionCol.append(el("div", { class: "attention-empty", text: "Nothing needs you." }));
  }
  for (const r of attention.slice(0, 12)) {
    attentionCol.append(attentionItem(r, data, actions));
  }

  grid.append(liveCol, attentionCol);
  container.append(grid);

  // --- queue summary ---------------------------------------------------------
  container.append(
    el("h2", { class: "section", text: "Queue" }),
    el("div", { class: "summary-line", html: queueHTML(queue, data) }),
  );
}

// Coverage band: big verified% number, explicit arithmetic (the percent and
// the displayed numbers must agree), trend sparkline from git history.
function coverageBand(coverage) {
  const band = el("section", { class: "coverage" });
  if (!coverage || !coverage.precise) {
    band.append(el("div", { class: "empty-note", text: "No coverage data." }));
    return band;
  }
  const p = coverage.precise;
  const done = p.implemented + p.verified;
  band.append(
    el("div", { class: "cov-number" },
      `${p.pct.toFixed(1)}%`,
      el("small", { text: "verified + implemented" }),
    ),
    el("div", { class: "cov-detail", html:
      `<b>${p.verified}</b> verified + <b>${p.implemented}</b> implemented` +
      ` = <b>${done}</b> of <b>${p.applicable}</b> applicable sections` +
      ` <span class="muted">(${p.na} n/a excluded · ${p.assigned} assigned)</span>` }),
    coverageSparkline(coverage.trend || []),
  );
  return band;
}

// Simple inline SVG sparkline of coverage history points [{t, pct}].
function coverageSparkline(trend) {
  const W = 220, H = 36, PAD = 2;
  const wrap = el("div", { class: "cov-chart" });
  if (!trend.length) {
    wrap.append(el("span", { class: "muted", text: "no trend history yet" }));
    return wrap;
  }
  const pts = trend.slice(-60); // last 60 commits
  const xs = pts.map((p, i) => PAD + (i / Math.max(1, pts.length - 1)) * (W - 2 * PAD));
  const lo = Math.min(...pts.map((p) => p.pct)), hi = Math.max(...pts.map((p) => p.pct));
  const span = Math.max(0.5, hi - lo);
  const ys = pts.map((p) => H - PAD - ((p.pct - lo) / span) * (H - 2 * PAD));
  const d = xs.map((x, i) => `${i ? "L" : "M"}${x.toFixed(1)},${ys[i].toFixed(1)}`).join(" ");
  const area = `${d} L${xs[xs.length - 1].toFixed(1)},${H} L${xs[0].toFixed(1)},${H} Z`;
  const svg = `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" aria-hidden="true">` +
    `<path d="${area}" fill="var(--pf-blue-400, #2b9af3)" opacity="0.15"/>` +
    `<path d="${d}" fill="none" stroke="var(--pf-blue-300, #73bcf7)" stroke-width="1.5"/>` +
    `</svg>`;
  const first = pts[0].pct, last = pts[pts.length - 1].pct;
  const delta = last - first;
  wrap.innerHTML = svg + `<span class="cov-delta ${delta >= 0 ? "up" : "down"}">${delta >= 0 ? "▲" : "▼"} ${Math.abs(delta).toFixed(1)}pp over ${pts.length} commits</span>`;
  return wrap;
}

function liveRow(r, actions) {
  const node = el("div", {
    class: "live-row" + (r.runtime === "stalled" ? " stalled" : ""),
    onclick: () => actions.openTask(r.id),
  },
    el("span", { class: "pulse" }),
    el("span", { class: "id", text: r.id }),
    el("span", { class: "title", text: r.title }),
    el("span", { class: "act", text: r.lastActivity ? r.lastActivity.summary : "no classified activity" }),
    el("span", { class: "meta" }),
    badge(r.result, r.result),
    r.attention !== "none" ? badge(`! ${r.attention}`, `attention-${r.attention}`) : null,
  );
  node.dataset.key = r.id;
  return node;
}

function rowMeta(r, data) {
  const parts = [];
  if (r.round != null) parts.push(`r${r.round}/${r.roundsTotal ?? "?"}`);
  parts.push(r.workflow);
  if (r.lastActivityAgeMs != null) parts.push(fmtAge(r.lastActivityAgeMs));
  return parts.join(" · ");
}

function attentionItem(r, data, actions) {
  const why = [];
  if (r.attention === "critical") why.push("critical incident");
  if (r.runtime === "stalled") why.push("stalled");
  if (r.result === "needs-revision") why.push("needs revision");
  return el("div", {
    class: "attention-item " + (r.attention === "critical" ? "critical" : ""),
    onclick: () => actions.openTask(r.id),
  },
    el("div", { class: "head" },
      el("span", { class: "id", text: r.id }),
      el("span", { text: r.title }),
    ),
    el("div", { class: "why", text: why.join(" · ") + (r.lastActivityAgeMs != null ? ` · last activity ${fmtAge(r.lastActivityAgeMs)}` : "") }),
  );
}

function throughputHTML(data) {
  const h = data.health || {};
  const act = h.workersActive ?? 0;
  const stalled = h.workersStalled ?? 0;
  const rev = data.queue?.["needs-revision"] ?? 0;
  return `<b>${act}</b> active · <b>${stalled}</b> stalled · <b>${rev}</b> revisions outstanding`;
}

function queueHTML(queue, data) {
  const total = (data.rows || []).length;
  const q = queue || {};
  const parts = [
    `<b>${q["needs-revision"] ?? 0}</b> needs-revision`,
    `<b>${q["in-progress"] ?? 0}</b> in-progress`,
    `<b>${q["not-started"] ?? 0}</b> not-started`,
    `<b>${q.complete ?? 0}</b> complete`,
    `<b>${q.unknown ?? 0}</b> unknown`,
    `· <b>${total}</b> tasks tracked`,
  ];
  return parts.join(" · ") + ` · <a href="#" data-nav="tasks">view all tasks</a>`;
}
