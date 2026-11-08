import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import { activeRuns, resetActiveRuns } from '../lib/liveness.svelte.js';

/**
 * HSI §3 "The Built Software Is Always Alive" — canvas-level pins for the
 * Layer 1 liveness indicators (agent glow, fading recent-change ring,
 * Evaluative-lens gate edge colors) and the Layer 2 trace play-icon
 * scaffolding. The pure mapping logic lives in liveness.test.js; these tests
 * exercise the draw path and click wiring through the real component.
 */

let mockCtx;
let strokes;   // [{ style, shadowColor, shadowBlur, alpha }] captured at ctx.stroke()
let fills;     // [{ style }] captured at ctx.fill()
let arcs;      // [x, y, r] captured at ctx.arc()
let rafQueue;  // pending requestAnimationFrame callbacks

beforeEach(() => {
  strokes = [];
  fills = [];
  arcs = [];
  rafQueue = [];
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn((x, y, r) => { arcs.push({ x, y, r }); }),
    fill: vi.fn(() => fills.push({ style: mockCtx.fillStyle })),
    stroke: vi.fn(() => strokes.push({
      style: mockCtx.strokeStyle,
      shadowColor: mockCtx.shadowColor,
      shadowBlur: mockCtx.shadowBlur,
      alpha: mockCtx.globalAlpha,
    })),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(),
    setTransform: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
    font: '',
    textAlign: '',
    textBaseline: '',
    shadowColor: '',
    shadowBlur: 0,
    setLineDash: vi.fn(),
    getLineDash: vi.fn(() => []),
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  // Queue-based rAF: frames run only when flushed, so the continuous
  // active-glow animation loop cannot recurse synchronously forever.
  global.requestAnimationFrame = vi.fn((cb) => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();
  resetActiveRuns();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetActiveRuns();
});

/** Run up to n queued animation frames. */
function flushFrames(n = 8) {
  for (let i = 0; i < n; i++) {
    const cbs = rafQueue.splice(0);
    if (cbs.length === 0) break;
    for (const cb of cbs) cb(16 * (i + 1));
  }
}

// ── Fixtures ──────────────────────────────────────────────────────────────

// Nodes governed by specs (spec_path), spec_confidence 'none' so node
// borders are amber (#eab308) — distinct from the gate edge greens/reds.
// All leaves share one qualified-name tree ('app.svc.*') so they land in a
// single tree-group large enough to show leaves at initial fit zoom
// (same fixture shape as the filter-dimming tests).
const LNODES = [
  { id: 'g1', node_type: 'function', name: 'create_user', qualified_name: 'app.svc.create_user', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none', test_node: false },
  { id: 'g2', node_type: 'function', name: 'get_user', qualified_name: 'app.svc.get_user', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none', test_node: false },
  { id: 'g3', node_type: 'function', name: 'bill', qualified_name: 'app.svc.bill', file_path: 'app/svc.py', spec_path: 'specs/system/billing.md', spec_confidence: 'none', test_node: false },
  { id: 'g4', node_type: 'function', name: 'fill1', qualified_name: 'app.svc.fill1', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none', test_node: false },
  { id: 'g5', node_type: 'function', name: 'fill2', qualified_name: 'app.svc.fill2', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none', test_node: false },
  { id: 'g6', node_type: 'function', name: 'fill3', qualified_name: 'app.svc.fill3', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none', test_node: false },
];

const LEDGES = [
  { id: 'le1', source_id: 'g1', target_id: 'g2', edge_type: 'calls' },
  { id: 'le2', source_id: 'g2', target_id: 'g3', edge_type: 'calls' },
];

function deltaMinutesAgo(min, qns) {
  return {
    id: `d-${min}`,
    repo_id: 'r1',
    commit_sha: 'abc',
    timestamp: Math.floor((Date.now() - min * 60 * 1000) / 1000),
    spec_ref: null,
    agent_id: null,
    delta_json: JSON.stringify({ nodes_added: qns.map((qn) => ({ qualified_name: qn })) }),
  };
}

function legendText(container) {
  return Array.from(container.querySelectorAll('.legend-item')).map((el) => el.textContent);
}

// ── Layer 1: active agent glow ────────────────────────────────────────────

describe('ExplorerCanvas liveness — active agent glow', () => {
  it('draws a pulsing glow on nodes governed by an active run\'s spec', () => {
    activeRuns['agent-1'] = { agentId: 'agent-1', taskId: 't1', specRef: 'auth.md §2', startedAt: Date.now() };
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES } });
    flushFrames();
    const glowStrokes = strokes.filter((s) => s.shadowColor === '#38bdf8' && s.shadowBlur >= 4);
    expect(glowStrokes.length).toBeGreaterThan(0);
  });

  it('glow follows the run lifecycle — stops after run_finished', async () => {
    activeRuns['agent-1'] = { agentId: 'agent-1', taskId: 't1', specRef: 'auth.md §2', startedAt: Date.now() };
    const { container } = render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES } });
    flushFrames();
    expect(strokes.some((s) => s.shadowColor === '#38bdf8')).toBe(true);

    // RunFinished: tracker deletes the run; the redraw effect fires and the
    // glow must disappear. (Derived recomputation is microtask-flushed —
    // await a tick before flushing frames.) Glow strokes are identified by
    // shadowBlur > 0 — the mock ctx's restore() does not reset shadowColor,
    // so the color alone would match later non-glow strokes.
    delete activeRuns['agent-1'];
    await new Promise((r) => setTimeout(r, 0));
    strokes = [];
    flushFrames();
    expect(strokes.some((s) => s.shadowColor === '#38bdf8' && s.shadowBlur >= 4)).toBe(false);
  });

  it('no glow when no runs are active', () => {
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES } });
    flushFrames();
    expect(strokes.some((s) => s.shadowColor === '#38bdf8' && s.shadowBlur >= 4)).toBe(false);
  });

  it('shows the Agent active legend entry only while runs are active', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES } });
    flushFrames();
    expect(legendText(container).join('|')).not.toContain('Agent active');

    activeRuns['agent-1'] = { agentId: 'agent-1', taskId: 't1', specRef: 'auth.md §2', startedAt: Date.now() };
    await new Promise((r) => setTimeout(r, 0));
    flushFrames();
    expect(legendText(container).join('|')).toContain('Agent active');
  });
});

