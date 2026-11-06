import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';
import { traceStepTargetNode } from '../lib/trace-navigation.js';

// ── Mock canvas context ──────────────────────────────────────────────────
let mockCtx;
let rafQueue = [];
beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
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
  rafQueue = [];
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  // QUEUED rAF (not synchronous): the sync mock used by the other suites
  // inverts animLoop's `animFrame = requestAnimationFrame(animLoop)`
  // handshake — nested frames complete before the assignment, leaving a
  // stale truthy animFrame so later scheduleRedraw() calls become no-ops.
  // That hides exactly what these tests must catch: the trace overlay
  // rendering after the trace state settles. Queuing matches real rAF
  // semantics (handle returned before the callback runs).
  global.requestAnimationFrame = vi.fn(cb => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();
});

// Run queued animation frames until the loop settles (camera convergence,
// drill fades). Bounded to guard against runaway loops.
async function flushFrames(maxFrames = 500) {
  let ran = 0;
  while (rafQueue.length > 0 && ran < maxFrames) {
    const batch = rafQueue.splice(0);
    for (const cb of batch) cb(performance.now());
    ran += batch.length;
    // Let Svelte effects invalidated by the flushed frames re-run before
    // the next batch (they may schedule further frames).
    await new Promise(r => setTimeout(r, 0));
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

// ── Fixtures ──────────────────────────────────────────────────────────────
//
// Diamond call graph (multi-segment qualified names → shared tree-group so
// every leaf is visible at initial fit zoom, per the task-065 dimming
// fixture pattern):
//
//   a ──calls──▶ b ──calls──▶ d
//    │
//    └─calls──▶ c ──calls──▶ e
//
//   z is unreachable from a (no causal path).

const DN = (id, node_type = 'function', extra = {}) => ({
  id, node_type, name: id, qualified_name: `app.svc.${id}`,
  file_path: 'app/svc.py', line_start: 0, line_end: 0,
  visibility: 'public', spec_confidence: 'none', test_node: false,
  ...extra,
});

const TRACE_NODES = [DN('a'), DN('b'), DN('c'), DN('d'), DN('e'), DN('z')];

const TRACE_EDGES = [
  { id: 'te1', source_id: 'a', target_id: 'b', edge_type: 'calls' },
  { id: 'te2', source_id: 'a', target_id: 'c', edge_type: 'calls' },
  { id: 'te3', source_id: 'b', target_id: 'd', edge_type: 'calls' },
  { id: 'te4', source_id: 'c', target_id: 'e', edge_type: 'calls' },
  // a is governed by a spec via a governed_by edge (no spec_path field)
  { id: 'te5', source_id: 'a', target_id: 'specs/platform-model.md', edge_type: 'governed_by' },
];

// The exact query startTraceFromNode issues (mirrors contextMenuAction('trace')
// and the toolbar button — both share that function).
function traceQueryFrom(node) {
  return {
    scope: { type: 'focus', node: node.qualified_name ?? node.name, edges: ['calls', 'routes_to'], direction: 'outgoing', depth: 15 },
    emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#22c55e', '#94a3b8'], dim_unmatched: 0.2 },
    edges: { filter: ['calls', 'routes_to'] },
    zoom: 'fit',
    annotation: { title: `Trace from: ${node.name}`, description: `Causal flow showing execution order` },
    _trace: true,
  };
}

// ── Toolbar activation ───────────────────────────────────────────────────

describe('ExplorerCanvas — causal flow trace (§6) activation', () => {
  it('toolbar Trace button issues the causal trace query for the selected node', () => {
    const onInteractiveQuery = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
        canvasState: { selectedNode: { id: 'a', name: 'a', node_type: 'function', qualified_name: 'app.svc.a' } },
        onInteractiveQuery,
      },
    });
    const btn = container.querySelector('[data-testid="toolbar-trace"]');
    expect(btn).toBeTruthy();
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onInteractiveQuery).toHaveBeenCalledTimes(1);
    expect(onInteractiveQuery).toHaveBeenCalledWith(expect.objectContaining({
      _trace: true,
      // BFS over outgoing Calls/RoutesTo edges from the selection
      scope: expect.objectContaining({
        type: 'focus',
        node: 'app.svc.a',
        edges: ['calls', 'routes_to'],
        direction: 'outgoing',
      }),
      // Spec: unreachable nodes dim to 20% opacity
      emphasis: expect.objectContaining({ dim_unmatched: 0.2 }),
      edges: { filter: ['calls', 'routes_to'] },
      annotation: expect.objectContaining({ title: 'Trace from: a' }),
    }));
  });

  it('toolbar Trace button is disabled without a selected node and issues no query', () => {
    const onInteractiveQuery = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, onInteractiveQuery },
    });
    const btn = container.querySelector('[data-testid="toolbar-trace"]');
    expect(btn).toBeTruthy();
    expect(btn.disabled).toBe(true);
    fireEvent.click(btn);
    expect(onInteractiveQuery).not.toHaveBeenCalled();
  });

  it('right-click "Trace from here" issues the same causal trace query', async () => {
    const onInteractiveQuery = vi.fn();
    // Standalone module node hit-testable at canvas center (world (0,0) →
    // screen (450,300) in jsdom, per the scope-drill fixture pattern).
    const node = { id: 'mod-a', node_type: 'module', name: 'svc', qualified_name: 'svc', file_path: 'svc.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false };
    const callee = { id: 'fn-b', node_type: 'function', name: 'handler', qualified_name: 'svc.handler', file_path: 'svc.rs', line_start: 12, line_end: 20, visibility: 'public', spec_confidence: 'none', test_node: false };
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: [node, callee],
        edges: [
          { id: 'c1', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'contains' },
          { id: 'c2', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'calls' },
        ],
        onInteractiveQuery,
      },
    });
    await new Promise(r => setTimeout(r, 50));
    const canvas = container.querySelector('canvas.treemap-canvas');
    canvas.dispatchEvent(new MouseEvent('contextmenu', { clientX: 450, clientY: 300, bubbles: true }));
    await new Promise(r => setTimeout(r, 20));

    const traceItem = container.querySelector('[data-testid="ctx-trace"]');
    expect(traceItem).toBeTruthy();
    expect(traceItem.textContent).toContain('Trace from here');
    fireEvent.click(traceItem);
    expect(onInteractiveQuery).toHaveBeenCalledTimes(1);
    expect(onInteractiveQuery).toHaveBeenCalledWith(expect.objectContaining({
      _trace: true,
      scope: expect.objectContaining({ type: 'focus', direction: 'outgoing', edges: ['calls', 'routes_to'] }),
    }));
  });
});

