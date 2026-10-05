// Incidents view: failures + severe diagnostics grouped by cause, with
// recurrence counts, affected tasks, first/last occurrence.
import { el, fmtAge, fmtClock } from "../dom.js";

export function renderIncidents(container, state, data, actions) {
  container.replaceChildren();
  const incidents = data.incidents || [];

  if (!incidents.length) {
    container.append(el("div", { class: "empty-note", text: "No incidents recorded this session. (Incidents accumulate from live streams; reload resets them.)" }));
    return;
  }

  for (const inc of incidents) {
    const node = el("div", { class: "incident-group" + (inc.severity === "critical" ? " critical" : "") },
      el("div", { class: "head" },
        el("span", { class: `badge ${inc.severity}`, text: inc.severity }),
        el("span", { class: "count", text: inc.count + "×" }),
        el("span", { class: "id mono", text: inc.taskId }),
        el("span", { text: inc.summary }),
        el("span", { class: "when", text: `${fmtClock(inc.firstTime)} → ${fmtClock(inc.lastTime)}` }),
      ),
      el("details", {},
        el("summary", { text: "details" }),
        el("div", { class: "mono", text: `kind: ${inc.kind}\ntype: ${inc.type}\ncauseKey: ${inc.causeKey}` }),
      ),
    );
    node.addEventListener("click", (e) => {
      if (e.target.tagName === "SUMMARY") return;
      actions.openTask(inc.taskId);
    });
    container.append(node);
  }
}