// ── Layer 1: recent-change fading ring ────────────────────────────────────

describe('ExplorerCanvas liveness — recent-change ring', () => {
  // The ring alpha is node-draw-opacity × (0.2 + 0.65·fade); each node's
  // spec border (#eab308) stroke in the same frame is drawn at exactly that
  // node's draw-opacity. Node opacities differ (zoom fade), so we check the
  // ring matches SOME border stroke scaled by the expected fade term.
  function ringFadeTermMatches(term) {
    const ring = strokes.find((s) => s.style === '#fbbf24');
    if (!ring) return false;
    return strokes.some(
      (s) => s.style === '#eab308' && Math.abs(ring.alpha - s.alpha * term) < 0.02 * s.alpha + 1e-9
    );
  }

  it('draws an amber ring on nodes changed within the last hour', () => {
    const timeline = [deltaMinutesAgo(30, ['app.svc.create_user'])];
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, timeline } });
    flushFrames();
    expect(strokes.some((s) => s.style === '#fbbf24')).toBe(true);
    // ~30 min old → fade ≈ 0.5 → ring = border × (0.2 + 0.65·0.5) ≈ 0.525
    expect(ringFadeTermMatches(0.2 + 0.65 * 0.5)).toBe(true);
  });

  it('ring fades with age — older change draws a fainter ring', () => {
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, timeline: [deltaMinutesAgo(10, ['app.svc.create_user'])] } });
    flushFrames();
    // 10 min old → fade = 1 - 10/60 → term ≈ 0.742
    expect(ringFadeTermMatches(0.2 + 0.65 * (50 / 60))).toBe(true);

    strokes = []; // reset capture between renders
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, timeline: [deltaMinutesAgo(50, ['app.svc.create_user'])] } });
    flushFrames();
    // 50 min old → fade = 1 - 50/60 → term ≈ 0.308 — fainter, never instant off
    expect(ringFadeTermMatches(0.2 + 0.65 * (10 / 60))).toBe(true);
  });

  it('no ring for changes older than an hour', () => {
    const timeline = [deltaMinutesAgo(90, ['app.svc.create_user'])];
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, timeline } });
    flushFrames();
    expect(strokes.some((s) => s.style === '#fbbf24')).toBe(false);
  });
});

// ── Layer 1: gate status edges (Evaluative lens only) ─────────────────────