// ── No call graph available ──────────────────────────────────────────────

describe('ExplorerCanvas — trace without call edges', () => {
  it('shows the call-graph warning instead of entering an empty trace (toolbar)', () => {
    const onInteractiveQuery = vi.fn();
    const noCalls = TRACE_EDGES.filter(e => e.edge_type === 'governed_by');
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: TRACE_NODES,
        edges: noCalls,
        canvasState: { selectedNode: { id: 'a', name: 'a', node_type: 'function', qualified_name: 'app.svc.a' } },
        onInteractiveQuery,
      },
    });
    fireEvent.click(container.querySelector('[data-testid="toolbar-trace"]'));
    // Exact spec warning (explorer-canvas.md §6 data requirement)
    const warning = container.querySelector('[data-testid="trace-warning"]');
    expect(warning).toBeTruthy();
    expect(warning.textContent).toContain('Call graph not available — run code sync to extract call edges');
    expect(onInteractiveQuery).not.toHaveBeenCalled();
    // Dismiss clears the banner
    fireEvent.click(warning.querySelector('.trace-warning-dismiss'));
    expect(container.querySelector('[data-testid="trace-warning"]')).toBeFalsy();
  });

  it('shows the warning via the right-click "Trace from here" path too', async () => {
    const onInteractiveQuery = vi.fn();
    const node = { id: 'node-a', node_type: 'module', name: 'auth', qualified_name: 'auth', file_path: 'auth.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: [node], edges: [], onInteractiveQuery },
    });
    await new Promise(r => setTimeout(r, 50));
    const canvas = container.querySelector('canvas.treemap-canvas');
    canvas.dispatchEvent(new MouseEvent('contextmenu', { clientX: 450, clientY: 300, bubbles: true }));
    await new Promise(r => setTimeout(r, 20));

    fireEvent.click(container.querySelector('[data-testid="ctx-trace"]'));
    expect(container.querySelector('[data-testid="trace-warning"]')?.textContent)
      .toContain('Call graph not available — run code sync to extract call edges');
    expect(onInteractiveQuery).not.toHaveBeenCalled();
  });
});

