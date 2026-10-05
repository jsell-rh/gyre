// Log classification: pure functions. No filesystem, no HTTP, no DOM.
// Precedence: lifecycle > failures > actions > diagnostics > unclassified.
// Input lines come from decoded agent transcripts + driver logs; output is
// structured event records the views render as "what is this worker doing".

// Strip control chars + null bytes for display; keep raw bytes on disk.
export function cleanText(s) {
  return String(s == null ? "" : s)
    .replace(/\x00+/g, "")
    .replace(/[\x01-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "")
    .replace(/[ \t]+$/gm, "");
}

// Driver.log lines arrive prefixed: "[23:08:44] [task-173/sandbox] message"
// or "[23:08:44] message". Strip the prefixes so classification rules can
// match message shapes. Returns { text, time } — time parsed from an
// HH:MM:SS prefix (as ms offset from `now`'s midnight) or null.
export function stripDriverPrefix(line) {
  const t = cleanText(line);
  let time = null;
  let rest = t;
  let m;
  if ((m = rest.match(/^\[(\d{2}:\d{2}:\d{2})\]\s*/))) {
    const [hh, mm, ss] = m[1].split(":").map(Number);
    const d = new Date();
    d.setHours(hh, mm, ss, 0);
    time = d.getTime();
    rest = rest.slice(m[0].length);
  }
  if ((m = rest.match(/^\[(?:task-[a-z0-9-]+(?:\/[a-z0-9-]+)?)\]\s*/i))) {
    rest = rest.slice(m[0].length);
  }
  return { text: rest, time };
}

// Classify one decoded transcript/driver line.
// Returns { kind, summary, severity, causeKey } or null (not an event).
// kinds: lifecycle | failure | action | diagnostic | unclassified
export function classifyLine(line) {
  const t = cleanText(line);
  if (!t) return null;

  // --- lifecycle: round/phase/sandbox/merge milestones -------------------
  let m, m2;
  if ((m = t.match(/^(?:>>>|<<<|--+) (.*)$/))) {
    const s = m[1];
    if (/Worker round \d+/.test(s)) return ev("lifecycle", s, "round start", null);
    if (/Sandbox .* (?:deleted|created)/.test(s)) return ev("lifecycle", s, "sandbox lifecycle", null);
    if (/Implementation \(round \d+\)|Review \(round \d+\)|Auditor|Rebase resolver/.test(s)) return ev("lifecycle", s, "phase", null);
    if (/Merging|Merge aborted|worktree|Pre-flight|Orchestrator cycle|Project Manager|Loop exiting|All specs covered/.test(s)) return ev("lifecycle", s, "milestone", null);
  }
  if ((m = t.match(/^=== Sandbox (\S+) deleted/))) return ev("lifecycle", `sandbox ${m[1]} deleted`, "sandbox lifecycle", null);
  if ((m = t.match(/^--- worker log ---/))) return null; // structural marker, not an event
  if ((m = t.match(/^\[(\d{2}:\d{2}:\d{2})\] (===|>>>|<<<) (.*)$/))) return ev("lifecycle", `${m[3]} (${m[1]})`, "milestone", null);

  // Tool shapes first — checked generically (any tool name), before content
  // rules: their payloads can contain "error:" strings from the *task's*
  // domain that must not be classified as driver failures.
  if ((m = t.match(/^✓ (\w+):/))) {
    return ev("diagnostic", t.slice(2, 120), "tool-ok", null, `tool-ok:${m[1]}`);
  }
  if ((m = t.match(/^→ (\w+)\((.*)$/))) {
    // Agent tool invocations are the primary action stream.
    const tool = m[1], args = m[2];
    let summary = null, causeKey = tool;
    if (tool === "bash") {
      const cmd = (args.match(/command=(.*)$/) || [])[1] || args;
      const first = cmd.split(/\s+|;/)[0] || "cmd";
      summary = `bash ${cmd.slice(0, 90)}`;
      causeKey = `bash:${first}`;
    } else if (tool === "hub" && (m2 = args.match(/op=(\w+)/))) {
      summary = `hub ${m2[1]}`;
      causeKey = `hub:${m2[1]}`;
    } else if (tool === "edit" || tool === "write") {
      const f = (args.match(/\[([^\]#]+)/) || [])[1] || "";
      summary = `${tool === "edit" ? "editing" : "writing"} ${f}`;
      causeKey = `${tool}:${f.split("/").pop()}`;
    } else if (tool === "read" || tool === "grep" || tool === "glob" || tool === "search") {
      const f = (args.match(/path=([^,\s)]+)/) || [])[1] ||
                (args.match(/pattern=([^,\s)]+)/) || [])[1] || "";
      summary = tool === "read" ? `reading ${f || "files"}` : `searching ${f || "specs"}`;
      causeKey = `${tool}:${f.split("/").pop()}`;
    }
    return ev("action", summary || `${tool} call`, "tool-invocation", null, causeKey);
  }

  // --- failures: error exits, timeouts, aborts, alert markers ------------
  if ((m = t.match(/^!!! (?:WARNING: )?(.*)$/))) {
    return ev("failure", m[1], "alert", /danger|WIP restore|conflicts|aborted|unresolved/i.test(m[1]) ? "critical" : "warning");
  }
  if ((m = t.match(/^(\d{4}-\d{2}-\d{2}T[\d:.]+Z?)\s+(ERROR|WARN)\s+(\S+):\s*(.*)$/))) {
    // e.g. TLS warn lines from openshell CLI
    const sev = m[2] === "ERROR" ? "critical" : null; // WARN = diagnostic unless paired with failure
    if (!sev) return ev("diagnostic", `${m[3]}: ${m[4]}`.slice(0, 120), "log-warn", null, `warn:${m[3]}:${m[4].slice(0, 40)}`);
    return ev("failure", `${m[3]}: ${m[4]}`.slice(0, 120), "log-error", "critical", `error:${m[3]}`);
  }
  if (/\berror: failed to\b|\bcommand failed\b|exit code [1-9]\b|nonzero exit|EPIPE|panic:/.test(t)) {
    return ev("failure", t.slice(0, 160), "command failure", "warning", `fail:${t.slice(0, 60)}`);
  }
  if (/Merge aborted|Rebase unresolved|failed to restore stashed WIP|Unresolvable conflicts/.test(t)) {
    return ev("failure", t.slice(0, 160), "loop failure", "critical", `fail:${t.slice(0, 60)}`);
  }

  // --- actions: what the agent is doing -----------------------------------
  if ((m = t.match(/^\$ (?:edit|write|apply_patch)\b.*?([^\s\/]+)$/m))) {
    return ev("action", m[1] ? `editing ${m[1]}` : "editing files", "edit", null);
  }
  if ((m = t.match(/^\$ read\b(?:.*?([^\s\/]+\.\w+))?/))) {
    return ev("action", m[1] ? `reading ${m[1]}` : "reading files", "read", null);
  }
  if ((m = t.match(/^\$ (grep|glob|search)\b/))) return ev("action", `searching (${m[1]})`, "search", null);
  if ((m = t.match(/^\$ (?:bash\s+(?:-c\s+)?|sh\s+)?(.+)$/))) {
    const cmd = m[1];
    return ev("action", `$ ${cmd.slice(0, 90)}`, "shell", null, `sh:${cmd.split(/\s+|;/)[0]}`);
  }

  // --- diagnostics: noise that gets collapsed ------------------------------
  if ((m = t.match(/^\d{4}-\d{2}-\d{2}T[\d:.]+Z?\s+WARN\b/))) return ev("diagnostic", t.slice(0, 120), "log-warn", null, "warn:tls");
  if (/Downloaded \S+|Compiling \w+ v|Downloaded crates|Blocking waiting|Fresh \w+ v/.test(t)) {
    return ev("diagnostic", t.slice(0, 100), "cargo-progress", null, "cargo-progress");
  }
  if (/^-> /.test(t)) return ev("diagnostic", t.slice(0, 120), "tool-result", null, "tool-result");
  if (/^\[think\]/.test(t)) return ev("diagnostic", "thinking", "think", null, "think");
  if (/^===STREAM-OPEN|^===NEXT-FILE/.test(t)) return null;

  // --- unclassified: prose or unknown — keep, low priority -----------------
  if (t.length > 2) return ev("unclassified", t.slice(0, 140), "raw", null);
  return null;

  function ev(kind, summary, type, severity, causeKey) {
    return { kind, summary: cleanText(summary).slice(0, 200), type, severity: severity || null, causeKey: causeKey || `${kind}:${type}:${summary.slice(0, 40)}` };
  }
}

// Turn a timestamped driver line into an event with time; transcript lines
// (no timestamps) get the collection time.
export function classifyTranscriptLine({ taskId, time, text, rawOffset }) {
  const c = classifyLine(text);
  if (!c) return null;
  return { time, taskId, rawOffset, ...c };
}

// Classify a raw driver.log line: strips the [HH:MM:SS] [task/sandbox]
// prefix, uses the prefix clock as the event time (falling back to `now`).
export function classifyDriverLine({ taskId, now, text, rawOffset }) {
  const { text: stripped, time } = stripDriverPrefix(text);
  const c = classifyLine(stripped);
  if (!c) return null;
  return { time: time != null ? time : now, taskId, rawOffset, ...c };
}

// Collapse adjacent repeats: same task+kind+causeKey within windowMs.
// Output events carry { count, firstTime, lastTime }.
export function collapseEvents(events, windowMs = 10 * 60 * 1000) {
  const out = [];
  for (const e of events) {
    const prev = out[out.length - 1];
    if (
      prev && prev.taskId === e.taskId && prev.kind === e.kind &&
      prev.causeKey === e.causeKey && prev.type === e.type &&
      (e.time == null || prev.lastTime == null || e.time - prev.lastTime <= windowMs)
    ) {
      prev.count = (prev.count || 1) + 1;
      prev.lastTime = e.time == null ? prev.lastTime : e.time;
      // keep the newest summary (e.g. latest file edited)
      if (e.summary && e.summary !== prev.summary) prev.summary = e.summary;
    } else {
      out.push({ ...e, count: 1, firstTime: e.time, lastTime: e.time });
    }
  }
  return out;
}

// The "last meaningful activity" for a live row: latest action/lifecycle/
// failure event, skipping diagnostics entirely.
export function lastMeaningfulActivity(events) {
  for (let i = events.length - 1; i >= 0; i--) {
    const k = events[i].kind;
    if (k === "action" || k === "lifecycle" || k === "failure") return events[i];
  }
  return null;
}

// Group failure/diagnostic events into incidents by causeKey.
export function groupIncidents(events) {
  const groups = new Map();
  for (const e of events) {
    if (e.kind !== "failure" && !(e.kind === "diagnostic" && e.severity)) continue;
    const key = `${e.taskId}:${e.causeKey}`;
    let g = groups.get(key);
    if (!g) {
      g = { taskId: e.taskId, kind: e.kind, causeKey: e.causeKey, type: e.type, summary: e.summary, severity: e.severity || "warning", count: 0, firstTime: e.time, lastTime: e.time };
      groups.set(key, g);
    }
    g.count++;
    if (e.time != null) {
      if (g.firstTime == null || e.time < g.firstTime) g.firstTime = e.time;
      if (e.time > g.lastTime) g.lastTime = e.time;
    }
    if (e.summary) g.summary = e.summary;
  }
  return [...groups.values()].sort((a, b) => (b.lastTime || 0) - (a.lastTime || 0));
}
