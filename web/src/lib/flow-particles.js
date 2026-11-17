// Flow-layout particle system — pure logic (ui-layout.md §4 Flow Layout,
// human-system-interface.md §3a Test-Time Trace Capture — Explorer
// Visualization).
//
// Extracted from ExplorerCanvas so playback, paused scrubbing, step-through,
// and the test-case filter all resolve identical particle state from the
// same code path, and so the span→particle mapping is unit-testable
// without a DOM.

// Spec colors (ui-layout.md §4 Particle rendering):
// success = #3b82f6 (blue), error = #ef4444 (red).
export const SUCCESS_PARTICLE_COLOR = '#3b82f6';
export const ERROR_PARTICLE_COLOR = '#ef4444';

// Spec trail constants: "opacity decay over 200ms, 20px trail length".
export const TRAIL_FADE_MS = 200;
export const TRAIL_LENGTH_PX = 20;

// Hue shift per test-case index, in degrees. ui-layout.md §4 "Multiple test
// cases": particles from different root spans (test requests) are
// "distinguished by slight color variation (hue-shifted within the
// success/error palette)". 15° keeps the shift slight while staying
// distinguishable across a handful of concurrent test cases.
const HUE_SHIFT_DEG = 15;

/**
 * Root spans of a trace — each root span (no parent) is one test case
 * (one traced test request). Returned in start_time order so the test
 * selector and legend list cases in execution order.
 *
 * @param {Array<any>|null} spans GateTrace spans
 * @returns {Array<{spanId: string, operationName: string, index: number}>}
 */
export function rootSpanCases(spans) {
  if (!spans?.length) return [];
  const roots = spans.filter((s) => s && !s.parent_span_id && s.span_id);
  roots.sort((a, b) => (a.start_time ?? 0) - (b.start_time ?? 0));
  return roots.map((s, index) => ({
    spanId: s.span_id,
    operationName: s.operation_name ?? s.span_id,
    index,
  }));
}

/**
 * Map every span id to its root span id (the test case it belongs to).
 * Cycle-safe; a span whose parent is unknown is treated as its own root —
 * an unresolvable parent must not silently re-target the span onto
 * another test case.
 *
 * @param {Array<any>|null} spans GateTrace spans
 * @returns {Map<string, string|null>}
 */
export function spanRootMap(spans) {
  const map = new Map();
  if (!spans?.length) return map;
  const byId = new Map();
  for (const s of spans) {
    if (s?.span_id) byId.set(s.span_id, s);
  }
  for (const s of spans) {
    if (!s?.span_id) continue;
    let cur = s;
    const seen = new Set();
    while (cur?.parent_span_id) {
      if (seen.has(cur.span_id)) break; // cycle guard
      seen.add(cur.span_id);
      const parent = byId.get(cur.parent_span_id);
      if (!parent) break; // unknown parent — span acts as its own root
      cur = parent;
    }
    map.set(s.span_id, cur?.span_id ?? null);
  }
  return map;
}

// ── Color helpers ─────────────────────────────────────────────────────────

function hexToRgb(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex));
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function rgbToHex(r, g, b) {
  const c = (v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0');
  return `#${c(r)}${c(g)}${c(b)}`;
}

function rgbToHsl(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0));
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  h *= 60;
  return { h, s, l };
}

function hueToRgb(p, q, t) {
  if (t < 0) t += 1;
  if (t > 1) t -= 1;
  if (t < 1 / 6) return p + (q - p) * 6 * t;
  if (t < 1 / 2) return q;
  if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
  return p;
}

function hslToRgb(h, s, l) {
  h = ((h % 360) + 360) % 360 / 360;
  if (s === 0) return { r: l * 255, g: l * 255, b: l * 255 };
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return {
    r: hueToRgb(p, q, h + 1 / 3) * 255,
    g: hueToRgb(p, q, h) * 255,
    b: hueToRgb(p, q, h - 1 / 3) * 255,
  };
}

/**
 * Hue-shift a base color for a test case. Index 0 (and any non-positive
 * index) returns the base color unchanged — the first test case renders in
 * the exact spec color; subsequent cases shift within the palette.
 *
 * @param {string} baseHex base color (#rrggbb)
 * @param {number} caseIndex test-case index (0-based)
 * @returns {string} shifted hex color
 */
export function testCaseColor(baseHex, caseIndex) {
  if (!caseIndex || caseIndex < 0) return baseHex;
  const rgb = hexToRgb(baseHex);
  if (!rgb) return baseHex;
  const { h, s, l } = rgbToHsl(rgb.r, rgb.g, rgb.b);
  const out = hslToRgb(h + caseIndex * HUE_SHIFT_DEG, s, l);
  return rgbToHex(out.r, out.g, out.b);
}