// ── Exit trace mode ──────────────────────────────────────────────────────

describe('ExplorerCanvas — exiting trace mode', () => {
  it('toolbar shows "Clear trace" while a trace is active and clears it', () => {
    const onInteractiveQuery = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]), onInteractiveQuery },
    });
    const clearBtn = container.querySelector('[data-testid="toolbar-clear-trace"]');
    expect(clearBtn).toBeTruthy();
    expect(clearBtn.textContent).toContain('Clear trace');
    // No idle-state Trace button alongside
    expect(container.querySelector('[data-testid="toolbar-trace"]')).toBeFalsy();
    fireEvent.click(clearBtn);
    expect(onInteractiveQuery).toHaveBeenCalledWith(null);
  });

  it('Escape (window-level) exits trace mode', () => {
    const onInteractiveQuery = vi.fn();
    render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]), onInteractiveQuery },
    });
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onInteractiveQuery).toHaveBeenCalledWith(null);
  });

  it('Escape does not clear non-trace queries via the window listener', () => {
    const onInteractiveQuery = vi.fn();
    render(ExplorerCanvas, {
      props: {
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
        activeQuery: {
          scope: { type: 'focus', node: 'a', edges: ['calls'], direction: 'incoming', depth: 10 },
          emphasis: { dim_unmatched: 0.12 },
        },
        onInteractiveQuery,
      },
    });
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onInteractiveQuery).not.toHaveBeenCalled();
  });
});

// ── Trace rendering on the canvas ────────────────────────────────────────