describe('ExplorerCanvas liveness — gate status edges', () => {
  const PASSED = [{ mrId: 'mr1', spec_ref: 'auth.md §2', gates: [{ status: 'passed' }, { status: 'passed' }] }];
  const FAILED = [{ mrId: 'mr2', spec_ref: 'billing.md §1', gates: [{ status: 'failed' }] }];

  it('evaluative lens colors edges green when all gates passed for both endpoints', () => {
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, lens: 'evaluative', gateRecords: PASSED } });
    flushFrames();
    // g1—g2 are both governed by auth.md (all passed) → green edge
    expect(strokes.some((s) => s.style === '#22c55e')).toBe(true);
    // No failed gate anywhere → no red gate edge. (#ef4444 only appears here
    // via gate failure — selected-node borders are #ef4444 but nothing is
    // selected in this test.)
    expect(strokes.some((s) => s.style === '#ef4444')).toBe(false);
  });

  it('evaluative lens colors edges red when a connected node has a failed gate', () => {
    // g2—g3: g3 is governed by billing.md which failed → red
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, lens: 'evaluative', gateRecords: FAILED } });
    flushFrames();
    expect(strokes.some((s) => s.style === '#ef4444')).toBe(true);
  });

  it('structural lens (default) shows NO gate colors for the same gate records', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, gateRecords: [...PASSED, ...FAILED] } });
    flushFrames();
    expect(strokes.some((s) => s.style === '#22c55e')).toBe(false);
    expect(strokes.some((s) => s.style === '#ef4444')).toBe(false);
    expect(legendText(container).join('|')).not.toContain('Gates passed');
  });

  it('evaluative lens shows the gate legend entries when gate records exist', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, lens: 'evaluative', gateRecords: FAILED } });
    flushFrames();
    const text = legendText(container).join('|');
    expect(text).toContain('Gates passed');
    expect(text).toContain('Gate failed');
  });
});

// ── Layer 2: trace play icon scaffolding ──────────────────────────────────

describe('ExplorerCanvas liveness — trace play icon (Layer 2 scaffolding)', () => {
  const TRACE = {
    spans: [
      { span_id: 's1', parent_span_id: null, graph_node_id: 'g1', operation_name: 'POST /users', start_time: 0, duration_us: 5000, status: 'ok' },
      { span_id: 's2', parent_span_id: 's1', graph_node_id: 'g2', operation_name: 'db insert', start_time: 1000, duration_us: 3000, status: 'ok' },
    ],
    root_spans: ['s1'],
  };

  /** Find the play-icon backdrop circle drawn for a node (fill '#0f172a'). */
  function findPlayIcon() {
    for (let i = fills.length - 1; i >= 0; i--) {
      if (fills[i].style === '#0f172a') {
        // The most recent arc() before this fill is the backdrop circle.
        return arcs[arcs.length - 1] ?? null;
      }
    }
    return null;
  }

  it('shows a play icon on nodes with trace data', () => {
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, traceData: TRACE } });
    flushFrames();
    expect(fills.some((f) => f.style === '#0f172a')).toBe(true);
  });

  it('no play icon without trace data', () => {
    render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES } });
    flushFrames();
    expect(fills.some((f) => f.style === '#0f172a')).toBe(false);
  });

  it('clicking ▶ starts trace playback (evaluative lens + playing state)', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: LNODES, edges: LEDGES, traceData: TRACE } });
    flushFrames();
    const icon = findPlayIcon();
    expect(icon).toBeTruthy();

    const canvas = container.querySelector('canvas');
    // jsdom canvas rects are all-zero, so client coords == canvas screen coords.
    fireEvent.mouseDown(canvas, { clientX: icon.x, clientY: icon.y, button: 0 });
    fireEvent.click(canvas, { clientX: icon.x, clientY: icon.y });

    // Lens switched to evaluative
    const evalBtn = Array.from(container.querySelectorAll('.lens-group .tb-btn'))
      .find((b) => b.textContent.includes('Evaluative'));
    expect(evalBtn?.classList.contains('active')).toBe(true);
    // Playback started — the toolbar play button now shows the pause glyph
    // (two rects) instead of the play triangle.
    const pbBtn = container.querySelector('.trace-pb-btn');
    expect(pbBtn).toBeTruthy();
    expect(pbBtn.querySelectorAll('svg rect').length).toBe(2);
  });
});