// ── Particle resolution ───────────────────────────────────────────────────

/**
 * Particle descriptors at a trace timestamp (microseconds since trace
 * start). A particle exists for every span whose parent→child pair maps
 * onto two laid-out graph nodes and whose [start_time, start_time +
 * duration_us] window contains `currentTime` — the same resolution used
 * for live playback, paused scrubbing, and step-through, so all three
 * show identical positions for the same timestamp.
 *
 * @param {Array<any>|null} spans GateTrace spans
 * @param {number} currentTime trace timestamp (µs)
 * @param {object} [opts]
 * @param {Set<string>|Map<string, any>|null} [opts.visibleNodeIds]
 *        Nodes currently laid out (drill scoping); null = all visible.
 * @param {string|null} [opts.testFilter] root span id to animate; null = all
 *        test cases (ui-layout.md §4 Test selector).
 * @param {Map<string, string>|null} [opts.rootOf] spanId → root span id map
 *        (from spanRootMap); built internally when omitted.
 * @param {Map<string, number>|null} [opts.caseIndexByRoot] root span id →
 *        test-case index (from rootSpanCases); hue-shift defaults to 0.
 * @returns {Array<{fromId: string, toId: string, progress: number, span: any,
 *            rootSpanId: string|null, caseIndex: number, color: string, glow: string}>}
 */
export function particlesAtTime(spans, currentTime, opts = {}) {
  const {
    visibleNodeIds = null,
    testFilter = null,
    rootOf = null,
    caseIndexByRoot = null,
  } = opts;
  if (!spans?.length) return [];
  const roots = rootOf ?? spanRootMap(spans);
  // Test-case indices default to rootSpanCases order so callers that omit
  // the map still hue-shift consistently with the legend.
  let caseIdx = caseIndexByRoot;
  if (!caseIdx) {
    caseIdx = new Map();
    for (const c of rootSpanCases(spans)) caseIdx.set(c.spanId, c.index);
  }
  const spanById = new Map();
  for (const s of spans) {
    if (s?.span_id) spanById.set(s.span_id, s);
  }
  const particles = [];
  for (const span of spans) {
    if (!span?.graph_node_id || !span.parent_span_id) continue;
    const parent = spanById.get(span.parent_span_id);
    if (!parent?.graph_node_id) continue;

    const rootId = roots.get(span.span_id) ?? null;
    if (testFilter && rootId !== testFilter) continue;

    // Scope particles to the current drill level: both endpoints must be
    // laid out, or the edge crosses into a level that is not rendered.
    if (visibleNodeIds &&
        (!visibleNodeIds.has(parent.graph_node_id) || !visibleNodeIds.has(span.graph_node_id))) {
      continue;
    }

    const spanStart = span.start_time ?? 0;
    const spanEnd = spanStart + (span.duration_us || 0);
    if (currentTime < spanStart || currentTime > spanEnd) continue;

    const progress = Math.min(1, Math.max(0, (currentTime - spanStart) / (span.duration_us || 1)));
    const isError = (span.status ?? '').toLowerCase() === 'error';
    const caseIndex = caseIdx.get(rootId) ?? 0;
    const color = testCaseColor(
      isError ? ERROR_PARTICLE_COLOR : SUCCESS_PARTICLE_COLOR,
      caseIndex,
    );
    particles.push({
      fromId: parent.graph_node_id,
      toId: span.graph_node_id,
      progress,
      span,
      rootSpanId: rootId,
      caseIndex,
      color,
      glow: color,
    });
  }
  return particles;
}

/**
 * Step-through target (HSI §3a "step-through (span by span)"): the earliest
 * span start strictly after `currentTime`, or null at the end of the trace.
 * Respects the test-case filter so stepping walks only the animated spans.
 *
 * @param {Array<any>|null} spans GateTrace spans
 * @param {number} currentTime trace timestamp (µs)
 * @param {Map<string, string>|null} [rootOf] spanId → root span id map
 * @param {string|null} [testFilter] root span id, or null = all
 * @returns {number|null}
 */
export function nextSpanStart(spans, currentTime, rootOf = null, testFilter = null) {
  if (!spans?.length) return null;
  let roots = null;
  if (testFilter) {
    roots = rootOf ?? spanRootMap(spans);
  }
  let next = null;
  for (const s of spans) {
    if (!s || s.start_time == null) continue;
    if (testFilter && roots.get(s.span_id) !== testFilter) continue;
    if (s.start_time > currentTime && (next == null || s.start_time < next)) {
      next = s.start_time;
    }
  }
  return next;
}