describe('ExplorerCanvas — trace mode rendering', () => {
  it('renders the trace annotation with the traced node name', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]) },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toBe('Trace from: a');
    expect(container.querySelector('.annotation-desc')?.textContent).toContain('Causal flow showing execution order');
  });

  it('draws numbered step badges ①②③④⑤ for the BFS trace order', async () => {
    render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]) },
    });
    // Let effects settle (trace order computed) and animation frames run
    // (the overlay is drawn by drawFrame).
    await flushFrames();
    const texts = mockCtx.fillText.mock.calls.map(c => String(c[0]));
    // Every reachable node (a, b, c, d, e — not z) gets a step badge
    for (const step of ['1', '2', '3', '4', '5']) {
      expect(texts, `badge ${step} drawn`).toContain(step);
    }
  });

  it('dims unreachable nodes to 20% of their untraced opacity while traced nodes are boosted', async () => {
    // The query dim composes with the semantic-zoom LOD fade, so the absolute
    // opacity of a dimmed node is LOD × 0.2. Compare each node's opacity
    // under the trace query against the SAME fixture without a query: the
    // dim factor must be exactly dim_unmatched (0.2), and traced nodes must
    // be boosted to full opacity.
    const capture = () => {
      const labelAlphas = new Map();
      mockCtx.fillText = vi.fn(function (text) {
        const t = String(text);
        if (['a', 'b', 'c', 'd', 'e', 'z'].includes(t)) {
          if (!labelAlphas.has(t)) labelAlphas.set(t, []);
          labelAlphas.get(t).push(mockCtx.globalAlpha);
        }
      });
      return labelAlphas;
    };
    const baseline = capture();
    render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES },
    });
    await flushFrames();
    const traced = capture();
    render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]) },
    });
    await flushFrames();
    for (const t of ['a', 'b', 'c', 'd', 'e', 'z']) {
      expect(baseline.get(t)?.length, `leaf ${t} baseline drawn`).toBeGreaterThan(0);
      expect(traced.get(t)?.length, `leaf ${t} traced drawn`).toBeGreaterThan(0);
    }
    const zBase = baseline.get('z')[0];
    const zTraced = traced.get('z')[0];
    // Spec §6: unreachable nodes dim to 20% opacity. Tolerance 3 decimals:
    // the trace query's fit zoom shifts the LOD fade slightly between the
    // two renders (~0.04%), far below any real dim-factor regression.
    expect(zTraced / zBase).toBeCloseTo(0.2, 3);
    // Traced nodes are boosted to full opacity by the query composition
    for (const t of ['a', 'b', 'c', 'd', 'e']) {
      expect(traced.get(t)[0]).toBeCloseTo(1.0, 5);
    }
  });

  it('draws thick colored trace path arrows between numbered steps', async () => {
    // The trace overlay renders its edge labels in trace red (#fca5a5 on a
    // red label chip), distinct from the regular edge labels (#64748b,
    // zoom > 3 only). Capture the fillStyle at each 'calls' label.
    const callsLabelColors = [];
    mockCtx.fillText = vi.fn(function (text) {
      if (String(text) === 'calls') callsLabelColors.push(mockCtx.fillStyle);
    });
    render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, activeQuery: traceQueryFrom(TRACE_NODES[0]) },
    });
    await flushFrames();
    expect(callsLabelColors.length).toBeGreaterThan(0);
    expect(callsLabelColors).toContain('#fca5a5');
  });
});

// ── External focus request (detail-panel step click pan/zoom) ───────────

describe('ExplorerCanvas — focusRequest camera control', () => {
  it('pans/zooms to the requested node and selects it', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
        focusRequest: { nodeId: 'd', nonce: 1 },
      },
    });
    await flushFrames();
    // zoomToNode selects the focused node — announced in the live region
    const live = container.querySelector('.sr-only');
    expect(live?.textContent).toContain('Selected: d');
  });

  it('ignores focus requests without a nodeId', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, focusRequest: { nodeId: null, nonce: 1 } },
    });
    await flushFrames();
    const live = container.querySelector('.sr-only');
    expect(live?.textContent).not.toContain('Selected:');
  });
});

// ── Trace activation populates the detail panel (spec §6 step 6) ────────

