/**
 * ExplorerGeneratedViews.test.js — HSI §3 "Generated Views (LLM-Powered,
 * On-Demand)" (task-089), ExplorerView integration.
 *
 * Full-tree render (graph load, ExplorerCanvas, chat column) verifying the
 * wiring between the pieces the component tests can't see:
 *
 *   - the chat column's mode tabs switch between the Explorer agent chat
 *     and the Generated Views panel
 *   - a generated view is applied to the ExplorerCanvas: the LLM's
 *     explanation renders as the canvas query annotation (the sidebar
 *     annotation alongside the graph) and the data layer drives the
 *     matched nodes
 *   - a "flow" layout view fetches the trace_source MR's captured trace
 *     (the data the particle animation replays)
 *
 * The WS explorer chat is mocked (as in ExplorerViewShortcuts.test.js); the
 * generate endpoint is exercised through api.generateExplorerView with an
 * SSE body shaped exactly like the server's.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// ── API mock ─────────────────────────────────────────────────────────────
// Must be before the ExplorerView import so vi.mock runs first.
const generateExplorerView = vi.fn();
const mrTrace = vi.fn();

vi.mock('../lib/api.js', () => {
  const nodes = [
    { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
    { id: 'fn1', node_type: 'function', name: 'create_user', qualified_name: 'api.create_user', file_path: 'api/handlers.py', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
    { id: 'type1', node_type: 'type', name: 'User', qualified_name: 'domain.User', file_path: 'domain/models.py', line_start: 1, line_end: 20, visibility: 'public', spec_confidence: 'high', test_node: false },
  ];
  const edges = [
    { id: 'e1', source_id: 'pkg1', target_id: 'fn1', edge_type: 'contains' },
    { id: 'e2', source_id: 'pkg1', target_id: 'type1', edge_type: 'contains' },
    { id: 'e3', source_id: 'fn1', target_id: 'type1', edge_type: 'calls' },
  ];
  return {
    api: {
      workspaces: vi.fn().mockResolvedValue([]),
      workspaceBudget: vi.fn().mockResolvedValue(null),
      repos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
      workspaceRepos: vi.fn().mockResolvedValue([]),
      allRepos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
      repoGraph: vi.fn().mockResolvedValue({ nodes, edges }),
      getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
      generateExplorerView: (...args) => generateExplorerView(...args),
      mrTrace: (...args) => mrTrace(...args),
      mergeRequests: vi.fn().mockResolvedValue({ merge_requests: [] }),
      graphQueryDryrun: vi.fn().mockResolvedValue(null),
    },
  };
});

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import ExplorerView from '../components/ExplorerView.svelte';

// ── Shared mocks (canvas context, rAF, WebSocket) ────────────────────────

let mockCtx;
let sockets;
let rafQueue;

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
    roundRect: vi.fn(),
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  rafQueue = [];
  global.requestAnimationFrame = vi.fn(cb => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();

  sockets = [];
  global.WebSocket = class MockWebSocket {
    constructor(url) {
      this.url = url;
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
      sockets.push(this);
    }
  };

  global.window.matchMedia = vi.fn((query) => ({
    matches: false,
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));

  localStorage.setItem('gyre_auth_token', 'test-token');

  generateExplorerView.mockReset();
  mrTrace.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
  sockets = [];
  rafQueue = [];
});

function flushRaf(maxFrames = 3000) {
  let batches = 0;
  while (rafQueue.length > 0 && batches < maxFrames) {
    const batch = rafQueue;
    rafQueue = [];
    for (const cb of batch) cb(performance.now());
    batches++;
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────

const REPO_SCOPE = { type: 'repo', repoId: 'test-repo-1', workspaceId: 'ws1' };

async function renderLoadedExplorerView() {
  const result = render(ExplorerView, {
    props: { scope: REPO_SCOPE },
    context: new Map([
      ['navigate', vi.fn()],
      ['goToWorkspaceSettings', vi.fn()],
    ]),
  });
  await waitFor(() => {
    expect(result.container.querySelector('.treemap-canvas')).toBeTruthy();
  });
  await waitFor(() => {
    expect(result.container.querySelector('.concept-input').disabled).toBe(false);
  });
  return result;
}

function sseResponse(events) {
  const text = events
    .map(({ event, data }) => `event: ${event}\ndata: ${JSON.stringify(data)}\n`)
    .join('\n');
  return { ok: true, status: 200, text: () => Promise.resolve(text) };
}

const AUTH_SPEC = {
  name: 'How auth works',
  description: 'Authentication view',
  data: { concept: 'auth', node_types: ['Module', 'Function'], edge_types: ['Contains'], depth: 2 },
  // Canvas-rendered layout: this fixture pins the ViewQuery translation +
  // canvas query annotation (the LLM explanation in the sidebar). Since
  // task-177 the `layout` field selects the renderer through the layout
  // registry — 'hierarchical' would render on the SVG canvas instead
  // (covered by ExplorerViewLayoutRenderers.test.js), so the annotation
  // fixture uses the default canvas layout.
  layout: 'graph',
  explanation: 'Authentication flows through require_auth_middleware.',
};

const FLOW_SPEC = {
  name: 'Payment retry flow',
  data: {
    concept: 'retry',
    node_types: ['Function'],
    edge_types: [],
    depth: 3,
    trace_source: { mr_id: 'mr-flow-47' },
  },
  layout: 'flow',
  explanation: 'Retry requests flow from the endpoint through the gateway.',
};

const TRACE = {
  id: 'trace-1',
  mr_id: 'mr-flow-47',
  spans: [
    { span_id: 's1', parent_span_id: null, operation_name: 'POST /payments/retry', service_name: 'payment-api', kind: 'server', start_time: 0, duration_us: 500, attributes: {}, status: 'ok', graph_node_id: 'fn1' },
    { span_id: 's2', parent_span_id: 's1', operation_name: 'PaymentGateway::charge', service_name: 'payment-api', kind: 'internal', start_time: 100, duration_us: 300, attributes: {}, status: 'ok', graph_node_id: 'fn1' },
  ],
  root_spans: ['s1'],
};

// ── Tests ────────────────────────────────────────────────────────────────

describe('ExplorerView — HSI §3 Generated Views', () => {
  it('mode tabs switch the chat column between Explorer chat and Generated Views', async () => {
    const { container } = await renderLoadedExplorerView();
    await waitFor(() => expect(sockets.length).toBeGreaterThan(0));

    // Default: the WS Explorer chat is mounted.
    expect(container.querySelector('.chat-input')).toBeTruthy();
    expect(container.querySelector('.gen-input')).toBeFalsy();

    // Switch to Generated Views.
    const genTab = container.querySelector('.chat-mode-tab:nth-child(2)');
    expect(genTab.textContent).toBe('Generated Views');
    fireEvent.click(genTab);

    await waitFor(() => {
      expect(container.querySelector('.gen-input')).toBeTruthy();
    });
    expect(container.querySelector('.chat-input')).toBeFalsy();

    // And back.
    fireEvent.click(container.querySelector('.chat-mode-tab:nth-child(1)'));
    await waitFor(() => {
      expect(container.querySelector('.chat-input')).toBeTruthy();
    });
    expect(container.querySelector('.gen-input')).toBeFalsy();
  });

  it('applies a generated view to the canvas with the LLM explanation as the query annotation', async () => {
    generateExplorerView.mockResolvedValue(
      sseResponse([{ event: 'complete', data: { view_spec: AUTH_SPEC, explanation: AUTH_SPEC.explanation } }])
    );
    const { container } = await renderLoadedExplorerView();

    fireEvent.click(container.querySelector('.chat-mode-tab:nth-child(2)'));
    const input = await waitFor(() => container.querySelector('.gen-input'));
    await waitFor(() => expect(input.disabled).toBe(false));

    fireEvent.input(input, { target: { value: 'How does authentication work?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    // The generate call carries the workspace + repo scope.
    await waitFor(() => {
      expect(generateExplorerView).toHaveBeenCalledWith('ws1', {
        question: 'How does authentication work?',
        repo_id: 'test-repo-1',
      });
    });

    // The canvas renders the query annotation: the LLM's explanation as a
    // sidebar annotation alongside the graph (HSI §3).
    await waitFor(() => {
      const annotation = container.querySelector('.query-annotation');
      expect(annotation).toBeTruthy();
      expect(annotation.textContent).toContain('How auth works');
      expect(annotation.textContent).toContain('Authentication flows through require_auth_middleware.');
    });
    flushRaf(50);
  });

  it('a flow layout view fetches the trace_source MR trace for the particle animation', async () => {
    mrTrace.mockResolvedValue(TRACE);
    generateExplorerView.mockResolvedValue(
      sseResponse([{ event: 'complete', data: { view_spec: FLOW_SPEC, explanation: FLOW_SPEC.explanation } }])
    );
    const { container } = await renderLoadedExplorerView();

    fireEvent.click(container.querySelector('.chat-mode-tab:nth-child(2)'));
    const input = await waitFor(() => container.querySelector('.gen-input'));
    await waitFor(() => expect(input.disabled).toBe(false));

    fireEvent.input(input, { target: { value: 'How does a payment retry flow through the system?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mrTrace).toHaveBeenCalledWith('mr-flow-47');
    });

    // Playback switches the canvas to the evaluative lens (where trace
    // particles render) — the lens toggle reflects it.
    await waitFor(() => {
      const evalBtn = [...container.querySelectorAll('.tb-btn')].find(
        (b) => b.textContent.trim() === 'Evaluative'
      );
      expect(evalBtn).toBeTruthy();
      expect(evalBtn.classList.contains('active')).toBe(true);
    });
    flushRaf(50);
  });
});