/**
 * All spans that traversed a graph edge (parent node → child node), with
 * their timings — the evaluative-lens edge click payload (ui-layout.md §4
 * "Click edge during animation: shows all spans that traversed this edge
 * with their timings").
 *
 * @param {Array<any>|null} spans GateTrace spans
 * @param {string} srcNodeId parent span's graph node id
 * @param {string} tgtNodeId child span's graph node id
 * @returns {Array<{span_id, operation_name, status, start_time, duration_us}>}
 */
export function spansTraversingEdge(spans, srcNodeId, tgtNodeId) {
  if (!spans?.length || !srcNodeId || !tgtNodeId) return [];
  const byId = new Map();
  for (const s of spans) {
    if (s?.span_id) byId.set(s.span_id, s);
  }
  const out = [];
  for (const s of spans) {
    if (!s?.parent_span_id || !s.graph_node_id) continue;
    const parent = byId.get(s.parent_span_id);
    if (parent?.graph_node_id !== srcNodeId || s.graph_node_id !== tgtNodeId) continue;
    out.push({
      span_id: s.span_id,
      operation_name: s.operation_name,
      status: s.status,
      start_time: s.start_time,
      duration_us: s.duration_us,
    });
  }
  out.sort((a, b) => (a.start_time ?? 0) - (b.start_time ?? 0));
  return out;
}

/**
 * The particle currently at (or nearest) a graph node — the span data shown
 * when a node is clicked during animation (ui-layout.md §4 "Click node
 * during animation"). Distance is measured in edge-progress units: a
 * particle heading toward the node covers (1 − progress), one leaving it
 * covers progress.
 *
 * @param {Array<any>|null} particles particle descriptors (particlesAtTime)
 * @param {string} nodeId graph node id
 * @returns {object|null}
 */
export function nearestParticleToNode(particles, nodeId) {
  if (!particles?.length || !nodeId) return null;
  let best = null;
  let bestDist = Infinity;
  for (const p of particles) {
    let dist = Infinity;
    if (p.toId === nodeId) dist = 1 - (p.progress ?? 0);
    else if (p.fromId === nodeId) dist = p.progress ?? 0;
    if (dist < bestDist) {
      bestDist = dist;
      best = p;
    }
  }
  return best;
}

/**
 * Fading trail segments for one particle's recent position history
 * (ui-layout.md §4 "Trail": opacity decay over 200ms, 20px trail length).
 * Mutates `history` in place: points older than TRAIL_FADE_MS are dropped.
 * Stationary points produce no segments, so a paused or scrub-frozen
 * particle leaves no trail.
 *
 * @param {Array<{x: number, y: number, t: number}>|null} history
 *        screen-space positions with wall-clock timestamps (ms), oldest first
 * @param {number} now current wall-clock timestamp (ms)
 * @returns {Array<{x1: number, y1: number, x2: number, y2: number, alpha: number}>}
 *          segments ordered tail→head; alpha ramps 0→1 toward the particle
 */
export function trailSegments(history, now) {
  if (!history?.length) return [];
  while (history.length > 0 && now - history[0].t > TRAIL_FADE_MS) {
    history.shift();
  }
  if (history.length < 2) return [];

  // Walk newest→oldest until the length cap: the trail never exceeds
  // TRAIL_LENGTH_PX even when the particle moves fast — the boundary
  // segment is clipped to the remaining budget.
  const kept = [history[history.length - 1]];
  let total = 0;
  for (let i = history.length - 2; i >= 0; i--) {
    const prev = kept[kept.length - 1];
    const pt = history[i];
    const d = Math.hypot(prev.x - pt.x, prev.y - pt.y);
    if (d < 0.5) continue; // stationary point — nothing to trail
    if (total + d >= TRAIL_LENGTH_PX) {
      const budget = TRAIL_LENGTH_PX - total;
      if (budget > 0.5) {
        const f = budget / d;
        kept.push({ x: prev.x + (pt.x - prev.x) * f, y: prev.y + (pt.y - prev.y) * f, t: pt.t });
      }
      break;
    }
    total += d;
    kept.push(pt);
  }
  kept.reverse(); // oldest → newest

  const segments = [];
  for (let i = 0; i < kept.length - 1; i++) {
    const a = kept[i];
    const b = kept[i + 1];
    if (Math.hypot(b.x - a.x, b.y - a.y) < 0.5) continue; // stationary
    const age = Math.max(0, now - b.t);
    const alpha = 1 - age / TRAIL_FADE_MS;
    if (alpha <= 0) continue;
    segments.push({ x1: a.x, y1: a.y, x2: b.x, y2: b.y, alpha: Math.min(1, alpha) });
  }
  return segments;
}