describe('ExplorerCanvas — trace activation populates the detail panel', () => {
  const SELECTED_A = { selectedNode: { id: 'a', name: 'a', node_type: 'function', qualified_name: 'app.svc.a' } };

  it('toolbar Trace emits the start node enriched with the full step list once the trace computes', async () => {
    const onInteractiveQuery = vi.fn();
    const onNodeDetail = vi.fn();
    const { container, rerender } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, canvasState: SELECTED_A, onInteractiveQuery, onNodeDetail },
    });
    fireEvent.click(container.querySelector('[data-testid="toolbar-trace"]'));
    expect(onInteractiveQuery).toHaveBeenCalledTimes(1);
    const query = onInteractiveQuery.mock.calls[0][0];

    // Feed the query back as activeQuery (what ExplorerView does via
    // onInteractiveQuery → activeViewQuery) and let the trace effect run.
    await rerender({ activeQuery: query });
    await flushFrames();

    const enriched = onNodeDetail.mock.calls.map(c => c[0]).find(n => n?._traceSteps?.length > 0);
    expect(enriched, 'detail panel received the trace step list on activation').toBeTruthy();
    expect(enriched.id).toBe('a');
    expect(enriched._traceStep).toBe(1);
    expect(enriched._traceTotal).toBe(5);
    // BFS order over the diamond: a b c d e (z unreachable — not in the list)
    expect(enriched._traceSteps.map(s => s.nodeId)).toEqual(['a', 'b', 'c', 'd', 'e']);
    // Step annotation: a is governed by specs/platform-model.md via its
    // governed_by edge (spec §6 step 5)
    expect(enriched._traceSteps[0].specPath).toBe('specs/platform-model.md');
  });

  it('right-click "Trace from here" emits the enriched start node too', async () => {
    const onInteractiveQuery = vi.fn();
    const onNodeDetail = vi.fn();
    const node = { id: 'mod-a', node_type: 'module', name: 'svc', qualified_name: 'svc', file_path: 'svc.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false };
    const callee = { id: 'fn-b', node_type: 'function', name: 'handler', qualified_name: 'svc.handler', file_path: 'svc.rs', line_start: 12, line_end: 20, visibility: 'public', spec_confidence: 'none', test_node: false };
    const { container, rerender } = render(ExplorerCanvas, {
      props: {
        nodes: [node, callee],
        edges: [
          { id: 'c1', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'contains' },
          { id: 'c2', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'calls' },
        ],
        onInteractiveQuery,
        onNodeDetail,
      },
    });
    await new Promise(r => setTimeout(r, 50));
    const canvas = container.querySelector('canvas.treemap-canvas');
    canvas.dispatchEvent(new MouseEvent('contextmenu', { clientX: 450, clientY: 300, bubbles: true }));
    await new Promise(r => setTimeout(r, 20));
    fireEvent.click(container.querySelector('[data-testid="ctx-trace"]'));
    expect(onInteractiveQuery).toHaveBeenCalledTimes(1);

    await rerender({ activeQuery: onInteractiveQuery.mock.calls[0][0] });
    await flushFrames();

    const enriched = onNodeDetail.mock.calls.map(c => c[0]).find(n => n?._traceSteps?.length > 0);
    expect(enriched, 'detail panel received the trace step list on activation').toBeTruthy();
    // Right-click does not select the node — the emission must not depend on selection
    expect(enriched.id).toBe('mod-a');
    expect(enriched._traceSteps.map(s => s.nodeId)).toEqual(['mod-a', 'fn-b']);
  });

  it('does not re-emit on effect re-runs (data reloads keep the current detail selection)', async () => {
    const onInteractiveQuery = vi.fn();
    const onNodeDetail = vi.fn();
    const { container, rerender } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, canvasState: SELECTED_A, onInteractiveQuery, onNodeDetail },
    });
    fireEvent.click(container.querySelector('[data-testid="toolbar-trace"]'));
    await rerender({ activeQuery: onInteractiveQuery.mock.calls[0][0] });
    await flushFrames();
    expect(onNodeDetail.mock.calls.filter(c => c[0]?._traceSteps?.length > 0).length).toBe(1);

    // Same-content query object (e.g. the graph reloaded and the host
    // re-set the active query): the effect re-runs but must not clobber
    // the user's current detail selection with a re-emission.
    await rerender({ activeQuery: traceQueryFrom(TRACE_NODES[0]) });
    await flushFrames();
    expect(onNodeDetail.mock.calls.filter(c => c[0]?._traceSteps?.length > 0).length).toBe(1);
  });

  it('a new trace from a different node replaces the emission', async () => {
    const onInteractiveQuery = vi.fn();
    const onNodeDetail = vi.fn();
    const { container, rerender } = render(ExplorerCanvas, {
      props: { nodes: TRACE_NODES, edges: TRACE_EDGES, canvasState: SELECTED_A, onInteractiveQuery, onNodeDetail },
    });
    fireEvent.click(container.querySelector('[data-testid="toolbar-trace"]'));
    await rerender({ activeQuery: onInteractiveQuery.mock.calls[0][0] });
    await flushFrames();
    expect(onNodeDetail.mock.calls.some(c => c[0]?.id === 'a' && c[0]?._traceSteps)).toBe(true);

    // Clear the trace, select c, trace again: the panel must show c's trace
    fireEvent.click(container.querySelector('[data-testid="toolbar-clear-trace"]'));
    await rerender({ activeQuery: null });
    await flushFrames();
    await rerender({ canvasState: { selectedNode: { id: 'c', name: 'c', node_type: 'function', qualified_name: 'app.svc.c' } } });
    fireEvent.click(container.querySelector('[data-testid="toolbar-trace"]'));
    await rerender({ activeQuery: onInteractiveQuery.mock.calls[2][0] });
    await flushFrames();

    const enriched = onNodeDetail.mock.calls.map(c => c[0]).filter(n => n?._traceSteps?.length > 0).pop();
    expect(enriched.id).toBe('c');
    expect(enriched._traceSteps.map(s => s.nodeId)).toEqual(['c', 'e']);
  });
});

// ── Detail panel trace step list ──────────────────────────────────────

describe('NodeDetailPanel — causal flow trace step list', () => {
  const STEP_NODE = {
    id: 'a', node_type: 'function', name: 'a', qualified_name: 'app.svc.a',
    file_path: 'app/svc.py', line_start: 1, line_end: 9,
    visibility: 'public', spec_confidence: 'none', test_node: false,
  };

  const TRACE_STEPS = [
    { step: 1, nodeId: 'a', name: 'a', qualifiedName: 'app.svc.a', type: 'function', specPath: 'specs/platform-model.md', outEdges: [{ toId: 'b', edgeType: 'calls', toStep: 2 }] },
    { step: 2, nodeId: 'b', name: 'b', qualifiedName: 'app.svc.b', type: 'function', specPath: null, outEdges: [] },
    { step: 3, nodeId: 'c', name: 'c', qualifiedName: 'app.svc.c', type: 'function', specPath: null, outEdges: [] },
  ];

  it('renders the numbered trace step list with spec annotations', () => {
    const { container } = render(NodeDetailPanel, {
      props: {
        node: { ...STEP_NODE, _traceSteps: TRACE_STEPS },
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
      },
    });
    const items = container.querySelectorAll('.trace-step-item');
    expect(items.length).toBe(3);
    const nums = [...container.querySelectorAll('.trace-step-num')].map(el => el.textContent);
    expect(nums).toEqual(['1', '2', '3']);
    // Step names and types
    expect(items[0].textContent).toContain('a');
    expect(items[1].textContent).toContain('function');
    // Governing spec shown per step (spec §6 step 5)
    expect(items[0].textContent).toContain('specs/platform-model.md');
    expect(items[2].querySelector('.trace-step-spec')).toBeFalsy();
    // Current step is flagged
    expect(items[0].classList.contains('trace-current')).toBe(true);
  });

  it('clicking a step calls onTraceStepNavigate with the step (host pans the canvas)', () => {
    const onTraceStepNavigate = vi.fn();
    const { container } = render(NodeDetailPanel, {
      props: {
        node: { ...STEP_NODE, _traceSteps: TRACE_STEPS },
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
        onTraceStepNavigate,
      },
    });
    const items = container.querySelectorAll('.trace-step-item');
    fireEvent.click(items[1]);
    expect(onTraceStepNavigate).toHaveBeenCalledTimes(1);
    expect(onTraceStepNavigate).toHaveBeenCalledWith(expect.objectContaining({ step: 2, nodeId: 'b' }));
  });

  it('falls back to onNavigate when no trace-specific handler is wired', () => {
    const onNavigate = vi.fn();
    const { container } = render(NodeDetailPanel, {
      props: {
        node: { ...STEP_NODE, _traceSteps: TRACE_STEPS },
        nodes: TRACE_NODES,
        edges: TRACE_EDGES,
        onNavigate,
      },
    });
    const items = container.querySelectorAll('.trace-step-item');
    fireEvent.click(items[1]);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(expect.objectContaining({ id: 'b' }));
  });

  it('no trace section without trace steps', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: STEP_NODE, nodes: TRACE_NODES, edges: TRACE_EDGES },
    });
    expect(container.querySelector('.trace-step-list')).toBeFalsy();
  });
});

// ── Step list persistence across navigation clicks (spec §6 step 6) ─────

describe('trace step navigation keeps the step list in the detail panel', () => {
  const STEP_NODE = {
    id: 'a', node_type: 'function', name: 'a', qualified_name: 'app.svc.a',
    file_path: 'app/svc.py', line_start: 1, line_end: 9,
    visibility: 'public', spec_confidence: 'none', test_node: false,
  };

  const TRACE_STEPS = [
    { step: 1, nodeId: 'a', name: 'a', qualifiedName: 'app.svc.a', type: 'function', specPath: 'specs/platform-model.md', outEdges: [{ toId: 'b', edgeType: 'calls', toStep: 2 }] },
    { step: 2, nodeId: 'b', name: 'b', qualifiedName: 'app.svc.b', type: 'function', specPath: null, outEdges: [] },
    { step: 3, nodeId: 'c', name: 'c', qualifiedName: 'app.svc.c', type: 'function', specPath: null, outEdges: [] },
  ];

  it('traceStepTargetNode carries the steps onto the navigation target', () => {
    const detailNode = { ...STEP_NODE, _traceSteps: TRACE_STEPS };
    const step = TRACE_STEPS[1]; // click step 2 (node b)
    const target = traceStepTargetNode(detailNode, step, TRACE_NODES);
    expect(target).toBeTruthy();
    expect(target.id).toBe('b');
    // The full step list travels onto the target so the panel keeps
    // rendering it after the navigation.
    expect(target._traceSteps).toBe(TRACE_STEPS);
    expect(target._traceStep).toBe(2);
    expect(target._traceTotal).toBe(3);
  });

  it('traceStepTargetNode returns the raw target when the panel had no steps', () => {
    const target = traceStepTargetNode(STEP_NODE, TRACE_STEPS[1], TRACE_NODES);
    expect(target).toBeTruthy();
    expect(target.id).toBe('b');
    expect(target._traceSteps).toBeUndefined();
  });

  it('traceStepTargetNode returns null for an unknown node', () => {
    expect(traceStepTargetNode(STEP_NODE, { nodeId: 'nope', step: 1 }, TRACE_NODES)).toBeNull();
  });

  it('the step list survives a step click (panel re-render with the navigated node)', () => {
    // Replicates the ExplorerView onTraceStepNavigate wiring with the real
    // traceStepTargetNode: clicking a step replaces the panel's node with
    // the resolved target. The list must still render afterwards.
    const onTraceStepNavigate = vi.fn();
    const rendered = render(NodeDetailPanel, {
      props: { node: { ...STEP_NODE, _traceSteps: TRACE_STEPS }, nodes: TRACE_NODES, edges: TRACE_EDGES, onTraceStepNavigate },
    });
    expect(rendered.container.querySelectorAll('.trace-step-item').length).toBe(3);

    // Click step 2 — the panel dispatches the step to the host handler
    fireEvent.click(rendered.container.querySelectorAll('.trace-step-item')[1]);
    expect(onTraceStepNavigate).toHaveBeenCalledTimes(1);
    const step = onTraceStepNavigate.mock.calls[0][0];
    const detailNode = { ...STEP_NODE, _traceSteps: TRACE_STEPS };
    const target = traceStepTargetNode(detailNode, step, TRACE_NODES);
    expect(target.id).toBe('b');

    // ExplorerView sets detailNode = target — the panel re-renders with it.
    // The actual regression: the raw graph node carried no _traceSteps, so
    // this re-render unmounted the list and a second click was impossible.
    rendered.rerender({ node: target });
    const items = rendered.container.querySelectorAll('.trace-step-item');
    expect(items.length).toBe(3);
    expect(items[1].classList.contains('trace-current')).toBe(true);

    // A second step is still clickable — the trace can be walked from the panel
    fireEvent.click(items[2]);
    expect(onTraceStepNavigate).toHaveBeenCalledTimes(2);
  });
});
